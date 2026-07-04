use rig_core::vector_store::VectorStoreIndex;
use rig_core::vector_store::request::VectorSearchRequest;
use rig_lancedb::LanceDbVectorIndex;

use crate::embed::EmbedProvider;
use crate::rag::schema::Chunk;

pub async fn retrieve(
    index: &LanceDbVectorIndex<EmbedProvider>,
    question: &str,
    n: usize,
) -> Result<Vec<Chunk>, String> {
    let req = VectorSearchRequest::builder()
        .query(question)
        .samples(n as u64)
        .build();

    let results: Vec<(f64, String, Chunk)> = index.top_n(req).await.map_err(|e| e.to_string())?;
    Ok(results.into_iter().map(|(_, _, chunk)| chunk).collect())
}

pub fn build_preamble(chunks: &[Chunk]) -> String {
    if chunks.is_empty() {
        return "You are a helpful assistant. No relevant excerpts were found in the user's \
                 indexed articles for this question. Say you don't know rather than guessing."
            .to_string();
    }

    let mut preamble = String::from(
        "You are a helpful assistant that answers questions using ONLY the excerpts below, \
         taken from the user's downloaded articles. If the excerpts do not contain the answer, \
         say you don't know rather than relying on outside knowledge. Cite the source using its \
         slug in brackets, like [slug], when you use an excerpt.\n\n",
    );
    for chunk in chunks {
        preamble.push_str(&format!("--- [{}] {} ---\n{}\n\n", chunk.slug, chunk.heading, chunk.content));
    }
    preamble
}
