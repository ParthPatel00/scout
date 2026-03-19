use std::path::Path;

use anyhow::{bail, Result};

/// Open `file_path` (relative to `repo_root`) at `line` in the user's editor.
pub fn open(file_path: &str, line: usize, repo_root: &Path) -> Result<()> {
    let abs = repo_root.join(file_path);
    let abs_str = abs.to_string_lossy().to_string();

    match detect() {
        Editor::VSCode => {
            // VS Code / Cursor: non-blocking GUI app.
            std::process::Command::new("code")
                .args(["--goto", &format!("{abs_str}:{line}")])
                .spawn()
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to launch VS Code: {e}"))?;
        }
        Editor::Zed => {
            std::process::Command::new("zed")
                .arg(format!("{abs_str}:{line}"))
                .spawn()
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to launch Zed: {e}"))?;
        }
        Editor::Helix => {
            // Helix uses file:line syntax.
            std::process::Command::new("hx")
                .arg(format!("{abs_str}:{line}"))
                .status()
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to launch Helix: {e}"))?;
        }
        Editor::Terminal(cmd) => {
            // Vim, Neovim, nano, emacs, etc. — all support `+LINE file`.
            std::process::Command::new(&cmd)
                .arg(format!("+{line}"))
                .arg(&abs_str)
                .status()
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to launch {cmd}: {e}"))?;
        }
        Editor::None => {
            bail!(
                "No editor found. Set $SCOUT_EDITOR, $VISUAL, or $EDITOR.\n  \
                 Example: export SCOUT_EDITOR=nvim"
            );
        }
    }

    Ok(())
}

// ─── Detection ────────────────────────────────────────────────────────────────

enum Editor {
    VSCode,
    Zed,
    Helix,
    Terminal(String),
    None,
}

fn detect() -> Editor {
    // Priority: SCOUT_EDITOR > VISUAL > EDITOR > auto-detect from PATH.
    for var in &["SCOUT_EDITOR", "VISUAL", "EDITOR"] {
        if let Ok(val) = std::env::var(var) {
            let val = val.trim().to_string();
            if !val.is_empty() {
                return classify(val);
            }
        }
    }

    // Auto-detect: prefer terminal editors so the user stays in their workflow.
    for cmd in &["nvim", "vim", "hx", "nano", "emacs", "code", "zed"] {
        if cmd_in_path(cmd) {
            return classify(cmd.to_string());
        }
    }

    Editor::None
}

/// Map an editor command string to its open strategy.
fn classify(cmd: String) -> Editor {
    let base = Path::new(&cmd)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| cmd.to_lowercase().into());

    if base == "code" || base == "cursor" || base.starts_with("code-") {
        Editor::VSCode
    } else if base == "zed" {
        Editor::Zed
    } else if base == "hx" || base.contains("helix") {
        Editor::Helix
    } else {
        Editor::Terminal(cmd)
    }
}

fn cmd_in_path(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
