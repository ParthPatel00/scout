pub mod embeddings;
pub mod model;

use anyhow::Result;

/// Common interface for all embedding backends (local Candle model, cloud APIs).
pub trait EmbeddingModel: Send + Sync {
    /// Embed a batch of text snippets, returning one f32 vector per input.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    /// Output dimension (e.g. 768 for UniXcoder).
    fn dimension(&self) -> usize;
    /// Human-readable model identifier (stored in `code_units.embedding_model`).
    fn model_name(&self) -> &str;
}
