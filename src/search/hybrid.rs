/// Hybrid search: BM25 + vector similarity fused via RRF.
///
/// Falls back gracefully to BM25-only when no model or vector store is available.

use std::path::Path;

use anyhow::Result;

use crate::ml::EmbeddingModel;
use crate::search::{bm25, rrf, SearchFilter};
use crate::storage::vectors::VectorStore;
use crate::types::SearchResult;

/// Run hybrid search. If `model` is Some and the vector store exists,
/// combines BM25 + vector results via three-component RRF. Otherwise
/// falls back to BM25 + name-match fusion.
pub fn search(
    tantivy_dir: &Path,
    vector_path: &Path,
    query: &str,
    limit: usize,
    filter: &SearchFilter,
    model: Option<&dyn EmbeddingModel>,
) -> Result<Vec<SearchResult>> {
    let bm25_results = bm25::search(tantivy_dir, query, limit, filter)?;

    // Attempt vector search.
    let vector_hits = if let Some(model) = model {
        if vector_path.exists() {
            match embed_query_and_search(vector_path, query, limit, model) {
                Ok(hits) => hits,
                Err(e) => {
                    eprintln!("vector search failed (falling back to BM25): {e}");
                    vec![]
                }
            }
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

fn embed_query_and_search(
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
