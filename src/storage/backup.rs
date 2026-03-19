//! Index backup and checksum utilities.
//!
//! Before each write batch the DB is snapshotted to `.scout/backup/`.
//! A SHA-256 checksum of the DB is stored in `metadata.json` and validated
//! on every open, enabling early detection of corruption.

use std::path::Path;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::index;
use crate::types::IndexMetadata;

/// Copy `metadata.db` into `.scout/backup/` before a write batch.
/// Safe to call even if no DB exists yet (first index run).
pub fn create_backup(index_dir: &Path) -> Result<()> {
    let db_path = index::db_path(index_dir);
    if !db_path.exists() {
        return Ok(());
    }
    let backup_dir = index_dir.join("backup");
    std::fs::create_dir_all(&backup_dir)
        .context("failed to create backup directory")?;
    let dest = backup_dir.join("metadata.db");
    std::fs::copy(&db_path, &dest)
        .with_context(|| format!("failed to write backup to {}", dest.display()))?;
    Ok(())
}

/// Restore `metadata.db` from the last backup. Used when corruption is detected.
pub fn restore_from_backup(index_dir: &Path) -> Result<()> {
    let backup_path = index_dir.join("backup").join("metadata.db");
    if !backup_path.exists() {
        bail!(
            "No backup found at {}. Run `scout rebuild` to regenerate the index.",
            backup_path.display()
        );
    }
    let db_path = index::db_path(index_dir);
    std::fs::copy(&backup_path, &db_path)
        .context("failed to restore backup")?;
    eprintln!("warning: restored index from backup — some recent changes may be lost");
    Ok(())
}

/// Compute a SHA-256 hex digest of `metadata.db`.
pub fn compute_db_checksum(index_dir: &Path) -> Result<String> {
    let db_path = index::db_path(index_dir);
    if !db_path.exists() {
        return Ok(String::new());
    }
    let bytes = std::fs::read(&db_path)
        .with_context(|| format!("failed to read {} for checksum", db_path.display()))?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}

/// Verify the stored checksum matches the current DB. On mismatch, attempt
/// backup restoration and return an error prompting the user.
pub fn validate_checksum(index_dir: &Path, meta: &IndexMetadata) -> Result<()> {
    if meta.checksum.is_empty() {
        // No checksum recorded yet (e.g. index just created). Skip.
        return Ok(());
    }
    let actual = compute_db_checksum(index_dir)?;
    if actual != meta.checksum {
        // Try to restore from backup automatically.
        if restore_from_backup(index_dir).is_ok() {
            bail!(
                "Index checksum mismatch — database was corrupted and has been restored from backup.\n\
                 Re-run your command. If this keeps happening, run `scout rebuild`."
            );
        }
        bail!(
            "Index checksum mismatch and no backup is available.\n\
             Run `scout rebuild` to regenerate the index from scratch."
        );
    }
    Ok(())
}
