//! Persistent user configuration at `~/.config/scout/config.toml`.
//!
//! Config values are overridden by explicit CLI flags — the precedence chain is:
//!   CLI flag > config file > built-in default

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ─── Config structs ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Config {
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub editor: EditorConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchConfig {
    /// Default number of results (overridden by --limit).
    pub limit: usize,
    /// Always use plain-text output, never launch TUI.
    pub no_tui: bool,
    /// Default output format: "plain", "json", or "csv".
    pub format: Option<String>,
    /// Always exclude test files from results.
    pub exclude_tests: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            limit: 10,
            no_tui: false,
            format: None,
            exclude_tests: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct IndexConfig {
    /// Automatically index the current directory when no index is found.
    pub auto_index: bool,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct EditorConfig {
    /// Override editor auto-detection (e.g. "code", "nvim", "zed").
    pub command: Option<String>,
}

// ─── I/O ─────────────────────────────────────────────────────────────────────

pub fn config_path() -> PathBuf {
    dirs_path().join("config.toml")
}

fn dirs_path() -> PathBuf {
    // ~/.config/scout/
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scout")
}

impl Config {
    /// Load config from disk. Returns `Default` if no config file exists yet.
    pub fn load() -> Self {
        let path = config_path();
        if !path.exists() {
            return Self::default();
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        toml::from_str(&raw).unwrap_or_default()
    }

    /// Write config to `~/.config/scout/config.toml`.
    pub fn save(&self) -> Result<()> {
        let dir = dirs_path();
        std::fs::create_dir_all(&dir)?;
        let toml = toml::to_string_pretty(self)?;
        std::fs::write(config_path(), toml)?;
        Ok(())
    }

    /// Set a dot-separated key to a string value.
    /// Supported keys: search.limit, search.no_tui, search.format,
    ///                  search.exclude_tests, index.auto_index, editor.command
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "search.limit" => {
                self.search.limit = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("search.limit must be a number"))?;
            }
            "search.no_tui" => {
                self.search.no_tui = parse_bool(value)?;
            }
            "search.format" => {
                match value {
                    "plain" | "json" | "csv" => self.search.format = Some(value.to_string()),
                    _ => anyhow::bail!("search.format must be plain, json, or csv"),
                }
            }
            "search.exclude_tests" => {
                self.search.exclude_tests = parse_bool(value)?;
            }
            "index.auto_index" => {
                self.index.auto_index = parse_bool(value)?;
            }
            "editor.command" => {
                self.editor.command = if value.is_empty() || value == "auto" {
                    None
                } else {
                    Some(value.to_string())
                };
            }
            _ => anyhow::bail!(
                "Unknown config key '{}'. Run `scout config list` to see all keys.",
                key
            ),
        }
        Ok(())
    }

    /// Get a config value as a string.
    pub fn get(&self, key: &str) -> Result<String> {
        let v = match key {
            "search.limit" => self.search.limit.to_string(),
            "search.no_tui" => self.search.no_tui.to_string(),
            "search.format" => self
                .search
                .format
                .clone()
                .unwrap_or_else(|| "plain".to_string()),
            "search.exclude_tests" => self.search.exclude_tests.to_string(),
            "index.auto_index" => self.index.auto_index.to_string(),
            "editor.command" => self
                .editor
                .command
                .clone()
                .unwrap_or_else(|| "(auto-detect)".to_string()),
            _ => anyhow::bail!(
                "Unknown config key '{}'. Run `scout config list` to see all keys.",
                key
            ),
        };
        Ok(v)
    }

    /// Print all config keys, their current values, and descriptions.
    pub fn list(&self) {
        let path = config_path();
        println!(
            "Config file: {}\n",
            if path.exists() {
                path.display().to_string()
            } else {
                format!("{} (not yet created)", path.display())
            }
        );

        let rows = [
            ("search.limit",         self.search.limit.to_string(),
             "Default number of results"),
            ("search.no_tui",        self.search.no_tui.to_string(),
             "Always use plain text, never launch TUI"),
            ("search.format",        self.search.format.clone().unwrap_or_else(|| "plain".to_string()),
             "Default output format (plain / json / csv)"),
            ("search.exclude_tests", self.search.exclude_tests.to_string(),
             "Always exclude test files"),
            ("index.auto_index",     self.index.auto_index.to_string(),
             "Auto-index when no index found"),
            ("editor.command",       self.editor.command.clone().unwrap_or_else(|| "(auto-detect)".to_string()),
             "Editor to open results in"),
        ];

        let key_w = rows.iter().map(|(k, _, _)| k.len()).max().unwrap_or(0);
        let val_w = rows.iter().map(|(_, v, _)| v.len()).max().unwrap_or(0);

        for (key, val, desc) in &rows {
            println!(
                "  {:<key_w$}  {:<val_w$}  \x1b[2m# {}\x1b[0m",
                key,
                val,
                desc,
                key_w = key_w,
                val_w = val_w,
            );
        }
    }
}

fn parse_bool(s: &str) -> Result<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => Ok(true),
        "false" | "no" | "0" | "off" => Ok(false),
        _ => anyhow::bail!("expected true/false, got '{}'", s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> Config {
        Config::default()
    }

    // ── defaults ──────────────────────────────────────────────────────────────

    #[test]
    fn default_values() {
        let cfg = default_config();
        assert_eq!(cfg.search.limit, 10);
        assert!(!cfg.search.no_tui);
        assert!(cfg.search.format.is_none());
        assert!(!cfg.search.exclude_tests);
        assert!(!cfg.index.auto_index);
        assert!(cfg.editor.command.is_none());
    }

    // ── Config::get ───────────────────────────────────────────────────────────

    #[test]
    fn get_known_keys_return_defaults() {
        let cfg = default_config();
        assert_eq!(cfg.get("search.limit").unwrap(), "10");
        assert_eq!(cfg.get("search.no_tui").unwrap(), "false");
        assert_eq!(cfg.get("search.format").unwrap(), "plain");
        assert_eq!(cfg.get("search.exclude_tests").unwrap(), "false");
        assert_eq!(cfg.get("index.auto_index").unwrap(), "false");
        assert_eq!(cfg.get("editor.command").unwrap(), "(auto-detect)");
    }

    #[test]
    fn get_unknown_key_errors() {
        assert!(default_config().get("unknown.key").is_err());
    }

    // ── Config::set ───────────────────────────────────────────────────────────

    #[test]
    fn set_search_limit() {
        let mut cfg = default_config();
        cfg.set("search.limit", "25").unwrap();
        assert_eq!(cfg.search.limit, 25);
        assert_eq!(cfg.get("search.limit").unwrap(), "25");
    }

    #[test]
    fn set_search_limit_invalid_errors() {
        let mut cfg = default_config();
        assert!(cfg.set("search.limit", "not_a_number").is_err());
    }

    #[test]
    fn set_search_no_tui_variants() {
        let mut cfg = default_config();
        for truthy in &["true", "yes", "1", "on"] {
            cfg.set("search.no_tui", truthy).unwrap();
            assert!(cfg.search.no_tui, "expected true for '{}'", truthy);
        }
        for falsy in &["false", "no", "0", "off"] {
            cfg.set("search.no_tui", falsy).unwrap();
            assert!(!cfg.search.no_tui, "expected false for '{}'", falsy);
        }
    }

    #[test]
    fn set_search_format_valid() {
        let mut cfg = default_config();
        for fmt in &["plain", "json", "csv"] {
            cfg.set("search.format", fmt).unwrap();
            assert_eq!(cfg.search.format.as_deref(), Some(*fmt));
        }
    }

    #[test]
    fn set_search_format_invalid_errors() {
        let mut cfg = default_config();
        assert!(cfg.set("search.format", "xml").is_err());
    }

    #[test]
    fn set_exclude_tests() {
        let mut cfg = default_config();
        cfg.set("search.exclude_tests", "true").unwrap();
        assert!(cfg.search.exclude_tests);
    }

    #[test]
    fn set_index_auto_index() {
        let mut cfg = default_config();
        cfg.set("index.auto_index", "true").unwrap();
        assert!(cfg.index.auto_index);
    }

    #[test]
    fn set_editor_command() {
        let mut cfg = default_config();
        cfg.set("editor.command", "nvim").unwrap();
        assert_eq!(cfg.editor.command.as_deref(), Some("nvim"));
        assert_eq!(cfg.get("editor.command").unwrap(), "nvim");
    }

    #[test]
    fn set_editor_command_auto_clears() {
        let mut cfg = default_config();
        cfg.set("editor.command", "nvim").unwrap();
        cfg.set("editor.command", "auto").unwrap();
        assert!(cfg.editor.command.is_none());
    }

    #[test]
    fn set_editor_command_empty_clears() {
        let mut cfg = default_config();
        cfg.set("editor.command", "code").unwrap();
        cfg.set("editor.command", "").unwrap();
        assert!(cfg.editor.command.is_none());
    }

    #[test]
    fn set_unknown_key_errors() {
        let mut cfg = default_config();
        assert!(cfg.set("nonexistent.key", "value").is_err());
    }
}
