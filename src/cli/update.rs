//! `scout update` — self-update to the latest GitHub release.

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES_API: &str =
    "https://api.github.com/repos/ParthPatel00/scout/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn run() -> Result<()> {
    println!("Checking for updates...");

    let release = fetch_latest().context("failed to reach GitHub releases API")?;
    let latest = release.tag_name.trim_start_matches('v');

    if latest == CURRENT_VERSION {
        println!("Already up to date (v{CURRENT_VERSION}).");
        return Ok(());
    }

    // Simple semver comparison: split on '.' and compare numeric parts.
    if !is_newer(latest, CURRENT_VERSION) {
        println!("Already up to date (v{CURRENT_VERSION}).");
        return Ok(());
    }

    println!("Update available: v{CURRENT_VERSION} → v{latest}");

    let target = current_target();
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let archive_name = format!("scout-{target}.{ext}");

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == archive_name)
        .with_context(|| format!("No binary found for platform '{target}' in this release.\nExpected asset: {archive_name}"))?;

    println!("Downloading {}...", asset.name);
    let bytes = download(&asset.browser_download_url)
        .context("download failed")?;

    let binary = if cfg!(windows) {
        extract_zip(&bytes).context("failed to extract zip")?
    } else {
        extract_targz(&bytes).context("failed to extract tar.gz")?
    };

    install(binary).context("failed to replace binary")?;

    println!("Updated to v{latest}. Restart scout if you have it running.");
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn fetch_latest() -> Result<Release> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("scout/{CURRENT_VERSION}"))
        .build()?;
    let release = client
        .get(RELEASES_API)
        .send()?
        .error_for_status()?
        .json::<Release>()?;
    Ok(release)
}

fn download(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("scout/{CURRENT_VERSION}"))
        .build()?;
    let bytes = client.get(url).send()?.error_for_status()?.bytes()?;
    Ok(bytes.to_vec())
}

fn extract_targz(bytes: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    use std::io::Read;

    let gz = GzDecoder::new(bytes);
    let mut archive = Archive::new(gz);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name == "scout" {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    bail!("'scout' binary not found inside archive");
}

fn extract_zip(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::{Cursor, Read};
    use zip::ZipArchive;

    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if name == "scout.exe" || name == "scout" {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    bail!("'scout.exe' not found inside zip archive");
}

fn install(binary: Vec<u8>) -> Result<()> {
    let exe = std::env::current_exe().context("could not determine current executable path")?;

    // Stage the new binary in the system temp dir so we can always write it,
    // even when the directory containing the current binary is read-only.
    let tmp = std::env::temp_dir().join("scout.update-tmp");

    std::fs::write(&tmp, &binary).context("failed to write temporary binary to temp dir")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .context("failed to set permissions on temporary binary")?;
    }

    // Try atomic rename first (works when tmp and exe are on the same filesystem).
    // Fall back to copy+delete when they are on different filesystems (cross-device).
    let replace_err = std::fs::rename(&tmp, &exe).err();
    if replace_err.is_some() {
        std::fs::copy(&tmp, &exe).with_context(|| format!(
            "failed to replace binary at {} — try running with sudo or move scout to a writable location",
            exe.display()
        ))?;
        let _ = std::fs::remove_file(&tmp);
    }

    Ok(())
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64) {
        let mut parts = s.split('.');
        let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        (major, minor, patch)
    };
    parse(candidate) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_newer ───────────────────────────────────────────────────────────────

    #[test]
    fn newer_patch_version() {
        assert!(is_newer("1.0.1", "1.0.0"));
    }

    #[test]
    fn newer_minor_version() {
        assert!(is_newer("1.1.0", "1.0.9"));
    }

    #[test]
    fn newer_major_version() {
        assert!(is_newer("2.0.0", "1.9.9"));
    }

    #[test]
    fn same_version_is_not_newer() {
        assert!(!is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn older_patch_is_not_newer() {
        assert!(!is_newer("1.0.0", "1.0.1"));
    }

    #[test]
    fn older_minor_is_not_newer() {
        assert!(!is_newer("1.0.5", "1.1.0"));
    }

    #[test]
    fn older_major_is_not_newer() {
        assert!(!is_newer("1.9.9", "2.0.0"));
    }

    #[test]
    fn is_newer_handles_missing_parts() {
        // Partial versions should degrade gracefully (missing parts treated as 0).
        assert!(is_newer("1.1", "1.0.0")); // "1.1.0" > "1.0.0"
        assert!(!is_newer("1.0", "1.0.1")); // "1.0.0" < "1.0.1"
    }

    #[test]
    fn is_newer_handles_invalid_string_as_zero() {
        // Non-numeric parts parse to 0 — should not panic.
        assert!(!is_newer("abc", "1.0.0")); // 0.0.0 < 1.0.0
    }

    #[test]
    fn is_newer_large_numbers() {
        assert!(is_newer("10.0.0", "9.99.99"));
    }

    // ── current_target ────────────────────────────────────────────────────────

    #[test]
    fn current_target_is_non_empty() {
        let t = current_target();
        assert!(!t.is_empty());
    }

    #[test]
    fn current_target_is_known_platform() {
        let known = &[
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "unknown",
        ];
        assert!(
            known.contains(&current_target()),
            "unexpected target: {}",
            current_target()
        );
    }
}

fn current_target() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else {
        "unknown"
    }
}
