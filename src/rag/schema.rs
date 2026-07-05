use std::sync::Arc;

use arrow_array::{ArrayRef, FixedSizeListArray, RecordBatch, StringArray, types::Float64Type};
use arrow_schema::{ArrowError, DataType, Field, Schema};
use rig_core::embeddings::Embedding;
use rig_core::{Embed, OneOrMany};
use serde::{Deserialize, Serialize};

pub const TABLE_NAME: &str = "chunks";

#[derive(Embed, Clone, Serialize, Deserialize, Debug)]
pub struct Chunk {
    pub id: String,
    pub slug: String,
    pub source_path: String,
    pub heading: String,
    #[embed]
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct RagMeta {
    pub embed_provider: String,
    pub embed_model: String,
    pub dims: usize,
}

pub fn rag_dir(output_dir: &str) -> String {
    format!("{}/.rag", output_dir)
}

pub fn lancedb_dir(output_dir: &str) -> String {
    format!("{}/lancedb", rag_dir(output_dir))
}

pub fn meta_path(output_dir: &str) -> String {
    format!("{}/meta.json", rag_dir(output_dir))
}

pub fn read_meta(output_dir: &str) -> Option<RagMeta> {
    let content = std::fs::read_to_string(meta_path(output_dir)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn write_meta(output_dir: &str, meta: &RagMeta) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(meta_path(output_dir), json).map_err(|e| e.to_string())
}

pub fn arrow_schema(dims: usize) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("slug", DataType::Utf8, false),
        Field::new("source_path", DataType::Utf8, false),
        Field::new("heading", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float64, true)), dims as i32),
            false,
        ),
    ]))
}

pub fn as_record_batch(
    records: Vec<(Chunk, OneOrMany<Embedding>)>,
    dims: usize,
) -> Result<RecordBatch, ArrowError> {
    let id = StringArray::from_iter_values(records.iter().map(|(c, _)| c.id.clone()));
    let slug = StringArray::from_iter_values(records.iter().map(|(c, _)| c.slug.clone()));
    let source_path = StringArray::from_iter_values(records.iter().map(|(c, _)| c.source_path.clone()));
    let heading = StringArray::from_iter_values(records.iter().map(|(c, _)| c.heading.clone()));
    let content = StringArray::from_iter_values(records.iter().map(|(c, _)| c.content.clone()));

    let embedding = FixedSizeListArray::from_iter_primitive::<Float64Type, _, _>(
        records
            .into_iter()
            .map(|(_, embeddings)| {
                Some(embeddings.first().vec.into_iter().map(Some).collect::<Vec<_>>())
            })
            .collect::<Vec<_>>(),
        dims as i32,
    );

    RecordBatch::try_new(
        arrow_schema(dims),
        vec![
            Arc::new(id) as ArrayRef,
            Arc::new(slug) as ArrayRef,
            Arc::new(source_path) as ArrayRef,
            Arc::new(heading) as ArrayRef,
            Arc::new(content) as ArrayRef,
            Arc::new(embedding) as ArrayRef,
        ],
    )
}

/// Open the existing chat-time table read-only (no create/rebuild). Returns
/// `None` if the store or table hasn't been created yet (run `--index` first).
pub async fn open_existing_table(output_dir: &str) -> Option<lancedb::Table> {
    let dir = lancedb_dir(output_dir);
    if !std::path::Path::new(&dir).exists() {
        return None;
    }
    let db = lancedb::connect(&dir).execute().await.ok()?;
    let table_names = db.table_names().execute().await.ok()?;
    if !table_names.iter().any(|n| n == TABLE_NAME) {
        return None;
    }
    db.open_table(TABLE_NAME).execute().await.ok()
}

/// Connect to the local LanceDB store, dropping and recreating the table when
/// the embedding provider/model/dims recorded in `meta.json` no longer match
/// (avoids an Arrow schema mismatch on mixed-dimension re-indexing).
pub async fn open_or_rebuild_table(
    output_dir: &str,
    embed_provider: &str,
    embed_model: &str,
    dims: usize,
) -> Result<(lancedb::Table, bool), String> {
    let dir = lancedb_dir(output_dir);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;

    let db = lancedb::connect(&dir).execute().await.map_err(|e| e.to_string())?;

    let existing_meta = read_meta(output_dir);
    let meta_matches = existing_meta
        .as_ref()
        .map(|m| m.embed_provider == embed_provider && m.embed_model == embed_model && m.dims == dims)
        .unwrap_or(false);

    let table_names = db.table_names().execute().await.map_err(|e| e.to_string())?;
    let table_exists = table_names.iter().any(|n| n == TABLE_NAME);

    let rebuilt = !meta_matches || !table_exists;

    let table = if rebuilt {
        tracing::warn!(
            table_exists,
            meta_matches,
            provider = embed_provider,
            model = embed_model,
            dims,
            "Rebuilding RAG index table due to configuration mismatch or missing table"
        );
        if table_exists {
            db.drop_table(TABLE_NAME, &[]).await.map_err(|e| e.to_string())?;
        }
        let table = db
            .create_empty_table(TABLE_NAME, arrow_schema(dims))
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        write_meta(
            output_dir,
            &RagMeta {
                embed_provider: embed_provider.to_string(),
                embed_model: embed_model.to_string(),
                dims,
            },
        )?;
        table
    } else {
        db.open_table(TABLE_NAME).execute().await.map_err(|e| e.to_string())?
    };

    Ok((table, rebuilt))
}
