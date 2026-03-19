/// Git-based file watcher — monitors `.git/index` for modifications.
///
/// A change to `.git/index` means a commit, checkout, or merge occurred.
/// We respond by triggering a full hash-comparison scan (CheckAll) rather
/// than tracking specific paths, which is reliable across all git operations.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::Result;
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode, Watcher};

use super::WatchEvent;

/// Watch `.git/index` in `repo_root` and send `WatchEvent::CheckAll` on change.
/// Returns `Err` if this is not a git repository (caller should fall back).
pub fn watch(repo_root: &Path, tx: Sender<WatchEvent>) -> Result<()> {
    let git_index = repo_root.join(".git").join("index");
    if !git_index.exists() {
        anyhow::bail!("not a git repository — .git/index not found");
    }

    let tx2 = tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
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
    })?;

    watcher.watch(&git_index, RecursiveMode::NonRecursive)?;

    // Keep the watcher alive. The thread will be killed when the daemon exits.
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
