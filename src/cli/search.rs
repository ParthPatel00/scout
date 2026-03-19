use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

use crate::index;
use crate::search::bm25;

pub struct SearchArgs {
    pub path: PathBuf,
    pub query: String,
    pub limit: usize,
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

    let results = bm25::search(&tantivy_dir, &args.query, args.limit)?;

    if results.is_empty() {
        println!("No results for {:?}", args.query);
        return Ok(());
    }

    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];

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
            // Print syntax-highlighted signature.
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
    }
    println!();

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
