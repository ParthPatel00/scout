//! Cross-repo federated search and similarity detection.
//!
//! Runs independent BM25 (+ optional vector) searches across every registered
//! repo and merges the results into a single ranked list.  Results are tagged
//! with `repo_name` so the caller can display provenance.

use std::path::Path;

use anyhow::Result;

use crate::index;
use crate::ml::EmbeddingModel;
use crate::repo::registry::{Registry, RepoEntry};
use crate::search::{bm25, hybrid, rrf, SearchFilter};
use crate::storage::{sqlite, vectors::VectorStore};
use crate::types::SearchResult;

// ─── Cross-repo search ────────────────────────────────────────────────────────

/// Run search across every repo in `entries`. Returns all hits merged and
/// sorted by descending score, deduplicated by (name, body fingerprint).
pub fn search_repos(
    entries: &[&RepoEntry],
    query: &str,
    limit: usize,
    filter: &SearchFilter,
    model: Option<&dyn EmbeddingModel>,
) -> Result<Vec<(String, SearchResult)>> {
    let per_repo = limit.max(5);
    let mut all: Vec<(String, SearchResult)> = Vec::new();

    for entry in entries {
        let root = &entry.path;
        let idx_dir = match index::index_dir(root) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let tantivy_dir = idx_dir.join("tantivy");
        if !tantivy_dir.join("meta.json").exists() {
            continue; // repo not indexed
        }
        let vector_path = idx_dir.join("vectors.bin");

        let results = if model.is_some() {
            hybrid::search(&tantivy_dir, &vector_path, query, per_repo, filter, model)
                .unwrap_or_default()
        } else {
            let bm25_results = bm25::search(&tantivy_dir, query, per_repo, filter)
                .unwrap_or_default();
            rrf::fuse(query, bm25_results)
        };

        for r in results {
            all.push((entry.name.clone(), r));
        }
    }

    // Sort all results by descending score.
    all.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));

    // Deduplicate: drop lower-scoring copies of the same (name, body prefix).
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let deduped: Vec<(String, SearchResult)> = all
        .into_iter()
        .filter(|(_, r)| {
            let fp = body_fingerprint(&r.unit.name, &r.unit.body);
            seen.insert(fp)
        })
        .take(limit)
        .collect();

    Ok(deduped)
}

// ─── Find similar ─────────────────────────────────────────────────────────────

/// Find code units across all registered repos that are structurally similar
/// to the unit at `file_path:line` in `source_root`.
pub fn find_similar(
    registry: &Registry,
    source_root: &Path,
    file_path: &str,
    line: usize,
    limit: usize,
) -> Result<Vec<(String, SearchResult)>> {
    // Load the source unit from SQLite.
    let source_idx = index::index_dir(source_root)?;
    let db_path = index::db_path(&source_idx);
    let conn = sqlite::open(&db_path)?;

    let unit = sqlite::unit_at_line(&conn, file_path, line)
        .ok_or_else(|| anyhow::anyhow!("no code unit found at {file_path}:{line}"))?;

    // Load its embedding from the vector store.
    let vector_path = source_idx.join("vectors.bin");
    if !vector_path.exists() {
        anyhow::bail!(
            "No vector store found for this repo. \
             Run `scout index --download-model` and generate embeddings first."
        );
    }
    let mut store = VectorStore::load(&vector_path)?;
    let query_vec = store
        .get_vector(unit.id)?
        .ok_or_else(|| anyhow::anyhow!("unit {}/{} has no embedding", file_path, unit.name))?;

    // Search every registered repo's vector store.
    let mut all: Vec<(String, SearchResult)> = Vec::new();
    let per_repo = limit.max(5);

    for entry in &registry.repos {
        let idx_dir = match index::index_dir(&entry.path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let vpath = idx_dir.join("vectors.bin");
        if !vpath.exists() {
            continue;
        }
        let mut repo_store = match VectorStore::load(&vpath) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let hits = match repo_store.search(&query_vec, per_repo) {
            Ok(h) => h,
            Err(_) => continue,
        };

        if hits.is_empty() {
            continue;
        }

        // Resolve unit IDs to SearchResults via SQLite.
        let repo_db = index::db_path(&idx_dir);
        let repo_conn = match sqlite::open(&repo_db) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (id, score) in hits {
            if let Some(repo_unit) = sqlite::unit_by_id(&repo_conn, id) {
                let result = SearchResult {
                    score,
                    snippet: String::new(),
                    repo_name: Some(entry.name.clone()),
                    unit: repo_unit,
                };
                all.push((entry.name.clone(), result));
            }
        }
    }

    all.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));

    // Exclude the source unit itself.
    let source_name = unit.name.clone();
    let source_file = unit.file_path.clone();
    all.retain(|(_, r)| r.unit.name != source_name || r.unit.file_path != source_file);

    // Deduplicate by body fingerprint across repos.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let deduped = all
        .into_iter()
        .filter(|(_, r)| seen.insert(body_fingerprint(&r.unit.name, &r.unit.body)))
        .take(limit)
        .collect();

    Ok(deduped)
}

// ─── Embedding deduplication ──────────────────────────────────────────────────

/// Before calling the ML model, check whether any registered repo already has
/// an embedding for a unit with the same body hash.  Returns the embedding
/// vector if found.
#[allow(dead_code)]
pub fn find_cached_embedding(
    body_hash: &str,
    registry: &Registry,
) -> Option<Vec<f32>> {
    for entry in &registry.repos {
        let idx_dir = index::index_dir(&entry.path).ok()?;
        let db_path = index::db_path(&idx_dir);
        let conn = sqlite::open(&db_path).ok()?;

        // Find any unit with this body hash that already has an embedding.
        let unit_id: Option<i64> = conn
            .query_row(
                "SELECT cu.id FROM code_units cu
                 JOIN file_index fi ON cu.file_path = fi.file_path
                 WHERE fi.file_hash = ?1 AND cu.has_embedding = 1
                 LIMIT 1",
                [body_hash],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = unit_id {
            let vpath = idx_dir.join("vectors.bin");
            if vpath.exists() {
                if let Ok(mut store) = VectorStore::load(&vpath) {
                    if let Ok(Some(vec)) = store.get_vector(id) {
                        return Some(vec);
                    }
                }
            }
        }
    }
    None
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// A deduplication key: name + first 80 bytes of body.
fn body_fingerprint(name: &str, body: &str) -> String {
    let n = body.len().min(80);
    format!("{name}\0{}", &body[..n])
}
