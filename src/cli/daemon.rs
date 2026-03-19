use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::index;
use crate::storage::lock;

// ─── PID file ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct DaemonState {
    pid: u32,
    started_at: i64,
    root: String,
    last_updated: Option<i64>,
}

fn pid_path(idx_dir: &std::path::Path) -> std::path::PathBuf {
    idx_dir.join("daemon.pid")
}

fn read_state(idx_dir: &std::path::Path) -> Option<DaemonState> {
    let bytes = std::fs::read(pid_path(idx_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_state(idx_dir: &std::path::Path, state: &DaemonState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(pid_path(idx_dir), json).context("failed to write daemon.pid")
}

fn remove_state(idx_dir: &std::path::Path) {
    let _ = std::fs::remove_file(pid_path(idx_dir));
}

/// Check if a process with `pid` is currently running.
fn is_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) succeeds if the process exists and we can signal it.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

// ─── Start ────────────────────────────────────────────────────────────────────

pub struct StartArgs {
    pub path: PathBuf,
}

pub fn start(args: StartArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;

    // Check if already running.
    if let Some(state) = read_state(&idx_dir) {
        if is_running(state.pid) {
            bail!(
                "Daemon is already running (PID {}). Use `scout daemon stop` first.",
                state.pid
            );
        }
        // Stale PID file — clean it up.
        remove_state(&idx_dir);
    }

    // Ensure there's an index to watch.
    if !index::db_path(&idx_dir).exists() {
        bail!(
            "No index found at {}. Run `scout index` first.",
            root.display()
        );
    }

    // Spawn the daemon process (re-invokes this binary with the hidden `daemon run` subcommand).
    // On Unix, call setsid() in the child before exec so it detaches from the controlling
    // terminal and survives after the parent (and the user's shell session) exits.
    let exe = std::env::current_exe().context("failed to find current executable")?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("daemon")
        .arg("run")
        .arg("--path")
        .arg(&root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let child = cmd.spawn().context("failed to spawn daemon process")?;

    let pid = child.id();
    let state = DaemonState {
        pid,
        started_at: chrono::Utc::now().timestamp(),
        root: root.to_string_lossy().to_string(),
        last_updated: None,
    };
    write_state(&idx_dir, &state)?;

    println!("Daemon started (PID {pid}), watching {}", root.display());
    Ok(())
}

// ─── Stop ─────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct StopArgs {
    pub path: PathBuf,
}

pub fn stop(args: StopArgs) -> Result<()> {
    #[cfg(not(unix))]
    {
        let _ = args;
        bail!("Stopping the daemon is not supported on this platform.");
    }

    #[cfg(unix)]
    {
        let root = args.path.canonicalize().context("path not found")?;
        let idx_dir = index::index_dir(&root)?;
        let state = read_state(&idx_dir)
            .filter(|s| is_running(s.pid))
            .ok_or_else(|| anyhow::anyhow!("No daemon is running for this repository."))?;
        unsafe {
            libc::kill(state.pid as libc::pid_t, libc::SIGTERM);
        }
        remove_state(&idx_dir);
        println!("Daemon stopped (PID {}).", state.pid);
        Ok(())
    }
}

// ─── Status ───────────────────────────────────────────────────────────────────

pub struct StatusArgs {
    pub path: PathBuf,
}

pub fn status(args: StatusArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;

    match read_state(&idx_dir) {
        Some(state) if is_running(state.pid) => {
            let uptime = chrono::Utc::now().timestamp() - state.started_at;
            let uptime_str = format_duration(uptime);
            let last = state
                .last_updated
                .map(|t| {
                    let secs = chrono::Utc::now().timestamp() - t;
                    format!("{} ago", format_duration(secs))
                })
                .unwrap_or_else(|| "never".to_string());
            println!("Daemon running  PID {}  uptime {}  last update: {}",
                state.pid, uptime_str, last);
        }
        _ => {
            println!("Daemon is not running.");
        }
    }
    Ok(())
}

// ─── Run (hidden — invoked by the spawned daemon process) ─────────────────────

pub struct RunArgs {
    pub path: PathBuf,
}

pub fn run(args: RunArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    crate::watch::daemon::run(root)
}

// ─── Install hooks ────────────────────────────────────────────────────────────

pub struct InstallHooksArgs {
    pub path: PathBuf,
}

pub fn install_hooks(args: InstallHooksArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let hooks_dir = root.join(".git").join("hooks");

    if !hooks_dir.exists() {
        bail!(
            "No .git/hooks directory found at {}. Is this a git repository?",
            root.display()
        );
    }

    let exe = std::env::current_exe().context("failed to find current executable")?;
    let exe_str = exe.to_string_lossy();

    for hook in &["post-commit", "post-merge", "post-checkout"] {
        let hook_path = hooks_dir.join(hook);
        let script = format!(
            "#!/bin/sh\n# Added by scout\n\"{exe_str}\" update --path \"{root}\" &\n",
            root = root.display()
        );

        // Append to existing hook or create new.
        if hook_path.exists() {
            let existing = std::fs::read_to_string(&hook_path)?;
            if !existing.contains("scout") {
                let mut content = existing;
                content.push_str(&format!(
                    "\n# scout incremental update\n\"{exe_str}\" update --path \"{root}\" &\n",
                    root = root.display()
                ));
                std::fs::write(&hook_path, content)?;
            }
        } else {
            std::fs::write(&hook_path, &script)?;
        }

        // Make executable on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&hook_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&hook_path, perms)?;
        }

        println!("Installed hook: {}", hook_path.display());
    }

    Ok(())
}

// ─── Update (batch) ───────────────────────────────────────────────────────────

pub struct UpdateArgs {
    pub path: PathBuf,
}

pub fn update(args: UpdateArgs) -> Result<()> {
    let root = args.path.canonicalize().context("path not found")?;
    let idx_dir = index::index_dir(&root)?;
    let db_path = index::db_path(&idx_dir);

    if !db_path.exists() {
        bail!(
            "No index found at {}. Run `scout index` first.",
            root.display()
        );
    }

    let _lock = lock::IndexLock::acquire_exclusive(&idx_dir)?;
    let conn = crate::storage::sqlite::open(&db_path)?;
    let tantivy_dir = idx_dir.join("tantivy");
    let (tantivy_index, schema) = crate::storage::tantivy_store::open_index(&tantivy_dir)?;
    let mut writer = tantivy_index.writer(30_000_000)?;

    let count = crate::index::updater::batch_update(&conn, &mut writer, &schema, &root)?;
    writer.commit()?;

    drop(conn);
    let mut meta = index::load_metadata(&idx_dir)?;
    meta.last_updated = chrono::Utc::now().timestamp();
    meta.checksum = crate::storage::backup::compute_db_checksum(&idx_dir)?;
    index::save_metadata(&idx_dir, &meta)?;

    println!("Updated {count} changed file(s).");
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}
