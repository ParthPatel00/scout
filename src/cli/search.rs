use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

use crate::index;
use crate::cli::OutputFormat;
use crate::search::{bm25, hybrid, rrf, SearchFilter};
use crate::storage::sqlite;

pub struct SearchArgs {
    pub path: PathBuf,
    pub query: String,
    pub limit: usize,
    pub filter: SearchFilter,
    pub show_context: bool,
    pub format: Option<OutputFormat>,
    pub use_tui: bool,
    /// Use semantic (vector) search only.
    pub semantic: bool,
    /// Hybrid mode: BM25 + vector + name-match (highest quality).
    pub best: bool,
}

pub fn run(args: SearchArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let tantivy_dir = idx_dir.join("tantivy");
    let vector_path = idx_dir.join("vectors.bin");

    if !tantivy_dir.join("meta.json").exists() {
        bail!(
            "No index found at {}. Run `codesearch index` first.",
            root.display()
        );
    }

    let use_vectors = args.semantic || args.best;
    let model: Option<Box<dyn crate::ml::EmbeddingModel>> = if use_vectors {
        match crate::ml::model::load_model() {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("warning: could not load embedding model ({e}), falling back to BM25");
                if !crate::ml::model::is_model_downloaded() {
                    crate::ml::model::print_download_instructions();
                }
                None
            }
        }
    } else {
        None
    };

    let results = if use_vectors {
        hybrid::search(
            &tantivy_dir,
            &vector_path,
            &args.query,
            args.limit,
            &args.filter,
            model.as_deref(),
        )?
    } else {
        let bm25_results = bm25::search(&tantivy_dir, &args.query, args.limit, &args.filter)?;
        rrf::fuse(&args.query, bm25_results)
    };

    if results.is_empty() {
        eprintln!("No results for {:?}", args.query);
        return Ok(());
    }

    // Launch TUI when in a terminal with no format override.
    if args.use_tui {
        return crate::tui::run(args.query, results);
    }

    match args.format {
        Some(OutputFormat::Json) => output_json(&results),
        Some(OutputFormat::Csv) => output_csv(&results),
        _ => output_plain(&args, &idx_dir, results),
    }
}

// ─── Plain text ───────────────────────────────────────────────────────────────

fn output_plain(
    args: &SearchArgs,
    idx_dir: &std::path::Path,
    results: Vec<crate::types::SearchResult>,
) -> Result<()> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];

    let conn = if args.show_context {
        let db_path = index::db_path(idx_dir);
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

// ─── JSON ─────────────────────────────────────────────────────────────────────

fn output_json(results: &[crate::types::SearchResult]) -> Result<()> {
    let records: Vec<serde_json::Value> = results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            serde_json::json!({
                "rank": i + 1,
                "score": r.score,
                "name": r.unit.name,
                "unit_type": r.unit.unit_type.to_string(),
                "language": r.unit.language.to_string(),
                "file_path": r.unit.file_path,
                "line_start": r.unit.line_start,
                "line_end": r.unit.line_end,
                "signature": r.unit.full_signature,
                "docstring": r.unit.docstring,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&records)?);
    Ok(())
}

// ─── CSV ──────────────────────────────────────────────────────────────────────

fn output_csv(results: &[crate::types::SearchResult]) -> Result<()> {
    println!("rank,score,name,unit_type,language,file_path,line_start");
    for (i, r) in results.iter().enumerate() {
        println!(
            "{},{:.4},{},{},{},{},{}",
            i + 1,
            r.score,
            csv_escape(&r.unit.name),
            r.unit.unit_type,
            r.unit.language,
            csv_escape(&r.unit.file_path),
            r.unit.line_start,
        );
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

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
