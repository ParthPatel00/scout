use std::path::Path;

use anyhow::{bail, Result};

/// Open `file_path` (relative to `repo_root`) at `line` in the user's editor.
///
/// If `override_cmd` is set (from `editor.command` in config) it takes
/// precedence over env-var and PATH detection.
#[allow(dead_code)]
pub fn open(file_path: &str, line: usize, repo_root: &Path) -> Result<()> {
    open_with(file_path, line, repo_root, None)
}

pub fn open_with(
    file_path: &str,
    line: usize,
    repo_root: &Path,
    override_cmd: Option<&str>,
) -> Result<()> {
    let abs = repo_root.join(file_path);
    let abs_str = abs.to_string_lossy().to_string();

    let editor = if let Some(cmd) = override_cmd {
        classify(cmd.to_string())
    } else {
        detect()
    };

    match editor {
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

/// Return the name of the detected editor without opening anything.
/// Used by `scout init` to show the user what was auto-detected.
pub fn detect_name() -> Option<String> {
    for var in &["SCOUT_EDITOR", "VISUAL", "EDITOR"] {
        if let Ok(val) = std::env::var(var) {
            let val = val.trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    #[cfg(windows)]
    let candidates = &["nvim", "code.cmd", "code", "notepad"][..];
    #[cfg(not(windows))]
    let candidates = &["nvim", "vim", "hx", "nano", "emacs", "code", "zed"][..];
    for cmd in candidates {
        if cmd_in_path(cmd) {
            return Some(cmd.to_string());
        }
    }
    None
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
    // On Windows, VS Code registers as `code.cmd`; check that too.
    #[cfg(windows)]
    let candidates = &["nvim", "code.cmd", "code", "notepad"][..];
    #[cfg(not(windows))]
    let candidates = &["nvim", "vim", "hx", "nano", "emacs", "code", "zed"][..];

    for cmd in candidates {
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
        .unwrap_or_else(|| cmd.to_lowercase());

    if base == "code" || base == "code.cmd" || base == "cursor" || base.starts_with("code-") {
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
    // Walk PATH manually — avoids spawning `which`/`where` and works everywhere.
    let path_var = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    std::env::split_paths(&path_var).any(|dir| {
        // On Windows executables have extensions; check .exe and .cmd too.
        if dir.join(cmd).exists() {
            return true;
        }
        #[cfg(windows)]
        {
            if dir.join(format!("{cmd}.exe")).exists()
                || dir.join(format!("{cmd}.cmd")).exists()
                || dir.join(format!("{cmd}.bat")).exists()
            {
                return true;
            }
        }
        false
    })
}
