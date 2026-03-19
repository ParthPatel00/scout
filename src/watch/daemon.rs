//! Daemon event loop.
//!
//! Picks the best available watcher strategy (git → native → polling),
//! receives change events, and calls the incremental updater.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::Result;

use crate::index::{self, updater};
use crate::storage::{backup, lock, sqlite, tantivy_store};

use super::WatchEvent;

// ─── Shutdown flag ─────────────────────────────────────────────────────────────
// Set by SIGTERM/SIGINT handler; checked in the event loop.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

// ─── PID cleanup guard ─────────────────────────────────────────────────────────
// Removes the daemon.pid file when this guard is dropped (any exit path —
// normal return, error propagation, or panic).
struct PidCleanup {
    idx_dir: PathBuf,
}

impl Drop for PidCleanup {
    fn drop(&mut self) {
        crate::cli::daemon::remove_pid(&self.idx_dir);
    }
}

// ─── Entry point ───────────────────────────────────────────────────────────────

pub fn run(root: PathBuf) -> Result<()> {
    let idx_dir = index::index_dir(&root)?;

    // Register the PID cleanup guard before anything else.
    // This ensures the daemon.pid file is removed on every exit path —
    // including errors returned from process_events.
    let _cleanup = PidCleanup { idx_dir: idx_dir.clone() };

    // Install signal handlers so SIGTERM/SIGINT set the shutdown flag instead
    // of killing the process instantly (allowing in-flight writes to finish).
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGTERM, handle_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT,  handle_signal as *const () as libc::sighandler_t);
    }

    let (tx, rx) = mpsc::channel::<WatchEvent>();

    let strategy = start_best_watcher(&root, tx)?;
    eprintln!("scout daemon: watching {} [strategy: {}]", root.display(), strategy);

    process_events(&root, rx, &idx_dir)
}

// ─── Watcher selection ─────────────────────────────────────────────────────────

/// Start the best available watcher and return its name.
///
/// Uses a startup-result channel so we wait for each watcher to confirm it
/// initialized successfully rather than relying on a fragile sleep heuristic.
fn start_best_watcher(root: &Path, tx: mpsc::Sender<WatchEvent>) -> Result<&'static str> {
    // Strategy 1: git index watcher — single file, low overhead, works on monorepos.
    {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (root1, tx1) = (root.to_path_buf(), tx.clone());
        std::thread::spawn(move || super::git::watch(&root1, tx1, ready_tx));
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => return Ok("git"),
            Ok(Err(e)) => eprintln!("scout daemon: git watcher unavailable: {e}"),
            Err(_) => eprintln!("scout daemon: git watcher timed out, trying native"),
        }
    }

    // Strategy 2: native OS file events.
    {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (root2, tx2) = (root.to_path_buf(), tx.clone());
        std::thread::spawn(move || super::native::watch(&root2, tx2, ready_tx));
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => return Ok("native"),
            Ok(Err(e)) => eprintln!("scout daemon: native watcher unavailable: {e}"),
            Err(_) => eprintln!("scout daemon: native watcher timed out, using polling"),
        }
    }

    // Strategy 3: polling fallback — always works.
    let root3 = root.to_path_buf();
    std::thread::spawn(move || super::polling::watch(&root3, Duration::from_secs(5), tx));
    Ok("polling")
}

// ─── Event loop ────────────────────────────────────────────────────────────────

fn process_events(root: &Path, rx: Receiver<WatchEvent>, idx_dir: &Path) -> Result<()> {
    let db_path = index::db_path(idx_dir);
    let tantivy_dir = idx_dir.join("tantivy");

    loop {
        // Use recv_timeout so we can check the shutdown flag regularly.
        let first = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(ev) => ev,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if SHUTDOWN.load(Ordering::Relaxed) {
                    eprintln!("scout daemon: shutdown signal received, exiting.");
                    return Ok(());
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // All watcher threads have exited — nothing left to watch.
                eprintln!("scout daemon: all watchers exited, shutting down.");
                return Ok(());
            }
        };

        // Check shutdown after waking on an event too.
        if SHUTDOWN.load(Ordering::Relaxed) {
            eprintln!("scout daemon: shutdown signal received, exiting.");
            return Ok(());
        }

        // Debounce: collect any additional events that arrive within 200 ms.
        let mut events = vec![first];
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(e) => events.push(e),
                Err(_) => break,
            }
        }

        let mut paths: Vec<PathBuf> = Vec::new();
        let mut check_all = false;
        for event in events {
            match event {
                WatchEvent::CheckAll => check_all = true,
                WatchEvent::Files(changes) => {
                    for c in changes { paths.push(c.path); }
                }
            }
        }

        // Acquire write lock — skip this batch if another writer holds it
        // (e.g. `scout index` is running).
        let _lock = match lock::IndexLock::acquire_exclusive(idx_dir) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("daemon: skipping update, could not acquire lock: {e}");
                continue;
            }
        };

        let conn = match sqlite::open(&db_path) {
            Ok(c) => c,
            Err(e) => { eprintln!("daemon: SQLite open failed: {e}"); continue; }
        };
        let (tantivy_index, schema) = match tantivy_store::open_index(&tantivy_dir) {
            Ok(t) => t,
            Err(e) => { eprintln!("daemon: Tantivy open failed: {e}"); continue; }
        };
        let mut writer = match tantivy_index.writer(20_000_000) {
            Ok(w) => w,
            Err(e) => { eprintln!("daemon: Tantivy writer failed: {e}"); continue; }
        };

        let updated = if check_all {
            match updater::batch_update(&conn, &mut writer, &schema, root) {
                Ok(n) => n,
                Err(e) => { eprintln!("daemon: batch update failed: {e}"); continue; }
            }
        } else {
            let mut count = 0;
            for abs_path in &paths {
                let rel = abs_path
                    .strip_prefix(root)
                    .unwrap_or(abs_path)
                    .to_string_lossy()
                    .to_string();
                match updater::reindex_file(&conn, &mut writer, &schema, root, &rel) {
                    Ok(true) => count += 1,
                    Ok(false) => {}
                    Err(e) => eprintln!("daemon: reindex_file failed for {rel}: {e}"),
                }
            }
            count
        };

        if updated > 0 {
            if let Err(e) = writer.commit() {
                eprintln!("daemon: Tantivy commit failed: {e}");
                continue;
            }

            drop(conn);
            match index::load_metadata(idx_dir) {
                Ok(mut meta) => {
                    meta.last_updated = chrono::Utc::now().timestamp();
                    if let Ok(cs) = backup::compute_db_checksum(idx_dir) {
                        meta.checksum = cs;
                    }
                    let _ = index::save_metadata(idx_dir, &meta);
                }
                Err(e) => eprintln!("daemon: metadata update failed: {e}"),
            }

            eprintln!("daemon: reindexed {updated} file(s)");
        }
    }
}
