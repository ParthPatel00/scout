//! Native OS file watcher using the `notify` crate.
//!
//! Watches source directories for individual file changes and emits
//! `WatchEvent::Files` with the specific paths that changed. Falls back
//! gracefully if the OS inotify/kqueue limit is exceeded.

use std::path::Path;
use std::sync::mpsc::Sender;

use anyhow::Result;
use notify::event::{CreateKind, ModifyKind, RemoveKind};
use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::index::walker;
use super::{ChangeKind, FileChange, WatchEvent};

/// Watch all source files under `root` using OS-native file events.
/// Applies the same exclusion filters as the indexer.
pub fn watch(root: &Path, tx: Sender<WatchEvent>) -> Result<()> {
    let root = root.to_path_buf();
    let excluded = walker::excluded_dirs();

    let tx2 = tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(event) = res else { return };

        let kind = match event.kind {
            EventKind::Create(CreateKind::File) => ChangeKind::Created,
            EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Any) => {
                ChangeKind::Modified
            }
            EventKind::Remove(RemoveKind::File) => ChangeKind::Deleted,
            _ => return,
        };

        let changes: Vec<FileChange> = event
            .paths
            .into_iter()
            .filter(|p| {
                // Apply the same exclusion logic as the walker.
                !p.components().any(|c| {
                    c.as_os_str()
                        .to_str()
                        .map(|s| excluded.contains(s))
                        .unwrap_or(false)
                }) && walker::is_supported_extension(p)
            })
            .map(|path| FileChange { path, kind: kind.clone() })
            .collect();

        if !changes.is_empty() {
            let _ = tx2.send(WatchEvent::Files(changes));
        }
    })?;

    watcher.watch(&root, RecursiveMode::Recursive)
        .map_err(|e| anyhow::anyhow!("native watcher failed: {e} — try polling fallback"))?;

    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
