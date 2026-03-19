/// Multi-repo registry stored at `~/.config/codesearch/repos.json`.
///
/// Each entry records a human-readable name and an absolute path so that
/// cross-repo search commands can locate every registered index.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

// ─── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    /// Short name used in `--repos backend,frontend`.
    pub name: String,
    /// Absolute path to the repo root.
    pub path: PathBuf,
    /// Unix timestamp when this repo was registered.
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    pub repos: Vec<RepoEntry>,
}

// ─── Path helper ──────────────────────────────────────────────────────────────

pub fn registry_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("scout")
        .join("repos.json")
}

// ─── Registry impl ────────────────────────────────────────────────────────────

impl Registry {
    /// Load the registry from disk (returns empty registry if file absent).
    pub fn load() -> Result<Self> {
        let path = registry_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&contents).context("failed to parse repos.json")
    }

    /// Persist the registry to disk atomically.
    pub fn save(&self) -> Result<()> {
        let path = registry_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, contents)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).context("failed to save repos.json")?;
        Ok(())
    }

    /// Register a repo. Errors if a repo with the same name already exists.
    pub fn add(&mut self, name: &str, path: PathBuf) -> Result<()> {
        if self.repos.iter().any(|r| r.name == name) {
            bail!("A repo named '{name}' is already registered. Remove it first.");
        }
        self.repos.push(RepoEntry {
            name: name.to_string(),
            path,
            added_at: chrono::Utc::now().timestamp(),
        });
        Ok(())
    }

    /// Remove a repo by name. Returns true if it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.repos.len();
        self.repos.retain(|r| r.name != name);
        self.repos.len() < before
    }

    /// Look up a repo entry by name.
    pub fn find(&self, name: &str) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| r.name == name)
    }

    /// Return repos matching the comma-separated `names` list.
    /// Errors if any name is not registered.
    pub fn resolve_names(&self, names: &str) -> Result<Vec<&RepoEntry>> {
        names
            .split(',')
            .map(|n| {
                let n = n.trim();
                self.find(n)
                    .ok_or_else(|| anyhow::anyhow!("no registered repo named '{n}'"))
            })
            .collect()
    }
}

// ─── Index status ─────────────────────────────────────────────────────────────

/// Return true if a `.codesearch/metadata.db` exists under `repo_path`.
pub fn is_indexed(repo_path: &Path) -> bool {
    repo_path
        .join(".codesearch")
        .join("metadata.db")
        .exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_registry() -> (TempDir, Registry) {
        (TempDir::new().unwrap(), Registry::default())
    }

    #[test]
    fn add_and_remove() {
        let (_tmp, mut reg) = tmp_registry();
        reg.add("backend", PathBuf::from("/proj/backend")).unwrap();
        reg.add("frontend", PathBuf::from("/proj/frontend")).unwrap();
        assert_eq!(reg.repos.len(), 2);
        assert!(reg.remove("backend"));
        assert_eq!(reg.repos.len(), 1);
        assert!(!reg.remove("missing"));
    }

    #[test]
    fn duplicate_name_errors() {
        let (_tmp, mut reg) = tmp_registry();
        reg.add("svc", PathBuf::from("/a")).unwrap();
        assert!(reg.add("svc", PathBuf::from("/b")).is_err());
    }

    #[test]
    fn resolve_names() {
        let (_tmp, mut reg) = tmp_registry();
        reg.add("a", PathBuf::from("/a")).unwrap();
        reg.add("b", PathBuf::from("/b")).unwrap();
        let resolved = reg.resolve_names("a,b").unwrap();
        assert_eq!(resolved.len(), 2);
        assert!(reg.resolve_names("a,missing").is_err());
    }
}
