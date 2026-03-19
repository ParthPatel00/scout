//! `scout stats` — show index size, unit counts, language breakdown, and daemon status.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::index;
use crate::storage::sqlite;

pub struct StatsArgs {
    pub path: PathBuf,
}

pub fn run(args: StatsArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let db_path = index::db_path(&idx_dir);

    if !db_path.exists() {
        anyhow::bail!(
            "No index found at {}. Run `scout index` first.",
            root.display()
        );
    }

    let conn = sqlite::open(&db_path)?;
    let meta = index::load_metadata(&idx_dir).unwrap_or_else(|_| crate::types::IndexMetadata::new());

    // ── Code unit counts ──────────────────────────────────────────────────────

    let total_units: i64 = conn
        .query_row("SELECT COUNT(*) FROM code_units", [], |r| r.get(0))
        .unwrap_or(0);

    let total_files: i64 = conn
        .query_row("SELECT COUNT(*) FROM file_index", [], |r| r.get(0))
        .unwrap_or(0);

    // Per unit-type breakdown
    let mut by_type: Vec<(String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT unit_type, COUNT(*) as n FROM code_units GROUP BY unit_type ORDER BY n DESC",
        )?;
        let rows: Vec<_> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };
    by_type.sort_by(|a, b| b.1.cmp(&a.1));

    // Per-language breakdown
    let by_lang: Vec<(String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT language, COUNT(*) as n FROM code_units GROUP BY language ORDER BY n DESC",
        )?;
        let rows: Vec<_> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };

    // Call graph edges
    let call_edges: i64 = conn
        .query_row("SELECT COUNT(*) FROM call_graph", [], |r| r.get(0))
        .unwrap_or(0);

    // Docstrings
    let with_docs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_units WHERE docstring IS NOT NULL AND docstring != ''",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Embeddings
    let with_embeddings: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_units WHERE has_embedding = 1",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let embedding_model: Option<String> = conn
        .query_row(
            "SELECT embedding_model FROM code_units WHERE has_embedding = 1 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()
        .flatten();

    // ── Disk usage ────────────────────────────────────────────────────────────

    let db_size   = file_size(&db_path);
    let vec_size  = file_size(&idx_dir.join("vectors.bin"));
    let tvy_size  = dir_size(&idx_dir.join("tantivy"));
    let total_size = db_size + vec_size + tvy_size;

    // ── Daemon status ─────────────────────────────────────────────────────────

    let daemon_status = daemon_line(&idx_dir);

    // ── Last indexed ──────────────────────────────────────────────────────────

    let last_indexed = if meta.last_updated > 0 {
        let secs = chrono::Utc::now().timestamp() - meta.last_updated;
        format_age(secs)
    } else {
        "never".to_string()
    };

    // ── Render ────────────────────────────────────────────────────────────────

    let bold  = "\x1b[1m";
    let dim   = "\x1b[2m";
    let cyan  = "\x1b[36m";
    let reset = "\x1b[0m";

    println!("\n{bold}Index stats{reset}  {dim}{}{reset}\n", root.display());

    // Summary row
    println!(
        "  {bold}{:>7}{reset} functions/methods    {bold}{:>5}{reset} files    {bold}{:>5}{reset} call edges",
        fmt_n(total_units),
        fmt_n(total_files),
        fmt_n(call_edges),
    );
    println!();

    // Unit types
    println!("  {bold}Unit types{reset}");
    for (kind, n) in &by_type {
        println!("    {cyan}{:<18}{reset} {:>7}", kind, fmt_n(*n));
    }
    println!("    {dim}{:<18}{reset} {:>7}", "with docstrings", fmt_n(with_docs));
    println!();

    // Languages
    println!("  {bold}Languages{reset}");
    for (lang, n) in &by_lang {
        let pct = if total_units > 0 { (*n * 100) / total_units } else { 0 };
        let bar = bar(pct as usize, 20);
        println!("    {cyan}{:<14}{reset} {:>7}  {dim}{}{reset}  {}%", lang, fmt_n(*n), bar, pct);
    }
    println!();

    // Embeddings
    println!("  {bold}Embeddings{reset}");
    if total_units > 0 {
        let pct = (with_embeddings * 100) / total_units;
        let bar = bar(pct as usize, 20);
        println!(
            "    {}/{} units  {dim}{}{reset}  {}%",
            fmt_n(with_embeddings),
            fmt_n(total_units),
            bar,
            pct,
        );
        if let Some(model) = &embedding_model {
            println!("    {dim}model: {}{reset}", model);
        }
    } else {
        println!("    {dim}none{reset}");
    }
    println!();

    // Storage
    println!("  {bold}Storage{reset}");
    println!("    {:<22} {}", "database (metadata.db)", fmt_bytes(db_size));
    if vec_size > 0 {
        println!("    {:<22} {}", "vectors (vectors.bin)", fmt_bytes(vec_size));
    }
    if tvy_size > 0 {
        println!("    {:<22} {}", "tantivy index", fmt_bytes(tvy_size));
    }
    println!("    {dim}{:<22} {}{reset}", "total", fmt_bytes(total_size));
    println!();

    // Status
    println!("  {bold}Status{reset}");
    println!("    {:<22} {}", "last indexed", last_indexed);
    println!("    {:<22} v{}", "index version", meta.version);
    println!("    {:<22} {}", "daemon", daemon_status);
    println!();

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() { return 0; }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

fn fmt_bytes(bytes: u64) -> String {
    if bytes == 0         { return "—".to_string(); }
    if bytes < 1_024      { return format!("{} B",         bytes); }
    if bytes < 1_048_576  { return format!("{:.1} KB",     bytes as f64 / 1_024.0); }
    if bytes < 1_073_741_824 { return format!("{:.1} MB",  bytes as f64 / 1_048_576.0); }
    format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
}

fn fmt_n(n: i64) -> String {
    // Insert thousands separators.
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(','); }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn bar(pct: usize, width: usize) -> String {
    let filled = (pct * width) / 100;
    let filled = filled.min(width);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(width - filled))
}

fn format_age(secs: i64) -> String {
    if secs < 0         { return "just now".to_string(); }
    if secs < 60        { return format!("{secs}s ago"); }
    if secs < 3_600     { return format!("{}m ago",  secs / 60); }
    if secs < 86_400    { return format!("{}h {}m ago", secs / 3_600, (secs % 3_600) / 60); }
    format!("{}d ago", secs / 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fmt_n ──────────────────────────────────────────────────────────────────

    #[test]
    fn fmt_n_zero() {
        assert_eq!(fmt_n(0), "0");
    }

    #[test]
    fn fmt_n_small() {
        assert_eq!(fmt_n(42), "42");
        assert_eq!(fmt_n(999), "999");
    }

    #[test]
    fn fmt_n_thousands() {
        assert_eq!(fmt_n(1_000), "1,000");
        assert_eq!(fmt_n(10_000), "10,000");
        assert_eq!(fmt_n(1_234_567), "1,234,567");
    }

    // ── fmt_bytes ──────────────────────────────────────────────────────────────

    #[test]
    fn fmt_bytes_zero_is_dash() {
        assert_eq!(fmt_bytes(0), "—");
    }

    #[test]
    fn fmt_bytes_bytes() {
        assert_eq!(fmt_bytes(512), "512 B");
    }

    #[test]
    fn fmt_bytes_kilobytes() {
        assert_eq!(fmt_bytes(2048), "2.0 KB");
    }

    #[test]
    fn fmt_bytes_megabytes() {
        assert_eq!(fmt_bytes(4 * 1_048_576), "4.0 MB");
    }

    #[test]
    fn fmt_bytes_gigabytes() {
        assert_eq!(fmt_bytes(2 * 1_073_741_824), "2.00 GB");
    }

    // ── format_age ─────────────────────────────────────────────────────────────

    #[test]
    fn format_age_seconds() {
        assert_eq!(format_age(30), "30s ago");
    }

    #[test]
    fn format_age_minutes() {
        assert_eq!(format_age(90), "1m ago");
        assert_eq!(format_age(3599), "59m ago");
    }

    #[test]
    fn format_age_hours() {
        assert_eq!(format_age(3600), "1h 0m ago");
        assert_eq!(format_age(7322), "2h 2m ago");
    }

    #[test]
    fn format_age_days() {
        assert_eq!(format_age(86400), "1d ago");
        assert_eq!(format_age(86400 * 3), "3d ago");
    }

    #[test]
    fn format_age_negative_is_just_now() {
        assert_eq!(format_age(-5), "just now");
    }

    // ── bar ────────────────────────────────────────────────────────────────────

    #[test]
    fn bar_zero_pct() {
        let b = bar(0, 10);
        assert!(b.starts_with('['));
        assert!(b.ends_with(']'));
        assert!(!b.contains('█'));
    }

    #[test]
    fn bar_full_pct() {
        let b = bar(100, 10);
        assert!(!b.contains('░'));
    }

    #[test]
    fn bar_half_pct() {
        let b = bar(50, 10);
        assert_eq!(b, "[█████░░░░░]");
    }

    #[test]
    fn bar_clamped_above_100() {
        // Should not panic or overflow
        let b = bar(200, 10);
        assert!(!b.contains('░'));
    }

    // ── dir_size ───────────────────────────────────────────────────────────────

    #[test]
    fn dir_size_nonexistent_returns_zero() {
        assert_eq!(dir_size(std::path::Path::new("/this/does/not/exist")), 0);
    }

    #[test]
    fn dir_size_empty_dir_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(dir.path()), 0);
    }

    #[test]
    fn dir_size_counts_file_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("b.txt"), "world!").unwrap();
        assert_eq!(dir_size(dir.path()), 11); // 5 + 6
    }
}

fn daemon_line(idx_dir: &std::path::Path) -> String {
    let pid_path = idx_dir.join("daemon.pid");
    let Ok(bytes) = std::fs::read(&pid_path) else {
        return "\x1b[2mnot running\x1b[0m".to_string();
    };
    let Ok(state) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return "\x1b[2mnot running\x1b[0m".to_string();
    };
    let pid = state["pid"].as_u64().unwrap_or(0) as u32;

    let running = {
        #[cfg(unix)]
        { unsafe { libc::kill(pid as libc::pid_t, 0) == 0 } }
        #[cfg(not(unix))]
        { false }
    };

    if running {
        let started = state["started_at"].as_i64().unwrap_or(0);
        let uptime = chrono::Utc::now().timestamp() - started;
        format!("\x1b[32mrunning\x1b[0m  PID {}  uptime {}", pid, format_age(uptime))
    } else {
        "\x1b[2mnot running\x1b[0m".to_string()
    }
}
