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
        Editor::VSCode(bin) => {
            // VS Code / Cursor: non-blocking GUI app.
            // On macOS, `code`/`cursor` may not be in PATH even when the app is installed.
            // vscode_binary() falls back to the well-known app bundle path.
            let cmd = vscode_binary(&bin);
            std::process::Command::new(&cmd)
                .args(["--goto", &format!("{abs_str}:{line}")])
                .spawn()
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to launch {bin}: {e}\n  Tip: install the CLI via the app command palette → \"Install 'code' command in PATH\""))?;
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
    let candidates = &["code.cmd", "code", "nvim", "notepad"][..];
    #[cfg(not(windows))]
    let candidates = &["code", "cursor", "zed", "nvim", "vim", "hx", "nano", "emacs"][..];
    for cmd in candidates {
        if cmd_in_path(cmd) {
            return Some(cmd.to_string());
        }
    }
    None
}

// ─── Detection ────────────────────────────────────────────────────────────────

enum Editor {
    VSCode(String), // stores the command basename ("code" or "cursor")
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

    // Auto-detect: prefer VS Code / GUI editors first so results open in the
    // editor the user is most likely already working in.
    // On Windows, VS Code registers as `code.cmd`; check that too.
    #[cfg(windows)]
    let candidates = &["code.cmd", "code", "nvim", "notepad"][..];
    #[cfg(not(windows))]
    let candidates = &["code", "cursor", "zed", "nvim", "vim", "hx", "nano", "emacs"][..];

    for cmd in candidates {
        if cmd_in_path(cmd) {
            return classify(cmd.to_string());
        }
    }

    // macOS: VS Code/Cursor may be installed without the CLI in PATH.
    #[cfg(target_os = "macos")]
    for (app, label) in &[
        ("Visual Studio Code", "code"),
        ("Cursor", "cursor"),
    ] {
        let bundle = format!("/Applications/{app}.app/Contents/Resources/app/bin/code");
        if std::path::Path::new(&bundle).exists() {
            return classify(label.to_string());
        }
        if let Some(home) = std::env::var_os("HOME") {
            let user_bundle = format!(
                "{}/Applications/{app}.app/Contents/Resources/app/bin/code",
                home.to_string_lossy()
            );
            if std::path::Path::new(&user_bundle).exists() {
                return classify(label.to_string());
            }
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

    if base == "code" || base == "code.cmd" || base.starts_with("code-") {
        Editor::VSCode("code".to_string())
    } else if base == "cursor" {
        Editor::VSCode("cursor".to_string())
    } else if base == "zed" {
        Editor::Zed
    } else if base == "hx" || base.contains("helix") {
        Editor::Helix
    } else {
        Editor::Terminal(cmd)
    }
}

/// Return the best available path for a VS Code–family binary.
///
/// Priority:
///   1. `cmd` is already in PATH  →  use it as-is
///   2. macOS app-bundle CLI      →  full path inside .app
///   3. fall back to bare `cmd`   →  will fail at spawn time with a clear message
fn vscode_binary(cmd: &str) -> String {
    if cmd_in_path(cmd) {
        return cmd.to_string();
    }
    #[cfg(target_os = "macos")]
    {
        let (app_name, bin_name) = if cmd == "cursor" {
            ("Cursor", "cursor")
        } else {
            ("Visual Studio Code", "code")
        };
        for base in &["/Applications", &format!("{}/Applications", std::env::var("HOME").unwrap_or_default())] {
            let bundle = format!("{base}/{app_name}.app/Contents/Resources/app/bin/{bin_name}");
            if std::path::Path::new(&bundle).exists() {
                return bundle;
            }
        }
    }
    cmd.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify ───────────────────────────────────────────────────────────────

    #[test]
    fn classify_code_is_vscode_code() {
        assert!(matches!(classify("code".to_string()), Editor::VSCode(b) if b == "code"));
    }

    #[test]
    fn classify_code_cmd_is_vscode_code() {
        assert!(matches!(classify("code.cmd".to_string()), Editor::VSCode(b) if b == "code"));
    }

    #[test]
    fn classify_cursor_is_vscode_cursor() {
        assert!(matches!(classify("cursor".to_string()), Editor::VSCode(b) if b == "cursor"));
    }

    #[test]
    fn classify_zed_is_zed() {
        assert!(matches!(classify("zed".to_string()), Editor::Zed));
    }

    #[test]
    fn classify_hx_is_helix() {
        assert!(matches!(classify("hx".to_string()), Editor::Helix));
    }

    #[test]
    fn classify_helix_name_is_helix() {
        assert!(matches!(classify("helix".to_string()), Editor::Helix));
    }

    #[test]
    fn classify_nvim_is_terminal() {
        assert!(matches!(classify("nvim".to_string()), Editor::Terminal(cmd) if cmd == "nvim"));
    }

    #[test]
    fn classify_vim_is_terminal() {
        assert!(matches!(classify("vim".to_string()), Editor::Terminal(cmd) if cmd == "vim"));
    }

    #[test]
    fn classify_nano_is_terminal() {
        assert!(matches!(classify("nano".to_string()), Editor::Terminal(cmd) if cmd == "nano"));
    }

    #[test]
    fn classify_full_bundle_path_cursor_is_vscode_cursor() {
        // A full path to the Cursor bundle binary should classify as VSCode("cursor").
        let path = "/Applications/Cursor.app/Contents/Resources/app/bin/cursor".to_string();
        assert!(matches!(classify(path), Editor::VSCode(b) if b == "cursor"));
    }

    #[test]
    fn classify_full_bundle_path_code_is_vscode_code() {
        let path =
            "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code".to_string();
        assert!(matches!(classify(path), Editor::VSCode(b) if b == "code"));
    }

    // ── vscode_binary ──────────────────────────────────────────────────────────

    #[test]
    fn vscode_binary_falls_back_to_cmd_when_not_found() {
        // A nonsense command will never be in PATH or any bundle path.
        let result = vscode_binary("scout-fake-editor-xyz-99999");
        assert_eq!(result, "scout-fake-editor-xyz-99999");
    }

    #[test]
    fn vscode_binary_returns_cmd_when_in_path() {
        // `true` is universally available on Unix systems.
        // We test that when the command IS in PATH it is returned as-is (not a bundle path).
        #[cfg(unix)]
        {
            let result = vscode_binary("true");
            assert_eq!(result, "true", "command in PATH should be returned unchanged");
        }
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
