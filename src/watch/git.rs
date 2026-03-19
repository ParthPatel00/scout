//! Git-based file watcher — monitors `.git/index` for modifications.
//!
//! A change to `.git/index` means a commit, checkout, or merge occurred.
//! We respond by triggering a full hash-comparison scan (CheckAll) rather
//! than tracking specific paths, which is reliable across all git operations.

use std::path::Path;
use std::sync::mpsc::{Sender, SyncSender};
use std::time::Duration;

use anyhow::Result;
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode, Watcher};

use super::WatchEvent;

/// Watch `.git/index` in `repo_root` and send `WatchEvent::CheckAll` on change.
///
/// Signals startup success/failure via `ready` before entering the blocking
/// loop, so the caller does not need a sleep-based heuristic.
///
/// Returns `Err` if this is not a git repository (caller should fall back).
pub fn watch(repo_root: &Path, tx: Sender<WatchEvent>, ready: SyncSender<Result<()>>) {
    let git_index = repo_root.join(".git").join("index");
    if !git_index.exists() {
        let _ = ready.send(Err(anyhow::anyhow!("not a git repository — .git/index not found")));
        return;
    }

    let tx2 = tx.clone();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            let relevant = matches!(
                event.kind,
                EventKind::Modify(ModifyKind::Data(_))
                    | EventKind::Modify(ModifyKind::Any)
                    | EventKind::Create(_)
            );
            if relevant {
                let _ = tx2.send(WatchEvent::CheckAll);
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            let _ = ready.send(Err(anyhow::anyhow!("notify watcher failed: {e}")));
            return;
        }
    };

    if let Err(e) = watcher.watch(&git_index, RecursiveMode::NonRecursive) {
        let _ = ready.send(Err(anyhow::anyhow!("failed to watch .git/index: {e}")));
        return;
    }

    // Initialization succeeded — signal the caller before blocking.
    let _ = ready.send(Ok(()));

    // Keep watcher alive. Exits only when the daemon process is killed.
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
