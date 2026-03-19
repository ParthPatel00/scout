pub mod parser;
pub mod updater;
pub mod walker;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::types::IndexMetadata;

/// Returns the `.scout/` directory for a given repo root, creating it if needed.
pub fn index_dir(repo_root: &Path) -> Result<PathBuf> {
    let dir = repo_root.join(".scout");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create index dir {}", dir.display()))?;
    Ok(dir)
}

/// Path to the SQLite database file.
pub fn db_path(index_dir: &Path) -> PathBuf {
    index_dir.join("metadata.db")
}

/// Path to the metadata JSON file.
pub fn metadata_path(index_dir: &Path) -> PathBuf {
    index_dir.join("metadata.json")
}

/// Load metadata from disk, or return a fresh default if the file doesn't exist.
pub fn load_metadata(index_dir: &Path) -> Result<IndexMetadata> {
    let path = metadata_path(index_dir);
    if !path.exists() {
        return Ok(IndexMetadata::new());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&contents).context("failed to parse metadata.json")
}

/// Persist metadata to disk atomically (write to temp file then rename).
pub fn save_metadata(index_dir: &Path, meta: &IndexMetadata) -> Result<()> {
    let path = metadata_path(index_dir);
    let tmp = path.with_extension("json.tmp");
    let contents = serde_json::to_string_pretty(meta)?;
    std::fs::write(&tmp, contents)
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to rename metadata tmp file"))?;
    Ok(())
}
