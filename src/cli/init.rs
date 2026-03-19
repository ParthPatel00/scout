//! `scout init` — interactive first-run setup wizard.
//!
//! All questions use selection menus (no free-text except repo paths).
//! Every setting can be changed later with `scout config set <key> <value>`.

use std::path::PathBuf;

use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

use crate::config::{config_path, Config};
use crate::ml::model;

pub fn run() -> Result<()> {
    let theme = ColorfulTheme::default();

    println!(
        "\n\x1b[1;33m  Scout — First-time Setup\x1b[0m\n"
    );
    println!("  This wizard sets your preferences. Everything can be");
    println!("  changed later with \x1b[1mscout config set <key> <value>\x1b[0m\n");

    let mut cfg = Config::load();

    // ── 1. Result count ───────────────────────────────────────────────────────
    let limit_options = ["5", "10 (default)", "15", "20", "30", "50"];
    let limit_defaults = [5usize, 10, 15, 20, 30, 50];
    let limit_idx = Select::with_theme(&theme)
        .with_prompt("How many results to show by default?")
        .items(&limit_options)
        .default(1)
        .interact()?;
    cfg.search.limit = limit_defaults[limit_idx];

    // ── 2. Output format ──────────────────────────────────────────────────────
    let formats = [
        "Plain text with TUI when interactive (recommended)",
        "Always plain text (good for scripts / piping)",
        "JSON",
        "CSV",
    ];
    let format_idx = Select::with_theme(&theme)
        .with_prompt("Default output format")
        .items(&formats)
        .default(0)
        .interact()?;
    match format_idx {
        1 => { cfg.search.no_tui = true; cfg.search.format = None; }
        2 => { cfg.search.format = Some("json".to_string()); cfg.search.no_tui = true; }
        3 => { cfg.search.format = Some("csv".to_string()); cfg.search.no_tui = true; }
        _ => { cfg.search.no_tui = false; cfg.search.format = None; }
    }

    // ── 3. Exclude test files ─────────────────────────────────────────────────
    let excl_options = [
        "No  — include test files in results",
        "Yes — hide test files from results",
    ];
    let excl_idx = Select::with_theme(&theme)
        .with_prompt("Show test files in search results?")
        .items(&excl_options)
        .default(0)
        .interact()?;
    cfg.search.exclude_tests = excl_idx == 1;

    // ── 4. Keep index fresh ───────────────────────────────────────────────────
    let fresh_opts = [
        "No  — I'll re-run `scout index` when I want to update",
        "Yes — via daemon  (background process, watches file changes)",
        "Yes — via git hooks  (re-indexes on commit / merge / checkout)",
    ];
    let fresh_idx = Select::with_theme(&theme)
        .with_prompt("Keep the index fresh automatically?")
        .items(&fresh_opts)
        .default(0)
        .interact()?;

    // ── 5. Semantic (AI) search ───────────────────────────────────────────────
    println!();
    println!("  \x1b[1mSemantic search\x1b[0m uses a local AI model (~350 MB, one-time download)");
    println!("  to find code by concept, not just keywords. Without it, Scout uses");
    println!("  BM25 + name-match — fast and accurate, but keyword-only.\n");

    let sem_opts = [
        "Yes — download the model now  (~350 MB)",
        "Yes — I'll download it later with `scout index --download-model`",
        "No  — keyword search is fine for now",
    ];
    let sem_idx = Select::with_theme(&theme)
        .with_prompt("Enable AI-powered semantic search?")
        .items(&sem_opts)
        .default(0)
        .interact()?;

    // ── 6. Editor ─────────────────────────────────────────────────────────────
    let detected = crate::editor::detect_name();
    let editor_choices = build_editor_choices(&detected);
    let editor_items: Vec<&str> = editor_choices.iter().map(|s| s.as_str()).collect();
    let editor_idx = Select::with_theme(&theme)
        .with_prompt("Editor for opening results")
        .items(&editor_items)
        .default(0)
        .interact()?;

    cfg.editor.command = match editor_idx {
        0 => detected.clone(), // auto-detect
        n if n == editor_items.len() - 1 => {
            // "Other — enter path" — only option that takes text
            let custom: String = Input::with_theme(&theme)
                .with_prompt("Editor command")
                .interact_text()?;
            if custom.is_empty() { None } else { Some(custom) }
        }
        _ => {
            let name = editor_items[editor_idx];
            // Strip the description suffix after spaces
            Some(name.split_whitespace().next().unwrap_or(name).to_string())
        }
    };

    // ── 7. Shell completions ──────────────────────────────────────────────────
    let shell_opts = ["Skip for now", "Zsh", "Bash", "Fish"];
    let shell_idx = Select::with_theme(&theme)
        .with_prompt("Install shell completions?")
        .items(&shell_opts)
        .default(0)
        .interact()?;

    // ── 8. Additional repos ───────────────────────────────────────────────────
    println!();
    println!("  Cross-repo search lets you search across multiple codebases at once.");
    println!("  \x1b[2m(You can add repos later with `scout repos add <name> <path>`)\x1b[0m\n");
    let add_repos = Confirm::with_theme(&theme)
        .with_prompt("Add other repos for cross-repo search now?")
        .default(false)
        .interact()?;

    let mut extra_repos: Vec<(String, PathBuf)> = vec![];
    if add_repos {
        loop {
            let repo_path: String = Input::with_theme(&theme)
                .with_prompt("Repo path (leave blank to finish)")
                .allow_empty(true)
                .interact_text()?;
            if repo_path.trim().is_empty() {
                break;
            }
            let repo_name: String = Input::with_theme(&theme)
                .with_prompt("Short name for this repo (e.g. backend, frontend)")
                .interact_text()?;
            extra_repos.push((repo_name, PathBuf::from(repo_path.trim())));
        }
    }

    // ── Save config ───────────────────────────────────────────────────────────
    cfg.save()?;
    println!(
        "\n\x1b[32m✓\x1b[0m Config saved to {}\n",
        config_path().display()
    );

    // ── Act on deferred choices ───────────────────────────────────────────────

    // Daemon / git hooks setup — also run an initial index first so there is
    // something for the daemon/hooks to keep fresh.
    if fresh_idx == 1 || fresh_idx == 2 {
        let cwd = std::env::current_dir()?;
        println!("\n  Building the initial index for {} …", cwd.display());
        crate::cli::index::run(crate::cli::index::IndexArgs {
            path: cwd.clone(),
            verbose: false,
        })?;
        match fresh_idx {
            1 => {
                println!("  Starting background daemon …");
                let _ = crate::cli::daemon::start(crate::cli::daemon::StartArgs { path: cwd });
            }
            2 => {
                println!("  Installing git hooks …");
                let _ = crate::cli::daemon::install_hooks(crate::cli::daemon::InstallHooksArgs { path: cwd });
            }
            _ => {}
        }
    }

    // Register extra repos
    for (name, path) in extra_repos {
        println!("  Registering repo '{}' at {} …", name, path.display());
        let _ = crate::cli::repos::add(crate::cli::repos::AddArgs { name, path });
    }

    // Shell completions instructions
    if shell_idx > 0 {
        let shell = ["", "zsh", "bash", "fish"][shell_idx];
        print_completion_instructions(shell);
    }

    // Semantic model download
    match sem_idx {
        0 => {
            println!();
            if model::is_model_downloaded() {
                println!("\x1b[32m✓\x1b[0m Model already downloaded.");
            } else {
                model::print_download_instructions();
            }
        }
        1 => {
            println!(
                "\n  When ready: \x1b[1mscout index --download-model\x1b[0m"
            );
        }
        _ => {}
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    println!("\n\x1b[1mAll set!\x1b[0m  Next steps:\n");
    println!("  1. Index your codebase:    \x1b[1mscout index\x1b[0m");
    println!("  2. Search it:              \x1b[1mscout \"authentication logic\"\x1b[0m");
    println!("  3. View / change settings: \x1b[1mscout config list\x1b[0m\n");

    Ok(())
}

fn build_editor_choices(detected: &Option<String>) -> Vec<String> {
    let det_label = detected
        .as_deref()
        .map(|d| format!("Auto-detect  (currently: {})", d))
        .unwrap_or_else(|| "Auto-detect  (none found — set $SCOUT_EDITOR)".to_string());

    vec![
        det_label,
        "code   — VS Code".to_string(),
        "cursor — Cursor".to_string(),
        "zed    — Zed".to_string(),
        "nvim   — Neovim".to_string(),
        "vim    — Vim".to_string(),
        "hx     — Helix".to_string(),
        "nano   — Nano".to_string(),
        "Other  — enter path".to_string(),
    ]
}

fn print_completion_instructions(shell: &str) {
    println!("\n  Shell completions ({shell}):");
    match shell {
        "zsh" => println!(
            "    scout completions zsh > ~/.zsh/completions/_scout\n\n    Then add to ~/.zshrc:\n\n    fpath=(~/.zsh/completions $fpath)\n    autoload -Uz compinit && compinit"
        ),
        "bash" => println!(
            "    scout completions bash > ~/.bash_completions/scout\n\n    Then add to ~/.bashrc:\n\n    source ~/.bash_completions/scout"
        ),
        "fish" => println!(
            "    scout completions fish > ~/.config/fish/completions/scout.fish"
        ),
        _ => {}
    }
    println!();
}
