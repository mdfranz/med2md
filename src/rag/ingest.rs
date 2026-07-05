use rig_core::embeddings::{EmbeddingModel, EmbeddingsBuilder};

use crate::embed::EmbedProvider;
use crate::llm_config::EmbedConfig;
use crate::rag::schema::{self, Chunk};

const MAX_CHUNK_CHARS: usize = 1500;
const MIN_CHUNK_CHARS: usize = 40;
const EMBED_BATCH_SIZE: usize = 96;

/// Split article markdown into heading-scoped, paragraph-accumulated chunks.
/// Returns `(heading, text)` pairs; `heading` is empty for content preceding
/// the first heading. Chunks under `MIN_CHUNK_CHARS` are dropped.
pub fn chunk_markdown(content: &str) -> Vec<(String, String)> {
    let heading_re = regex::Regex::new(r"(?m)^#{1,6}[ \t].*$").unwrap();
    let matches: Vec<regex::Match> = heading_re.find_iter(content).collect();

    let mut sections: Vec<(String, String)> = Vec::new();
    if matches.is_empty() {
        sections.push((String::new(), content.to_string()));
    } else {
        let first_start = matches[0].start();
        if first_start > 0 {
            sections.push((String::new(), content[..first_start].to_string()));
        }
        for (i, m) in matches.iter().enumerate() {
            let heading_text = content[m.start()..m.end()].trim_start_matches('#').trim().to_string();
            let body_start = m.end();
            let body_end = matches.get(i + 1).map(|next| next.start()).unwrap_or(content.len());
            sections.push((heading_text, content[body_start..body_end].to_string()));
        }
    }

    let mut chunks: Vec<(String, String)> = Vec::new();
    for (heading, body) in sections {
        let paragraphs: Vec<&str> = body
            .split("\n\n")
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();

        let mut current = String::new();
        for p in paragraphs {
            if p.chars().count() > MAX_CHUNK_CHARS {
                if !current.is_empty() {
                    chunks.push((heading.clone(), std::mem::take(&mut current)));
                }
                chunks.push((heading.clone(), p.to_string()));
                continue;
            }

            if !current.is_empty() && current.chars().count() + p.chars().count() + 2 > MAX_CHUNK_CHARS {
                chunks.push((heading.clone(), std::mem::take(&mut current)));
            }

            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(p);
        }

        if !current.is_empty() {
            chunks.push((heading, current));
        }
    }

    chunks
        .into_iter()
        .filter(|(_, text)| text.chars().count() >= MIN_CHUNK_CHARS)
        .collect()
}

fn markdown_files(output_dir: &str) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map(|e| e == "md").unwrap_or(false) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

pub async fn run_index(output_dir: &str, embed_config: &EmbedConfig) -> Result<(), String> {
    let embed_provider = EmbedProvider::build(embed_config)?;
    let dims = embed_provider.ndims();
    let provider_name = embed_config.provider.name();

    let (table, rebuilt) = schema::open_or_rebuild_table(output_dir, provider_name, &embed_config.model, dims).await?;
    if rebuilt {
        tracing::info!(
            provider = provider_name,
            model = &embed_config.model,
            dims,
            "RAG index table rebuilt (embedding configuration changed or table was missing)"
        );
    }

    let files = markdown_files(output_dir);
    tracing::info!(count = files.len(), directory = %output_dir, "Found markdown files for indexing");

    let mut total_chunks = 0usize;
    let mut total_files_indexed = 0usize;

    for file in &files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(file = %file.display(), error = %e, "Failed to read markdown file for indexing");
                continue;
            }
        };

        let slug = file.file_stem().and_then(|s| s.to_str()).unwrap_or("article").to_string();
        let source_path = file.to_string_lossy().to_string();

        let raw_chunks = chunk_markdown(&content);
        if raw_chunks.is_empty() {
            continue;
        }

        let chunk_structs: Vec<Chunk> = raw_chunks
            .into_iter()
            .enumerate()
            .map(|(idx, (heading, text))| Chunk {
                id: format!("{}#{}", slug, idx),
                slug: slug.clone(),
                source_path: source_path.clone(),
                heading,
                content: text,
            })
            .collect();

        for batch in chunk_structs.chunks(EMBED_BATCH_SIZE) {
            let records = EmbeddingsBuilder::new(embed_provider.clone())
                .documents(batch.to_vec())
                .map_err(|e| e.to_string())?
                .build()
                .await
                .map_err(|e| e.to_string())?;

            let batch_len = records.len();
            let record_batch = schema::as_record_batch(records, dims).map_err(|e| e.to_string())?;
            let reader = arrow_array::RecordBatchIterator::new(vec![Ok(record_batch)], schema::arrow_schema(dims));

            let mut merge_builder = table.merge_insert(&["id"]);
            merge_builder.when_matched_update_all(None);
            merge_builder.when_not_matched_insert_all();
            merge_builder
                .execute(Box::new(reader))
                .await
                .map_err(|e| e.to_string())?;

            total_chunks += batch_len;
        }

        total_files_indexed += 1;
        tracing::info!(
            file = %file.display(),
            chunks = chunk_structs.len(),
            "Successfully indexed article"
        );
    }

    tracing::info!(
        total_chunks,
        total_files_indexed,
        "RAG indexing run completed"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_headings_and_tracks_heading_text() {
        let md = "# Title\n\nIntro paragraph that is long enough to survive the minimum chunk length filter.\n\n## Section One\n\nBody text for section one that is long enough to survive filtering.\n\n## Section Two\n\nMore body text here that is long enough to survive filtering.";
        let chunks = chunk_markdown(md);
        assert!(chunks.iter().any(|(h, _)| h == "Title"));
        assert!(chunks.iter().any(|(h, _)| h == "Section One"));
        assert!(chunks.iter().any(|(h, _)| h == "Section Two"));
    }

    #[test]
    fn content_before_first_heading_has_empty_heading() {
        let md = "Some preamble text that is long enough to not be dropped by filtering.\n\n# Heading\n\nBody.";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks[0].0, "");
    }

    #[test]
    fn drops_short_chunks() {
        let md = "# Heading\n\nHi.";
        let chunks = chunk_markdown(md);
        assert!(chunks.is_empty());
    }

    #[test]
    fn keeps_long_paragraph_whole_without_splitting() {
        let long_para = "x".repeat(2000);
        let md = format!("# Heading\n\n{}", long_para);
        let chunks = chunk_markdown(&md);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].1.chars().count(), 2000);
    }

    #[test]
    fn accumulates_short_paragraphs_up_to_limit() {
        let para = "y".repeat(100);
        let md = format!("# Heading\n\n{p}\n\n{p}\n\n{p}", p = para);
        let chunks = chunk_markdown(&md);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].1.contains(&para));
    }
}
