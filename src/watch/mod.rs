pub mod daemon;
pub mod git;
pub mod native;
pub mod polling;

use std::path::PathBuf;

/// A file-system change event produced by any watcher strategy.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    #[allow(dead_code)]
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Created,
    Deleted,
}

/// Internal event sent from a watcher thread to the daemon event loop.
pub enum WatchEvent {
    /// Specific files that changed (from native OS watcher).
    Files(Vec<FileChange>),
    /// Something changed — scan all files for hash differences (git/polling).
    CheckAll,
}
