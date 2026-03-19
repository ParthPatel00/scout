use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::index;
use crate::storage::sqlite;

pub struct ReportArgs {
    pub path: PathBuf,
    pub kind: ReportKind,
}

pub enum ReportKind {
    UnusedFunctions,
}

pub fn run(args: ReportArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let db_path = index::db_path(&idx_dir);

    if !db_path.exists() {
        bail!(
            "No index found at {}. Run `codesearch index` first.",
            root.display()
        );
    }

    let conn = sqlite::open(&db_path)?;

    match args.kind {
        ReportKind::UnusedFunctions => report_unused(&conn),
    }
}

fn report_unused(conn: &rusqlite::Connection) -> Result<()> {
    let unused = sqlite::unused_functions(conn)?;

    if unused.is_empty() {
        println!("No unused functions found.");
        return Ok(());
    }

    println!("{} potentially unused functions/methods:\n", unused.len());
    let mut current_file = String::new();

    for (name, file, line, unit_type) in &unused {
        if *file != current_file {
            println!("\x1b[1m{file}\x1b[0m");
            current_file = file.clone();
        }
        println!("  {line:>5}  {unit_type:<8}  {name}");
    }

    println!(
        "\n\x1b[2mNote: only static call sites in indexed files are considered.\n\
         Dynamic dispatch, reflection, and external callers are not detected.\x1b[0m"
    );

    Ok(())
}
