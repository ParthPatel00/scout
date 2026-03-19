/// Reciprocal Rank Fusion — combines multiple ranked lists.
///
/// Phase 3: BM25 + name-match (K_BM25=60, K_NAME=5)
/// Phase 7: adds vector-similarity component (K_VEC=60)

use std::collections::HashMap;

use strsim::jaro_winkler;

use crate::types::SearchResult;

/// Standard RRF constant for the BM25 and vector components.
const K_BM25: f32 = 60.0;
/// Smaller K for name-match so exact/prefix matches strongly outweigh position.
const K_NAME: f32 = 5.0;
/// RRF constant for the vector-similarity component.
const K_VEC: f32 = 60.0;

/// Fuse `bm25_results` (already ranked) with name-match re-ranking and return
/// a new ranked list. Results not in either list are dropped.
pub fn fuse(query: &str, bm25_results: Vec<SearchResult>) -> Vec<SearchResult> {
    if bm25_results.is_empty() {
        return bm25_results;
    }

    // Build name-match rank list — score each result by how well its name
    // matches the query, then sort descending.
    let mut name_scores: Vec<(usize, f32)> = bm25_results
        .iter()
        .enumerate()
        .map(|(i, r)| (i, name_match_score(query, &r.unit.name)))
        .collect();
    name_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Assign name-match ranks (0-indexed position in sorted list).
    let mut name_rank = vec![0usize; bm25_results.len()];
    for (rank, (idx, _)) in name_scores.iter().enumerate() {
        name_rank[*idx] = rank;
    }

    // Compute RRF score for each result.
    // BM25 rank = original position; name rank = position in name_scores list.
    // Name component uses a smaller K so exact matches dominate over BM25 rank.
    let mut scored: Vec<(usize, f32)> = bm25_results
        .iter()
        .enumerate()
        .map(|(bm25_rank, _)| {
            let nm_rank = name_rank[bm25_rank];
            let rrf = 1.0 / (K_BM25 + bm25_rank as f32 + 1.0)
                + 1.0 / (K_NAME + nm_rank as f32 + 1.0);
            (bm25_rank, rrf)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Reconstruct result list in fused order, updating scores.
    let mut results = bm25_results;
    let reordered: Vec<SearchResult> = scored
        .into_iter()
        .map(|(orig_idx, rrf_score)| {
            let mut r = results[orig_idx].clone();
            r.score = rrf_score * 1000.0; // scale for display clarity
            r
        })
        .collect();

    // Zero out old results to avoid using moved values.
    drop(results);
    reordered
}

/// Three-component RRF: BM25 + name-match + vector similarity.
///
/// `vector_hits` is a list of (unit_id, cosine_score) from the vector store,
/// pre-sorted by descending score.  Results absent from the vector list get
/// the worst possible vector rank (= len(bm25_results)).
pub fn fuse_hybrid(
    query: &str,
    bm25_results: Vec<SearchResult>,
    vector_hits: Vec<(i64, f32)>,
) -> Vec<SearchResult> {
    if bm25_results.is_empty() {
        return bm25_results;
    }

    // Build id → vector rank mapping.
    let vec_rank_map: HashMap<i64, usize> = vector_hits
        .iter()
        .enumerate()
        .map(|(rank, (id, _))| (*id, rank))
        .collect();
    let default_vec_rank = bm25_results.len(); // worst rank for missing entries

    // Name-match ranking (same logic as fuse()).
    let mut name_scores: Vec<(usize, f32)> = bm25_results
        .iter()
        .enumerate()
        .map(|(i, r)| (i, name_match_score(query, &r.unit.name)))
        .collect();
    name_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut name_rank = vec![0usize; bm25_results.len()];
    for (rank, (idx, _)) in name_scores.iter().enumerate() {
        name_rank[*idx] = rank;
    }

    let mut scored: Vec<(usize, f32)> = bm25_results
        .iter()
        .enumerate()
        .map(|(bm25_rank, r)| {
            let nm_rank = name_rank[bm25_rank];
            let vec_rank = *vec_rank_map.get(&r.unit.id).unwrap_or(&default_vec_rank);
            let rrf = 1.0 / (K_BM25 + bm25_rank as f32 + 1.0)
                + 1.0 / (K_NAME + nm_rank as f32 + 1.0)
                + 1.0 / (K_VEC + vec_rank as f32 + 1.0);
            (bm25_rank, rrf)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut results = bm25_results;
    let reordered: Vec<SearchResult> = scored
        .into_iter()
        .map(|(orig_idx, rrf_score)| {
            let mut r = results[orig_idx].clone();
            r.score = rrf_score * 1000.0;
            r
        })
        .collect();
    drop(results);
    reordered
}

/// Score how well `query` matches `name`.
/// Returns a value in [0, 1].
fn name_match_score(query: &str, name: &str) -> f32 {
    let q = query.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();

    // Exact match
    if n == q {
        return 1.0;
    }
    // Name starts with query
    if n.starts_with(&q) {
        return 0.9;
    }
    // Name contains query as substring
    if n.contains(&q) {
        return 0.7;
    }
    // Query token is contained in name (for multi-word queries)
    let query_tokens: Vec<&str> = q.split_whitespace().collect();
    let token_match = query_tokens.iter().any(|t| n.contains(*t));
    if token_match {
        return 0.5;
    }
    // Fuzzy fallback via Jaro-Winkler on the first query token vs name
    let first_token = query_tokens.first().copied().unwrap_or(&q);
    jaro_winkler(first_token, &n) as f32 * 0.4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(name: &str, score: f32) -> SearchResult {
        use crate::types::{CodeUnit, Language, UnitType};
        SearchResult {
            unit: CodeUnit::new("file.py", Language::Python, UnitType::Function, name, 1, 10, ""),
            score,
            snippet: String::new(),
            repo_name: None,
        }
    }

    #[test]
    fn exact_name_match_ranks_higher() {
        let results = vec![
            make_result("process_payment", 10.0),
            make_result("refund", 8.0),
            make_result("charge_customer", 6.0),
        ];
        let fused = fuse("charge_customer", results);
        assert_eq!(fused[0].unit.name, "charge_customer");
    }

    #[test]
    fn fuse_preserves_all_results() {
        let results = vec![
            make_result("foo", 5.0),
            make_result("bar", 4.0),
            make_result("baz", 3.0),
        ];
        let fused = fuse("foo", results);
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn empty_results_passthrough() {
        let fused = fuse("query", vec![]);
        assert!(fused.is_empty());
    }
}
