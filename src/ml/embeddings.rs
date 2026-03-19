/// Lazy embedding generation pipeline.
///
/// Fetches all code units that lack an embedding, processes them in batches of
/// BATCH_SIZE using the provided model, writes vectors to the VectorStore, and
/// marks each unit as embedded in SQLite.

use std::path::Path;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};

use crate::storage::vectors::VectorStore;
use super::EmbeddingModel;

/// Number of texts embedded per model call.
const BATCH_SIZE: usize = 64;

/// Generate embeddings for every code unit that has `has_embedding = 0`.
/// Returns the number of units newly embedded.
pub fn generate_embeddings(
    conn: &rusqlite::Connection,
    vector_path: &Path,
    model: &dyn EmbeddingModel,
) -> Result<usize> {
    // Collect pending units: (id, name, body).
    let pending: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name, body FROM code_units WHERE has_embedding = 0 ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    if pending.is_empty() {
        return Ok(0);
    }

    // Open or create the vector store.
    let mut store = if vector_path.exists() {
        VectorStore::load(vector_path)?
    } else {
        VectorStore::new(vector_path, model.dimension())
    };

    let pb = ProgressBar::new(pending.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
                 {pos}/{len} embeddings (ETA {eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );

    let model_name = model.model_name().to_string();
    let mut total = 0usize;

    for chunk in pending.chunks(BATCH_SIZE) {
        // Build text inputs: "name: <name>\n<body>" for each unit.
        let texts: Vec<String> = chunk
            .iter()
            .map(|(_, name, body)| format!("name: {name}\n{body}"))
            .collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

        let embeddings = model.embed_batch(&text_refs)?;

        for ((id, _, _), embedding) in chunk.iter().zip(embeddings.iter()) {
            store.insert(*id, embedding)?;
        }

        // Mark units as embedded in SQLite.
        let ids: Vec<i64> = chunk.iter().map(|(id, _, _)| *id).collect();
        mark_embedded(conn, &ids, &model_name)?;

        total += chunk.len();
        pb.inc(chunk.len() as u64);
    }

    pb.finish_and_clear();
    store.flush()?;
    println!("Generated {total} embedding(s) → {}", vector_path.display());
    Ok(total)
}

fn mark_embedded(conn: &rusqlite::Connection, ids: &[i64], model_name: &str) -> Result<()> {
    for id in ids {
        conn.execute(
            "UPDATE code_units SET has_embedding = 1, embedding_model = ?1 WHERE id = ?2",
            rusqlite::params![model_name, id],
        )?;
    }
    Ok(())
}
