use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::index::{self, walker};
use crate::storage::{sqlite, tantivy_store};
use crate::types::{FileRecord, Language};

pub struct IndexArgs {
    pub path: PathBuf,
    pub verbose: bool,
}

pub fn run(args: IndexArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let db_path = index::db_path(&idx_dir);

    let conn = sqlite::open(&db_path)?;
    sqlite::initialize_schema(&conn)?;

    let tantivy_dir = idx_dir.join("tantivy");
    let (tantivy_index, tantivy_schema) = tantivy_store::open_index(&tantivy_dir)?;
    let mut writer = tantivy_index
        .writer(50_000_000)
        .context("failed to open tantivy writer")?;

    let mut meta = index::load_metadata(&idx_dir)?;
    let start = Instant::now();

    let files = walker::walk_source_files(&root);
    if args.verbose {
        println!("Found {} source files to consider", files.len());
    }

    let mut indexed = 0usize;
    let mut skipped = 0usize;
    let mut total_units = 0usize;

    for file_path in &files {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };

        let hash = sha256_hex(&content);
        let rel_path = file_path
            .strip_prefix(&root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        // Skip unchanged files.
        if let Ok(Some(stored_hash)) = sqlite::get_file_hash(&conn, &rel_path) {
            if stored_hash == hash {
                if args.verbose {
                    println!("  skip (unchanged): {rel_path}");
                }
                skipped += 1;
                continue;
            }
        }

        let lang = language_for_path(file_path);
        let units = match crate::index::parser::parse_file(&rel_path, &content, &lang) {
            Ok(u) => u,
            Err(e) => {
                if args.verbose {
                    eprintln!("  warn: parse error in {rel_path}: {e}");
                }
                vec![]
            }
        };

        // Delete stale records for this file, then insert fresh ones.
        sqlite::delete_units_for_file(&conn, &rel_path)?;
        let unit_count = units.len();
        let mut inserted_units = Vec::with_capacity(units.len());
        for mut unit in units {
            let id = sqlite::insert_unit(&conn, &unit)?;
            unit.id = id;
            inserted_units.push(unit);
        }

        // Update Tantivy BM25 index.
        tantivy_store::index_file_units(&mut writer, &tantivy_schema, &rel_path, &inserted_units)?;

        let now = chrono::Utc::now().timestamp();
        sqlite::upsert_file_record(
            &conn,
            &FileRecord {
                file_path: rel_path.clone(),
                file_hash: hash,
                last_indexed: now,
                needs_reindex: false,
            },
        )?;

        total_units += unit_count;
        indexed += 1;

        if args.verbose {
            println!("  indexed ({unit_count} units): {rel_path}");
        }
    }

    // Commit the Tantivy writer so searches see the new data.
    writer.commit().context("failed to commit tantivy index")?;

    // Update and persist metadata.
    let now = chrono::Utc::now().timestamp();
    meta.last_updated = now;
    if meta.created_at == 0 {
        meta.created_at = now;
    }
    meta.num_files = sqlite::count_files(&conn)?;
    meta.num_units = sqlite::count_units(&conn)?;
    index::save_metadata(&idx_dir, &meta)?;

    let elapsed = start.elapsed();
    println!(
        "Indexed {indexed} files ({total_units} new units, {skipped} unchanged) in {:.2}s",
        elapsed.as_secs_f64()
    );
    println!(
        "Index totals: {} files, {} units — {}",
        meta.num_files,
        meta.num_units,
        idx_dir.display()
    );

    Ok(())
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

fn language_for_path(path: &Path) -> Language {
    path.extension()
        .and_then(|e| e.to_str())
        .map(Language::from_extension)
        .unwrap_or(Language::Unknown)
}
