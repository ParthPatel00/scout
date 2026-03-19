mod cli;
mod editor;
mod index;
mod ml;
mod repo;
mod search;
mod storage;
mod tui;
mod types;
mod watch;

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::cli::OutputFormat;
use crate::search::SearchFilter;

/// Known subcommand names — anything else is treated as a search query.
const SUBCOMMANDS: &[&str] = &[
    "index", "search", "s", "repos", "report", "rebuild", "optimize",
    "cleanup", "daemon", "help", "--help", "-h", "--version", "-V",
];

#[derive(Parser)]
#[command(
    name = "scout",
    about = "Code search for your codebase.\n\n  scout \"authentication with stripe\"\n  scout index",
    version,
    // Don't show subcommand list in the short help — keep it minimal.
    subcommand_help_heading = "Commands",
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build or update the search index.
    Index {
        /// Directory to index [default: current directory].
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Show each file as it is parsed.
        #[arg(short, long)]
        verbose: bool,

        /// Print instructions for downloading the AI embedding model.
        #[arg(long)]
        download_model: bool,
    },

    /// Search the index. Launches TUI in a terminal, plain text when piped.
    #[command(alias = "s")]
    Search {
        /// What to search for.
        query: String,

        /// Repository root to search [default: current directory].
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Max results to show.
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Filter by language: python, rust, go, java, typescript, javascript, cpp.
        #[arg(long)]
        lang: Option<String>,

        /// Only show results from files whose path contains this string.
        #[arg(long)]
        path_filter: Option<String>,

        /// Only show results from files indexed in the last N days.
        #[arg(long)]
        modified_last: Option<u64>,

        /// Exclude test files.
        #[arg(long)]
        exclude_tests: bool,

        /// Show callers and callees of each result.
        #[arg(long)]
        show_context: bool,

        /// Output format: plain, json, csv.
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,

        /// Plain-text output only, never launch the TUI.
        #[arg(long)]
        no_tui: bool,

        /// Force pure vector-only search (requires model). Default already uses
        /// the best available method automatically.
        #[arg(long)]
        semantic: bool,

        /// Deprecated: hybrid search is now the default. Accepted but ignored.
        #[arg(long, hide = true)]
        best: bool,

        /// Search across all registered repos.
        #[arg(long)]
        all_repos: bool,

        /// Search specific registered repos (comma-separated, e.g. backend,frontend).
        #[arg(long)]
        repos: Option<String>,

        /// Find functions similar to the one at FILE:LINE.
        #[arg(long, value_name = "FILE:LINE")]
        find_similar: Option<String>,
    },

    /// Manage registered repositories for cross-repo search.
    Repos {
        #[command(subcommand)]
        action: ReposCommand,
    },

    /// Generate codebase reports.
    Report {
        #[command(subcommand)]
        kind: ReportCommand,
    },

    /// Wipe the index and regenerate from scratch.
    Rebuild {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        verbose: bool,
    },

    /// Compact the database and remove orphaned data.
    Optimize {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Remove index entries for files deleted from disk.
    Cleanup {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Background daemon and git hooks management.
    Daemon {
        #[command(subcommand)]
        action: DaemonCommand,
    },
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Start the background file-watching daemon.
    Start {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Stop the running daemon.
    Stop {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Show daemon status.
    Status {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    #[command(hide = true)]
    Run {
        #[arg(short, long)]
        path: PathBuf,
    },
    /// Install post-commit/merge/checkout git hooks.
    InstallHooks {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Batch-update the index for all changed files (no daemon required).
    Update {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ReportCommand {
    /// List functions with no callers in the index.
    UnusedFunctions {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ReposCommand {
    /// Register a repository for cross-repo search.
    Add {
        /// Short name for this repo (e.g. backend, frontend).
        name: String,
        /// Path to the repository root.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// List all registered repositories.
    List,
    /// Unregister a repository.
    Remove {
        /// Name of the repo to remove.
        name: String,
    },
}

fn main() -> Result<()> {
    // Allow `scout "query"` as a shorthand for `scout search "query"`.
    // If the first argument is not a known subcommand or flag, inject "search".
    let raw: Vec<String> = std::env::args().collect();
    let args: Vec<String> = if raw.len() >= 2
        && !SUBCOMMANDS.contains(&raw[1].as_str())
        && !raw[1].starts_with('-')
    {
        let mut a = vec![raw[0].clone(), "search".to_string()];
        a.extend_from_slice(&raw[1..]);
        a
    } else {
        raw
    };

    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Index { path, verbose, download_model } => {
            if download_model {
                ml::model::print_download_instructions();
                return Ok(());
            }
            cli::index::run(cli::index::IndexArgs { path, verbose })?;
        }
        Command::Search {
            query,
            path,
            limit,
            lang,
            path_filter,
            modified_last,
            exclude_tests,
            show_context,
            format,
            no_tui,
            semantic,
            best,
            all_repos,
            repos,
            find_similar,
        } => {
            let modified_since = modified_last
                .map(|days| chrono::Utc::now().timestamp() - (days as i64 * 86_400));
            let filter = SearchFilter {
                lang,
                path_prefix: path_filter,
                modified_since,
                exclude_tests,
            };

            let use_tui = !no_tui
                && format.is_none()
                && !show_context
                && find_similar.is_none()
                && !all_repos
                && repos.is_none()
                && std::io::stdout().is_terminal();

            let _ = best; // accepted for backwards compatibility, no longer needed
            cli::search::run(cli::search::SearchArgs {
                path,
                query,
                limit,
                filter,
                show_context,
                format,
                use_tui,
                semantic,
                all_repos,
                repos,
                find_similar,
            })?;
        }
        Command::Repos { action } => match action {
            ReposCommand::Add { name, path } => {
                cli::repos::add(cli::repos::AddArgs { name, path })?;
            }
            ReposCommand::List => {
                cli::repos::list(cli::repos::ListArgs)?;
            }
            ReposCommand::Remove { name } => {
                cli::repos::remove(cli::repos::RemoveArgs { name })?;
            }
        },
        Command::Report { kind } => match kind {
            ReportCommand::UnusedFunctions { path } => {
                cli::report::run(cli::report::ReportArgs {
                    path,
                    kind: cli::report::ReportKind::UnusedFunctions,
                })?;
            }
        },
        Command::Rebuild { path, verbose } => {
            cli::maintenance::rebuild(cli::maintenance::RebuildArgs { path, verbose })?;
        }
        Command::Optimize { path } => {
            cli::maintenance::optimize(cli::maintenance::OptimizeArgs { path })?;
        }
        Command::Cleanup { path } => {
            cli::maintenance::cleanup(cli::maintenance::CleanupArgs { path })?;
        }
        Command::Daemon { action } => match action {
            DaemonCommand::Start { path } => {
                cli::daemon::start(cli::daemon::StartArgs { path })?;
            }
            DaemonCommand::Stop { path } => {
                cli::daemon::stop(cli::daemon::StopArgs { path })?;
            }
            DaemonCommand::Status { path } => {
                cli::daemon::status(cli::daemon::StatusArgs { path })?;
            }
            DaemonCommand::Run { path } => {
                cli::daemon::run(cli::daemon::RunArgs { path })?;
            }
            DaemonCommand::InstallHooks { path } => {
                cli::daemon::install_hooks(cli::daemon::InstallHooksArgs { path })?;
            }
            DaemonCommand::Update { path } => {
                cli::daemon::update(cli::daemon::UpdateArgs { path })?;
            }
        },
    }

    Ok(())
}
