/// Daemon event loop.
///
/// Picks the best available watcher strategy (git → native → polling),
/// receives change events, and calls the incremental updater.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::Result;

use crate::index::{self, updater};
use crate::storage::{backup, lock, sqlite, tantivy_store};

use super::WatchEvent;

/// Run the daemon event loop for `root`. This blocks indefinitely.
pub fn run(root: PathBuf) -> Result<()> {
    let (tx, rx) = mpsc::channel::<WatchEvent>();

    let strategy = start_best_watcher(&root, tx);
    eprintln!("codesearch daemon: watching {} [strategy: {}]", root.display(), strategy);

    process_events(&root, rx)
}

/// Spawn the best available watcher in a background thread.
/// Returns a string describing which strategy was chosen.
fn start_best_watcher(root: &Path, tx: mpsc::Sender<WatchEvent>) -> &'static str {
    // Strategy 1: git index watcher
    let root1 = root.to_path_buf();
    let tx1 = tx.clone();
    let git_result = std::thread::spawn(move || {
        super::git::watch(&root1, tx1)
    });

    // Give git watcher a moment to fail if not a git repo.
    std::thread::sleep(Duration::from_millis(50));
    if !git_result.is_finished() {
        return "git";
    }

    // Strategy 2: native OS watcher
    let root2 = root.to_path_buf();
    let tx2 = tx.clone();
    let native_result = std::thread::spawn(move || {
        super::native::watch(&root2, tx2)
    });

    std::thread::sleep(Duration::from_millis(50));
    if !native_result.is_finished() {
        return "native";
    }

    // Strategy 3: polling fallback (5-second interval)
    let root3 = root.to_path_buf();
    let tx3 = tx;
    std::thread::spawn(move || {
        super::polling::watch(&root3, Duration::from_secs(5), tx3);
    });

    "polling"
}

/// Process watch events and call incremental reindex for changed files.
fn process_events(root: &Path, rx: Receiver<WatchEvent>) -> Result<()> {
    let idx_dir = index::index_dir(root)?;
    let db_path = index::db_path(&idx_dir);
    let tantivy_dir = idx_dir.join("tantivy");

    loop {
        // Block until an event arrives, then drain any extras that queued up
        // (debounce rapid-fire saves by collecting for up to 200 ms).
        let first = match rx.recv() {
            Ok(e) => e,
            Err(_) => break, // channel closed
        };
        let mut events = vec![first];
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(e) => events.push(e),
                Err(_) => break,
            }
        }

        // Collect all specific file paths; any CheckAll collapses to a full scan.
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut check_all = false;
        for event in events {
            match event {
                WatchEvent::CheckAll => check_all = true,
                WatchEvent::Files(changes) => {
                    for c in changes {
                        paths.push(c.path);
                    }
                }
            }
        }

        // Acquire exclusive lock, open DB and Tantivy writer.
        let _lock = match lock::IndexLock::acquire_exclusive(&idx_dir) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("daemon: skipping update, could not acquire lock: {e}");
                continue;
            }
        };
        let conn = sqlite::open(&db_path)?;
        let (tantivy_index, schema) = tantivy_store::open_index(&tantivy_dir)?;
        let mut writer = tantivy_index.writer(20_000_000)?;

        let updated = if check_all {
            updater::batch_update(&conn, &mut writer, &schema, root)?
        } else {
            let mut count = 0;
            for abs_path in &paths {
                let rel = abs_path
                    .strip_prefix(root)
                    .unwrap_or(abs_path)
                    .to_string_lossy()
                    .to_string();
                if updater::reindex_file(&conn, &mut writer, &schema, root, &rel)? {
                    count += 1;
                }
            }
            count
        };

        if updated > 0 {
            writer.commit()?;

            // Update metadata checksum.
            drop(conn);
            let mut meta = index::load_metadata(&idx_dir)?;
            meta.last_updated = chrono::Utc::now().timestamp();
            meta.checksum = backup::compute_db_checksum(&idx_dir)?;
            index::save_metadata(&idx_dir, &meta)?;

            eprintln!("daemon: reindexed {updated} file(s)");
        }
    }

    Ok(())
}
