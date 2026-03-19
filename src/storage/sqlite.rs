use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::types::{CallEdge, CodeUnit, FileRecord, Language, UnitType};

/// Open (or create) the SQLite database at the given path with WAL mode enabled.
pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open SQLite at {}", path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;",
    )?;
    Ok(conn)
}

/// Create all tables and indexes if they do not already exist.
pub fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS code_units (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path       TEXT NOT NULL,
            language        TEXT NOT NULL,
            unit_type       TEXT NOT NULL,
            name            TEXT NOT NULL,
            full_signature  TEXT,
            docstring       TEXT,
            line_start      INTEGER NOT NULL,
            line_end        INTEGER NOT NULL,
            body            TEXT NOT NULL,
            parameters      TEXT NOT NULL DEFAULT '[]',
            return_type     TEXT,
            calls           TEXT NOT NULL DEFAULT '[]',
            imports         TEXT NOT NULL DEFAULT '[]',
            complexity      INTEGER NOT NULL DEFAULT 1,
            has_embedding   INTEGER NOT NULL DEFAULT 0,
            embedding_model TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_cu_name      ON code_units(name);
        CREATE INDEX IF NOT EXISTS idx_cu_file_path ON code_units(file_path);
        CREATE INDEX IF NOT EXISTS idx_cu_language  ON code_units(language);

        CREATE TABLE IF NOT EXISTS call_graph (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            caller_id   INTEGER NOT NULL,
            callee_name TEXT NOT NULL,
            line_number INTEGER NOT NULL,
            FOREIGN KEY(caller_id) REFERENCES code_units(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_cg_caller ON call_graph(caller_id);

        CREATE TABLE IF NOT EXISTS file_index (
            file_path       TEXT PRIMARY KEY,
            file_hash       TEXT NOT NULL,
            last_indexed    INTEGER NOT NULL,
            needs_reindex   INTEGER NOT NULL DEFAULT 0
        );",
    )
    .context("failed to initialize schema")
}

/// Insert or replace a `FileRecord` in `file_index`.
pub fn upsert_file_record(conn: &Connection, record: &FileRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO file_index (file_path, file_hash, last_indexed, needs_reindex)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(file_path) DO UPDATE SET
           file_hash     = excluded.file_hash,
           last_indexed  = excluded.last_indexed,
           needs_reindex = excluded.needs_reindex",
        params![
            record.file_path,
            record.file_hash,
            record.last_indexed,
            record.needs_reindex as i32,
        ],
    )?;
    Ok(())
}

/// Return the stored hash for a file, or `None` if not yet indexed.
pub fn get_file_hash(conn: &Connection, file_path: &str) -> Result<Option<String>> {
    let mut stmt =
        conn.prepare("SELECT file_hash FROM file_index WHERE file_path = ?1")?;
    let mut rows = stmt.query(params![file_path])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

/// Delete all code units (and their call graph edges) for a given file.
pub fn delete_units_for_file(conn: &Connection, file_path: &str) -> Result<()> {
    // Call graph edges are deleted via ON DELETE CASCADE.
    conn.execute(
        "DELETE FROM code_units WHERE file_path = ?1",
        params![file_path],
    )?;
    Ok(())
}

/// Insert a `CodeUnit` and return its assigned row ID.
pub fn insert_unit(conn: &Connection, unit: &CodeUnit) -> Result<i64> {
    conn.execute(
        "INSERT INTO code_units
            (file_path, language, unit_type, name, full_signature, docstring,
             line_start, line_end, body, parameters, return_type,
             calls, imports, complexity, has_embedding, embedding_model)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            unit.file_path,
            unit.language.as_str(),
            unit.unit_type.to_string(),
            unit.name,
            unit.full_signature,
            unit.docstring,
            unit.line_start as i64,
            unit.line_end as i64,
            unit.body,
            serde_json::to_string(&unit.parameters)?,
            unit.return_type,
            serde_json::to_string(&unit.calls)?,
            serde_json::to_string(&unit.imports)?,
            unit.complexity as i64,
            unit.has_embedding as i32,
            unit.embedding_model,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Insert a call graph edge.
pub fn insert_call_edge(conn: &Connection, edge: &CallEdge) -> Result<()> {
    conn.execute(
        "INSERT INTO call_graph (caller_id, callee_name, line_number)
         VALUES (?1, ?2, ?3)",
        params![edge.caller_id, edge.callee_name, edge.line_number as i64],
    )?;
    Ok(())
}

/// Convert a language string stored in SQLite back to the enum.
fn lang_from_str(s: &str) -> Language {
    match s {
        "python" => Language::Python,
        "rust" => Language::Rust,
        "typescript" => Language::TypeScript,
        "javascript" => Language::JavaScript,
        "go" => Language::Go,
        "java" => Language::Java,
        "cpp" => Language::Cpp,
        _ => Language::Unknown,
    }
}

/// Convert a unit_type string back to the enum.
fn unit_type_from_str(s: &str) -> UnitType {
    match s {
        "function" => UnitType::Function,
        "method" => UnitType::Method,
        "class" => UnitType::Class,
        "struct" => UnitType::Struct,
        "enum" => UnitType::Enum,
        "trait" => UnitType::Trait,
        "interface" => UnitType::Interface,
        "module" => UnitType::Module,
        other => UnitType::Other(other.to_string()),
    }
}

/// Load all code units for a given file path.
#[allow(dead_code)]
pub fn units_for_file(conn: &Connection, file_path: &str) -> Result<Vec<CodeUnit>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, language, unit_type, name, full_signature, docstring,
                line_start, line_end, body, parameters, return_type,
                calls, imports, complexity, has_embedding, embedding_model
         FROM code_units WHERE file_path = ?1",
    )?;
    let units = stmt
        .query_map(params![file_path], row_to_unit)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(units)
}

/// Load all code units in the index.
#[allow(dead_code)]
pub fn all_units(conn: &Connection) -> Result<Vec<CodeUnit>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, language, unit_type, name, full_signature, docstring,
                line_start, line_end, body, parameters, return_type,
                calls, imports, complexity, has_embedding, embedding_model
         FROM code_units",
    )?;
    let units = stmt
        .query_map([], row_to_unit)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(units)
}

fn row_to_unit(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodeUnit> {
    let params_json: String = row.get(10)?;
    let calls_json: String = row.get(12)?;
    let imports_json: String = row.get(13)?;

    Ok(CodeUnit {
        id: row.get(0)?,
        file_path: row.get(1)?,
        language: lang_from_str(&row.get::<_, String>(2)?),
        unit_type: unit_type_from_str(&row.get::<_, String>(3)?),
        name: row.get(4)?,
        full_signature: row.get(5)?,
        docstring: row.get(6)?,
        line_start: row.get::<_, i64>(7)? as usize,
        line_end: row.get::<_, i64>(8)? as usize,
        body: row.get(9)?,
        parameters: serde_json::from_str(&params_json).unwrap_or_default(),
        return_type: row.get(11)?,
        calls: serde_json::from_str(&calls_json).unwrap_or_default(),
        imports: serde_json::from_str(&imports_json).unwrap_or_default(),
        complexity: row.get::<_, i64>(14)? as u32,
        has_embedding: row.get::<_, i32>(15)? != 0,
        embedding_model: row.get(16)?,
    })
}

/// Return the names + locations of all units that call `callee_name`.
pub fn callers_of(conn: &Connection, callee_name: &str) -> Result<Vec<(String, String, usize)>> {
    let mut stmt = conn.prepare(
        "SELECT cu.name, cu.file_path, cu.line_start
         FROM call_graph cg
         JOIN code_units cu ON cg.caller_id = cu.id
         WHERE cg.callee_name = ?1
         LIMIT 10",
    )?;
    let rows = stmt
        .query_map(params![callee_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)? as usize))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Return the names + locations of callees of the unit with the given id.
pub fn callees_of(conn: &Connection, caller_id: i64) -> Result<Vec<(String, String, usize)>> {
    let mut stmt = conn.prepare(
        "SELECT cu.name, cu.file_path, cu.line_start
         FROM call_graph cg
         JOIN code_units cu ON cu.name = cg.callee_name
         WHERE cg.caller_id = ?1
         LIMIT 10",
    )?;
    let rows = stmt
        .query_map(params![caller_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)? as usize))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Return functions/methods that are never called by anyone in the index.
pub fn unused_functions(conn: &Connection) -> Result<Vec<(String, String, usize, String)>> {
    let mut stmt = conn.prepare(
        "SELECT cu.name, cu.file_path, cu.line_start, cu.unit_type
         FROM code_units cu
         WHERE (cu.unit_type = 'function' OR cu.unit_type = 'method')
           AND cu.name NOT IN (SELECT callee_name FROM call_graph)
         ORDER BY cu.file_path, cu.line_start",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Return the `last_indexed` timestamp for a file, or `None` if not tracked.
pub fn get_file_last_indexed(conn: &Connection, file_path: &str) -> Option<i64> {
    conn.query_row(
        "SELECT last_indexed FROM file_index WHERE file_path = ?1",
        rusqlite::params![file_path],
        |r| r.get(0),
    )
    .ok()
}

/// Find the code unit whose range contains `line` in `file_path`.
pub fn unit_at_line(conn: &Connection, file_path: &str, line: usize) -> Option<CodeUnit> {
    let mut stmt = conn
        .prepare(
            "SELECT id, file_path, language, unit_type, name, full_signature, docstring,
                    line_start, line_end, body, parameters, return_type,
                    calls, imports, complexity, has_embedding, embedding_model
             FROM code_units
             WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2
             ORDER BY (line_end - line_start) ASC
             LIMIT 1",
        )
        .ok()?;
    stmt.query_row(rusqlite::params![file_path, line as i64], row_to_unit)
        .ok()
}

/// Load a single code unit by its row ID.
pub fn unit_by_id(conn: &Connection, id: i64) -> Option<CodeUnit> {
    let mut stmt = conn
        .prepare(
            "SELECT id, file_path, language, unit_type, name, full_signature, docstring,
                    line_start, line_end, body, parameters, return_type,
                    calls, imports, complexity, has_embedding, embedding_model
             FROM code_units WHERE id = ?1",
        )
        .ok()?;
    stmt.query_row(rusqlite::params![id], row_to_unit).ok()
}

/// Count total code units.
pub fn count_units(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM code_units", [], |r| r.get(0))?;
    Ok(n as usize)
}

/// Count indexed files.
pub fn count_files(conn: &Connection) -> Result<usize> {
    let n: i64 =
        conn.query_row("SELECT COUNT(*) FROM file_index", [], |r| r.get(0))?;
    Ok(n as usize)
}
