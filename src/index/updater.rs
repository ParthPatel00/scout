/// Incremental single-file reindex logic shared between the daemon and
/// the `update --batch` command.

use std::path::Path;

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::storage::{sqlite, tantivy_store};
use crate::types::{CallEdge, FileRecord, Language};

/// Reindex a single source file relative to `root`. Skips if the file hash
/// hasn't changed. Returns true if the file was actually reindexed.
pub fn reindex_file(
    conn: &rusqlite::Connection,
    writer: &mut tantivy::IndexWriter,
    schema: &tantivy_store::Schema,
    root: &Path,
    rel_path: &str,
) -> Result<bool> {
    let abs_path = root.join(rel_path);

    // File was deleted — remove from index.
    if !abs_path.exists() {
        sqlite::delete_units_for_file(conn, rel_path)?;
        conn.execute("DELETE FROM file_index WHERE file_path = ?1", [rel_path])?;
        return Ok(true);
    }

    let content = match std::fs::read_to_string(&abs_path) {
        Ok(c) => c,
        Err(_) => return Ok(false), // binary or unreadable
    };

    let hash = sha256_hex(&content);

    // Skip if unchanged.
    if let Ok(Some(stored)) = sqlite::get_file_hash(conn, rel_path) {
        if stored == hash {
            return Ok(false);
        }
    }

    let lang = language_for_path(&abs_path);
    let units = crate::index::parser::parse_file(rel_path, &content, &lang)
        .unwrap_or_default();

    sqlite::delete_units_for_file(conn, rel_path)?;
    let mut inserted = Vec::with_capacity(units.len());
    for mut unit in units {
        let id = sqlite::insert_unit(conn, &unit)?;
        unit.id = id;
        inserted.push(unit);
    }

    for unit in &inserted {
        for callee_name in &unit.calls {
            sqlite::insert_call_edge(conn, &CallEdge {
                caller_id: unit.id,
                callee_name: callee_name.clone(),
                line_number: unit.line_start,
            })?;
        }
    }

    tantivy_store::index_file_units(writer, schema, rel_path, &inserted)?;

    sqlite::upsert_file_record(conn, &FileRecord {
        file_path: rel_path.to_string(),
        file_hash: hash,
        last_indexed: chrono::Utc::now().timestamp(),
        needs_reindex: false,
    })?;

    Ok(true)
}

/// Walk all source files under `root`, reindex any whose hash changed.
/// Returns the count of files actually reindexed.
pub fn batch_update(
    conn: &rusqlite::Connection,
    writer: &mut tantivy::IndexWriter,
    schema: &tantivy_store::Schema,
    root: &Path,
) -> Result<usize> {
    let files = crate::index::walker::walk_source_files(root);
    let mut count = 0;
    for abs_path in &files {
        let rel_path = abs_path
            .strip_prefix(root)
            .unwrap_or(abs_path)
            .to_string_lossy()
            .to_string();
        if reindex_file(conn, writer, schema, root, &rel_path)? {
            count += 1;
        }
    }
    Ok(count)
}

fn sha256_hex(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

fn language_for_path(path: &Path) -> Language {
    path.extension()
        .and_then(|e| e.to_str())
        .map(Language::from_extension)
        .unwrap_or(Language::Unknown)
}
