//! Hybrid search: BM25 + vector similarity fused via RRF.
//!
//! This is the default search mode. Falls back gracefully to BM25 + name-match
//! when no model or vector store is available — no user action required.

use std::path::Path;

use anyhow::Result;

use crate::ml::EmbeddingModel;
use crate::search::{bm25, rrf, SearchFilter};
use crate::storage::sqlite;
use crate::storage::vectors::VectorStore;
use crate::types::SearchResult;

/// Default search: BM25 + name-match always; adds vector component when
/// the model and vector store are both available. Silent fallback otherwise.
pub fn search(
    tantivy_dir: &Path,
    vector_path: &Path,
    query: &str,
    limit: usize,
    filter: &SearchFilter,
    model: Option<&dyn EmbeddingModel>,
) -> Result<Vec<SearchResult>> {
    let bm25_results = bm25::search(tantivy_dir, query, limit, filter)?;

    let vector_hits = if let Some(model) = model {
        if vector_path.exists() {
            embed_and_search(vector_path, query, limit, model).unwrap_or_default()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    if vector_hits.is_empty() {
        Ok(rrf::fuse(query, bm25_results))
    } else {
        Ok(rrf::fuse_hybrid(query, bm25_results, vector_hits))
    }
}

/// Pure vector search (--semantic flag). Requires model and vector store.
/// Resolves unit IDs back to SearchResults via SQLite.
pub fn search_semantic_only(
    vector_path: &Path,
    query: &str,
    limit: usize,
    filter: &SearchFilter,
    model: &dyn EmbeddingModel,
) -> Result<Vec<SearchResult>> {
    if !vector_path.exists() {
        anyhow::bail!(
            "No vector store found. Run `scout index` after downloading the embedding model."
        );
    }

    // The vector store lives alongside the SQLite db.
    let db_path = vector_path.with_file_name("metadata.db");
    let conn = sqlite::open(&db_path)?;

    let hits = embed_and_search(vector_path, query, limit, model)?;

    let mut results: Vec<SearchResult> = hits
        .into_iter()
        .filter_map(|(unit_id, score)| {
            let unit = sqlite::unit_by_id(&conn, unit_id)?;
            // Apply language / path filters.
            if let Some(ref lang) = filter.lang {
                if unit.language.as_str() != lang.as_str() {
                    return None;
                }
            }
            if let Some(ref prefix) = filter.path_prefix {
                if !unit.file_path.contains(prefix.as_str()) {
                    return None;
                }
            }
            if filter.exclude_tests && is_test_file(&unit.file_path) {
                return None;
            }
            Some(SearchResult {
                score,
                snippet: unit.full_signature.clone().unwrap_or_default(),
                unit,
                repo_name: None,
            })
        })
        .take(limit)
        .collect();

    // Sort descending by score (embed_and_search returns by cosine similarity).
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    Ok(results)
}

fn embed_and_search(
    vector_path: &Path,
    query: &str,
    top_k: usize,
    model: &dyn EmbeddingModel,
) -> Result<Vec<(i64, f32)>> {
    let embeddings = model.embed_batch(&[query])?;
    let query_vec = embeddings
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("model returned no embeddings for query"))?;

    let mut store = VectorStore::load(vector_path)?;
    store.search(&query_vec, top_k)
}

fn is_test_file(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.contains("/test") || p.contains("_test.") || p.contains(".test.") || p.contains("spec.")
}
