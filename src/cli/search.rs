use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;

use crate::index;
use crate::cli::OutputFormat;
use crate::repo::registry::Registry;
use crate::search::{cross_repo, hybrid, SearchFilter};
use crate::storage::sqlite;

pub struct SearchArgs {
    pub path: PathBuf,
    pub query: String,
    pub limit: usize,
    pub filter: SearchFilter,
    pub show_context: bool,
    pub format: Option<OutputFormat>,
    pub use_tui: bool,
    /// Force pure vector-only search (skips BM25 entirely).
    pub semantic: bool,
    /// Search all registered repos.
    pub all_repos: bool,
    /// Comma-separated repo names to search.
    pub repos: Option<String>,
    /// Find functions similar to the one at FILE:LINE.
    pub find_similar: Option<String>,
}

pub fn run(args: SearchArgs) -> Result<()> {
    // ── find-similar mode ────────────────────────────────────────────────────
    if let Some(ref loc) = args.find_similar {
        return run_find_similar(&args, loc);
    }

    // ── cross-repo mode ──────────────────────────────────────────────────────
    if args.all_repos || args.repos.is_some() {
        return run_cross_repo(&args);
    }

    // ── single-repo mode ─────────────────────────────────────────────────────
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let tantivy_dir = idx_dir.join("tantivy");
    let vector_path = idx_dir.join("vectors.bin");

    if !tantivy_dir.join("meta.json").exists() {
        bail!(
            "No index found at {}. Run `scout index` first.",
            root.display()
        );
    }

    // Always try to load the embedding model silently.
    // With model → 3-component fusion (BM25 + name-match + vector).
    // Without model → 2-component fusion (BM25 + name-match). No warning.
    // --semantic is the one exception: it means "vector only, no BM25".
    let model: Option<Box<dyn crate::ml::EmbeddingModel>> =
        crate::ml::model::load_model().ok().map(|m| m);

    let results = if args.semantic {
        // Pure vector search: only warn if the model is genuinely missing.
        match &model {
            Some(m) => hybrid::search_semantic_only(
                &vector_path,
                &args.query,
                args.limit,
                &args.filter,
                m.as_ref(),
            )?,
            None => {
                if !crate::ml::model::is_model_downloaded() {
                    eprintln!("--semantic requires the embedding model.");
                    crate::ml::model::print_download_instructions();
                } else {
                    #[cfg(not(feature = "local-models"))]
                    eprintln!(
                        "Local model support is not compiled in. Rebuild with:\n  \
                         cargo build --release --features local-models"
                    );
                }
                return Ok(());
            }
        }
    } else {
        // Default: hybrid fusion. Falls back to BM25+name-match if no model.
        hybrid::search(
            &tantivy_dir,
            &vector_path,
            &args.query,
            args.limit,
            &args.filter,
            model.as_deref(),
        )?
    };

    // Apply modified-since filter via SQLite (Tantivy doesn't store timestamps).
    let results = if let Some(since) = args.filter.modified_since {
        let db_path = index::db_path(&idx_dir);
        let conn = sqlite::open(&db_path)?;
        results
            .into_iter()
            .filter(|r| {
                sqlite::get_file_last_indexed(&conn, &r.unit.file_path)
                    .map(|t| t >= since)
                    .unwrap_or(false)
            })
            .collect()
    } else {
        results
    };

    if results.is_empty() {
        eprintln!("No results for \"{}\"", args.query);
        return Ok(());
    }

    // Launch TUI when in a terminal with no format override.
    if args.use_tui {
        return crate::tui::run(args.query, results, root);
    }

    match args.format {
        Some(OutputFormat::Json) => output_json(&results),
        Some(OutputFormat::Csv) => output_csv(&results),
        _ => output_plain(&args, &idx_dir, results),
    }
}

// ─── Cross-repo helpers ───────────────────────────────────────────────────────

fn run_cross_repo(args: &SearchArgs) -> Result<()> {
    let registry = Registry::load()?;
    let entries: Vec<&crate::repo::registry::RepoEntry> = if args.all_repos {
        registry.repos.iter().collect()
    } else {
        registry.resolve_names(args.repos.as_deref().unwrap_or(""))?
    };

    if entries.is_empty() {
        bail!("No repos selected. Register repos with `scout repos add`.");
    }

    let hits = cross_repo::search_repos(&entries, &args.query, args.limit, &args.filter, None)?;

    if hits.is_empty() {
        eprintln!("No results for \"{}\"", args.query);
        return Ok(());
    }

    match &args.format {
        Some(OutputFormat::Json) => {
            let records: Vec<serde_json::Value> = hits
                .iter()
                .enumerate()
                .map(|(i, (repo, r))| {
                    serde_json::json!({
                        "rank": i + 1,
                        "repo": repo,
                        "score": r.score,
                        "name": r.unit.name,
                        "unit_type": r.unit.unit_type.to_string(),
                        "language": r.unit.language.to_string(),
                        "file_path": r.unit.file_path,
                        "line_start": r.unit.line_start,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        Some(OutputFormat::Csv) => {
            println!("rank,repo,score,name,unit_type,language,file_path,line_start");
            for (i, (repo, r)) in hits.iter().enumerate() {
                println!(
                    "{},{},{:.4},{},{},{},{},{}",
                    i + 1,
                    csv_escape(repo),
                    r.score,
                    csv_escape(&r.unit.name),
                    r.unit.unit_type,
                    r.unit.language,
                    csv_escape(&r.unit.file_path),
                    r.unit.line_start,
                );
            }
        }
        _ => {
            for (repo, r) in hits.iter() {
                println!(
                    "\x1b[2m[{repo}]\x1b[0m \x1b[2m{file}:{line}\x1b[0m  \x1b[1m{name}\x1b[0m  \x1b[2m{unit_type} · {lang}\x1b[0m",
                    file = r.unit.file_path,
                    line = r.unit.line_start,
                    name = r.unit.name,
                    unit_type = r.unit.unit_type,
                    lang = r.unit.language,
                );
                println!();
            }
        }
    }
    Ok(())
}

fn run_find_similar(args: &SearchArgs, loc: &str) -> Result<()> {
    let (file_path, line) = parse_file_line(loc)?;
    let root = args.path.canonicalize().context("path not found")?;
    let registry = Registry::load()?;

    let hits = cross_repo::find_similar(&registry, &root, &file_path, line, args.limit)?;

    if hits.is_empty() {
        eprintln!("No similar functions found.");
        return Ok(());
    }

    println!("Functions similar to {file_path}:{line}:\n");
    for (repo, r) in hits.iter() {
        let repo_prefix = if repo.is_empty() {
            String::new()
        } else {
            format!("\x1b[2m[{repo}]\x1b[0m ")
        };
        println!(
            "{repo_prefix}\x1b[2m{file}:{line}\x1b[0m  \x1b[1m{name}\x1b[0m  \x1b[2m{unit_type} · {lang}\x1b[0m",
            file = r.unit.file_path,
            line = r.unit.line_start,
            name = r.unit.name,
            unit_type = r.unit.unit_type,
            lang = r.unit.language,
        );
        println!();
    }
    Ok(())
}

fn parse_file_line(loc: &str) -> Result<(String, usize)> {
    if let Some((file, line_str)) = loc.rsplit_once(':') {
        let line: usize = line_str
            .parse()
            .with_context(|| format!("invalid line number in '{loc}'"))?;
        Ok((file.to_string(), line))
    } else {
        bail!("--find-similar requires FILE:LINE format (e.g. src/auth.py:42)");
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

    for result in results.iter() {
        let unit = &result.unit;

        // Repo tag (only for cross-repo results)
        let repo_prefix = result
            .repo_name
            .as_deref()
            .map(|r| format!("\x1b[2m[{r}]\x1b[0m "))
            .unwrap_or_default();

        // Location in dim, name in bold
        println!(
            "{repo_prefix}\x1b[2m{file}:{line}\x1b[0m  \x1b[1m{name}\x1b[0m  \x1b[2m{unit_type} · {lang}\x1b[0m",
            file = unit.file_path,
            line = unit.line_start,
            name = unit.name,
            unit_type = unit.unit_type,
            lang = unit.language,
        );

        if let Some(sig) = &unit.full_signature {
            let first_line = sig.lines().next().unwrap_or("").trim();
            if !first_line.is_empty() {
                let syntax = ss
                    .find_syntax_by_extension(lang_ext(unit.language.as_str()))
                    .unwrap_or_else(|| ss.find_syntax_plain_text());
                let mut h = HighlightLines::new(syntax, theme);
                if let Ok(ranges) = h.highlight_line(first_line, &ss) {
                    let highlighted = as_24_bit_terminal_escaped(&ranges, false);
                    println!("  {highlighted}");
                }
            }
        } else if !result.snippet.is_empty() {
            println!("  \x1b[2m{}\x1b[0m", result.snippet);
        }

        if let Some(conn) = &conn {
            print_context(conn, unit.id, &unit.name)?;
        }

        println!();
    }
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
                "repo": r.repo_name,
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
