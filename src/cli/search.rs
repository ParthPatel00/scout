use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

use crate::index;
use crate::search::{bm25, rrf, SearchFilter};
use crate::storage::sqlite;

pub struct SearchArgs {
    pub path: PathBuf,
    pub query: String,
    pub limit: usize,
    pub filter: SearchFilter,
    pub show_context: bool,
}

pub fn run(args: SearchArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let tantivy_dir = idx_dir.join("tantivy");

    if !tantivy_dir.join("meta.json").exists() {
        bail!(
            "No index found at {}. Run `codesearch index` first.",
            root.display()
        );
    }

    // Fetch BM25 results (with filters applied).
    let bm25_results = bm25::search(&tantivy_dir, &args.query, args.limit, &args.filter)?;

    // Re-rank via RRF (name-match + BM25 fusion).
    let results = rrf::fuse(&args.query, bm25_results);

    if results.is_empty() {
        println!("No results for {:?}", args.query);
        return Ok(());
    }

    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];

    // Open SQLite only if --show-context is requested.
    let conn = if args.show_context {
        let db_path = index::db_path(&idx_dir);
        Some(sqlite::open(&db_path)?)
    } else {
        None
    };

    for (i, result) in results.iter().enumerate() {
        let unit = &result.unit;
        println!(
            "\n{rank}. [{score:.2}] {unit_type} \x1b[1m{name}\x1b[0m",
            rank = i + 1,
            score = result.score,
            unit_type = unit.unit_type,
            name = unit.name,
        );
        println!(
            "   \x1b[2m{file}:{line}\x1b[0m   [{lang}]",
            file = unit.file_path,
            line = unit.line_start,
            lang = unit.language,
        );

        if let Some(sig) = &unit.full_signature {
            let syntax = ss
                .find_syntax_by_extension(lang_ext(unit.language.as_str()))
                .unwrap_or_else(|| ss.find_syntax_plain_text());
            let mut h = HighlightLines::new(syntax, theme);
            let highlighted: String = LinesWithEndings::from(sig)
                .filter_map(|line| h.highlight_line(line, &ss).ok())
                .map(|ranges| as_24_bit_terminal_escaped(&ranges, false))
                .collect();
            println!("   {}", highlighted.trim_end());
        } else if !result.snippet.is_empty() {
            println!("   {}", result.snippet);
        }

        // --show-context: display callers and callees from call graph.
        if let Some(conn) = &conn {
            print_context(conn, unit.id, &unit.name)?;
        }
    }
    println!();

    Ok(())
}

fn print_context(conn: &rusqlite::Connection, unit_id: i64, unit_name: &str) -> Result<()> {
    let callers = sqlite::callers_of(conn, unit_name)?;
    if !callers.is_empty() {
        print!("   \x1b[2mCallers:\x1b[0m");
        for (name, file, line) in &callers {
            print!("  {name} ({file}:{line})");
        }
        println!();
    }

    let callees = sqlite::callees_of(conn, unit_id)?;
    if !callees.is_empty() {
        print!("   \x1b[2mCalls:\x1b[0m  ");
        for (name, file, line) in &callees {
            print!("  {name} ({file}:{line})");
        }
        println!();
    }

    Ok(())
}

fn lang_ext(lang: &str) -> &str {
    match lang {
        "python" => "py",
        "rust" => "rs",
        "typescript" => "ts",
        "javascript" => "js",
        "go" => "go",
        "java" => "java",
        "cpp" => "cpp",
        _ => "txt",
    }
}
