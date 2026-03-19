//! File-based index locking using OS advisory locks.
//!
//! Writers acquire an exclusive lock; concurrent readers use shared locks.
//! SQLite WAL mode already handles concurrent reads safely, so shared locks
//! are mainly a signal to writers that readers are active.

use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result};
use fs2::FileExt;

pub struct IndexLock {
    file: File,
}

impl IndexLock {
    /// Acquire an exclusive write lock. Fails immediately if another process
    /// holds any lock on the index.
    pub fn acquire_exclusive(index_dir: &Path) -> Result<Self> {
        let lock_path = index_dir.join("index.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;
        file.try_lock_exclusive().map_err(|_| {
            anyhow::anyhow!(
                "Index is locked by another process. \
                 Is another `scout index` running?"
            )
        })?;
        Ok(Self { file })
    }

    /// Acquire a shared read lock. Multiple readers can hold this simultaneously.
    #[allow(dead_code)]
    pub fn acquire_shared(index_dir: &Path) -> Result<Self> {
        let lock_path = index_dir.join("index.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;
        file.try_lock_shared().map_err(|_| {
            anyhow::anyhow!("Index is exclusively locked — a write is in progress, please retry.")
        })?;
        Ok(Self { file })
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
