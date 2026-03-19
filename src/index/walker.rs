use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::types::Language;

/// Directories that should never be indexed.
const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".venv",
    "venv",
    "env",
    ".env",
    ".tox",
    "vendor",
    ".cargo",
    "bazel-out",
    ".next",
    ".nuxt",
    "coverage",
    ".coverage",
    "htmlcoverage",
    ".idea",
    ".vscode",
    ".DS_Store",
];

/// Maximum file size (bytes) — skip larger files.
const MAX_FILE_BYTES: u64 = 1_000_000; // 1 MB

/// Maximum number of lines — skip files that exceed this.
const MAX_FILE_LINES: usize = 10_000;

/// Walk `root`, yielding paths to source files that should be indexed.
pub fn walk_source_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_excluded(e))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| is_indexable(e.path()))
        .filter(|e| {
            // Skip files that are too large.
            e.metadata()
                .map(|m| m.len() <= MAX_FILE_BYTES)
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect()
}

/// Returns `true` if this directory entry should be excluded entirely.
fn is_excluded(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    entry
        .file_name()
        .to_str()
        .map(|name| EXCLUDED_DIRS.contains(&name))
        .unwrap_or(false)
}

/// Returns `true` if the file has a supported source extension.
fn is_indexable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| !matches!(Language::from_extension(ext), Language::Unknown))
        .unwrap_or(false)
}

/// Count the number of newlines in `content`, returning `true` if within limit.
pub fn within_line_limit(content: &str) -> bool {
    content.chars().filter(|&c| c == '\n').count() < MAX_FILE_LINES
}

/// Returns the set of excluded directory names (for the native watcher).
pub fn excluded_dirs() -> std::collections::HashSet<&'static str> {
    EXCLUDED_DIRS.iter().copied().collect()
}

/// Returns true if the path has a supported source file extension.
pub fn is_supported_extension(path: &std::path::Path) -> bool {
    is_indexable(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_file(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn excludes_node_modules() {
        let tmp = TempDir::new().unwrap();
        make_file(tmp.path(), "src/main.rs", "fn main() {}");
        make_file(tmp.path(), "node_modules/foo/index.js", "module.exports = {}");

        let files = walk_source_files(tmp.path());
        assert!(files.iter().any(|p| p.ends_with("src/main.rs")));
        assert!(!files.iter().any(|p| p.to_str().unwrap().contains("node_modules")));
    }

    #[test]
    fn excludes_dot_git() {
        let tmp = TempDir::new().unwrap();
        make_file(tmp.path(), "lib.py", "def foo(): pass");
        make_file(tmp.path(), ".git/config", "[core]");

        let files = walk_source_files(tmp.path());
        assert!(!files.iter().any(|p| p.to_str().unwrap().contains(".git")));
    }

    #[test]
    fn collects_supported_extensions() {
        let tmp = TempDir::new().unwrap();
        make_file(tmp.path(), "a.py", "x = 1");
        make_file(tmp.path(), "b.rs", "fn f() {}");
        make_file(tmp.path(), "c.ts", "const x = 1");
        make_file(tmp.path(), "d.go", "package main");
        make_file(tmp.path(), "e.md", "# docs");
        make_file(tmp.path(), "f.txt", "hello");

        let files = walk_source_files(tmp.path());
        assert_eq!(files.len(), 4); // .md and .txt excluded
    }
}
