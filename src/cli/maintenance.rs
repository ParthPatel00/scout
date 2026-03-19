use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::index;
use crate::storage::{backup, lock, sqlite};

// ─── Rebuild ──────────────────────────────────────────────────────────────────

pub struct RebuildArgs {
    pub path: PathBuf,
    pub verbose: bool,
}

/// Delete the entire index and regenerate it from scratch.
pub fn rebuild(args: RebuildArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;

    // Acquire exclusive lock before wiping.
    let _lock = lock::IndexLock::acquire_exclusive(&idx_dir)?;

    eprintln!("Removing existing index at {} ...", idx_dir.display());
    std::fs::remove_dir_all(&idx_dir)
        .context("failed to remove index directory")?;

    // Re-run full indexing.
    drop(_lock); // release lock so index command can re-acquire
    super::index::run(super::index::IndexArgs {
        path: args.path,
        verbose: args.verbose,
    })
}

// ─── Optimize ─────────────────────────────────────────────────────────────────

pub struct OptimizeArgs {
    pub path: PathBuf,
}

/// Compact the SQLite database, update query planner statistics, and remove
/// orphaned call graph edges.
pub fn optimize(args: OptimizeArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let db_path = index::db_path(&idx_dir);

    if !db_path.exists() {
        bail!(
            "No index found at {}. Run `codesearch index` first.",
            root.display()
        );
    }

    let _lock = lock::IndexLock::acquire_exclusive(&idx_dir)?;
    let conn = sqlite::open(&db_path)?;

    // Remove call graph edges whose caller no longer exists (stale after deletions).
    let orphaned_edges: usize = conn.execute(
        "DELETE FROM call_graph WHERE caller_id NOT IN (SELECT id FROM code_units)",
        [],
    )?;

    // SQLite VACUUM reclaims free pages; ANALYZE refreshes query statistics.
    conn.execute_batch("PRAGMA optimize; VACUUM; ANALYZE;")
        .context("failed to run VACUUM/ANALYZE")?;

    let page_count: i64 =
        conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let page_size: i64 =
        conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let size_kb = page_count * page_size / 1024;

    drop(conn);

    // Update checksum after VACUUM (page layout changes).
    let mut meta = index::load_metadata(&idx_dir)?;
    meta.checksum = backup::compute_db_checksum(&idx_dir)?;
    index::save_metadata(&idx_dir, &meta)?;

    println!(
        "Optimized: removed {orphaned_edges} orphaned call edges, \
         database is now {size_kb} KB"
    );
    Ok(())
}

// ─── Cleanup ──────────────────────────────────────────────────────────────────

pub struct CleanupArgs {
    pub path: PathBuf,
}

/// Remove code units for files that no longer exist on disk, and clean up
/// their call graph edges.
pub fn cleanup(args: CleanupArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let db_path = index::db_path(&idx_dir);

    if !db_path.exists() {
        bail!(
            "No index found at {}. Run `codesearch index` first.",
            root.display()
        );
    }

    let _lock = lock::IndexLock::acquire_exclusive(&idx_dir)?;
    let conn = sqlite::open(&db_path)?;

    // Find all indexed file paths. Use an explicit loop so stmt drops before conn.
    let paths: Vec<String> = {
        let mut stmt = conn.prepare("SELECT file_path FROM file_index")?;
        let mut rows = stmt.query([])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            result.push(row.get::<_, String>(0)?);
        }
        result
    };

    let mut removed_files = 0usize;
    let mut removed_units = 0usize;

    for rel_path in paths {
        let abs_path = root.join(&rel_path);
        if !abs_path.exists() {
            // File deleted from disk — remove from index.
            conn.execute("DELETE FROM file_index WHERE file_path = ?1", [&rel_path])?;
            let n = conn.execute(
                "DELETE FROM code_units WHERE file_path = ?1",
                [&rel_path],
            )?;
            removed_units += n;
            removed_files += 1;
        }
    }

    drop(conn);

    if removed_files > 0 {
        let mut meta = index::load_metadata(&idx_dir)?;
        meta.checksum = backup::compute_db_checksum(&idx_dir)?;
        meta.num_files = meta.num_files.saturating_sub(removed_files);
        meta.num_units = meta.num_units.saturating_sub(removed_units);
        index::save_metadata(&idx_dir, &meta)?;
    }

    println!(
        "Cleanup: removed {removed_files} deleted files ({removed_units} units) from the index"
    );
    Ok(())
}
