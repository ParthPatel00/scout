/// Schema version migration system.
///
/// `IndexMetadata.version` tracks the index format. On open, this module
/// compares the stored version against `CURRENT_VERSION`:
///   - Patch-compatible (same major): run incremental migrations silently.
///   - Newer than binary: hard error — upgrade codesearch.
///   - Major version too old: hard error — requires `codesearch rebuild`.

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::types::IndexMetadata;

/// The index schema version produced by this binary.
pub const CURRENT_VERSION: u32 = 1;

/// Check version compatibility and run any needed migrations.
/// Updates `meta.version` in place; caller must persist metadata.
pub fn run_migrations(conn: &Connection, meta: &mut IndexMetadata) -> Result<()> {
    if meta.version == CURRENT_VERSION {
        return Ok(());
    }

    if meta.version > CURRENT_VERSION {
        bail!(
            "Index was created by a newer version of codesearch \
             (index format v{}, this binary supports up to v{}).\n\
             Please upgrade codesearch.",
            meta.version,
            CURRENT_VERSION
        );
    }

    // Run incremental migrations from meta.version up to CURRENT_VERSION.
    let from = meta.version;
    migrate(conn, from, CURRENT_VERSION)?;
    meta.version = CURRENT_VERSION;
    eprintln!(
        "info: migrated index from v{from} to v{CURRENT_VERSION}"
    );
    Ok(())
}

/// Apply migrations from `from_version` up to (but not including) `to_version`.
/// Add new `migrate_vN_to_vM` functions here as the schema evolves.
fn migrate(conn: &Connection, from_version: u32, to_version: u32) -> Result<()> {
    let mut v = from_version;
    while v < to_version {
        match v {
            // Example future migration:
            // 1 => migrate_v1_to_v2(conn)?,
            _ => {
                bail!(
                    "No migration path from v{v} to v{}. Run `codesearch rebuild`.",
                    v + 1
                );
            }
        }
        v += 1;
    }
    let _ = conn; // suppress unused warning until real migrations exist
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IndexMetadata;

    #[test]
    fn same_version_is_noop() {
        let conn = Connection::open_in_memory().unwrap();
        let mut meta = IndexMetadata::new();
        meta.version = CURRENT_VERSION;
        assert!(run_migrations(&conn, &mut meta).is_ok());
        assert_eq!(meta.version, CURRENT_VERSION);
    }

    #[test]
    fn newer_index_is_error() {
        let conn = Connection::open_in_memory().unwrap();
        let mut meta = IndexMetadata::new();
        meta.version = CURRENT_VERSION + 1;
        assert!(run_migrations(&conn, &mut meta).is_err());
    }
}
