mod cli;
mod index;
mod ml;
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

#[derive(Parser)]
#[command(
    name = "codesearch",
    about = "Semantic code search for your codebase",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build or update the search index for the current repository.
    Index {
        /// Root directory to index (defaults to current directory).
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Show each file as it is parsed.
        #[arg(short, long)]
        verbose: bool,

        /// Print instructions for downloading the UniXcoder embedding model.
        #[arg(long)]
        download_model: bool,
    },

    /// Search the index (BM25 + RRF re-ranking). Launches TUI when in a terminal.
    #[command(alias = "s")]
    Search {
        /// The search query.
        query: String,

        /// Root directory of the repository to search.
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Maximum number of results to display.
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Filter by language (e.g. python, rust, go, java, typescript).
        #[arg(long)]
        lang: Option<String>,

        /// Filter to files whose path contains this string.
        #[arg(long)]
        path_filter: Option<String>,

        /// Only show results from files indexed in the last N days (e.g. 7).
        #[arg(long)]
        modified_last: Option<u64>,

        /// Exclude test files from results.
        #[arg(long)]
        exclude_tests: bool,

        /// Show callers and callees of each result.
        #[arg(long)]
        show_context: bool,

        /// Output format (plain, json, csv). Defaults to plain when piped, TUI when in a terminal.
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,

        /// Always use plain-text output, never launch the TUI.
        #[arg(long)]
        no_tui: bool,

        /// Use semantic (vector embedding) search only.
        #[arg(long)]
        semantic: bool,

        /// Hybrid mode: BM25 + vector + name-match (highest quality, requires model).
        #[arg(long)]
        best: bool,
    },

    /// Generate reports about the codebase.
    Report {
        /// Root directory of the repository.
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        #[command(subcommand)]
        kind: ReportCommand,
    },

    /// Wipe the index and regenerate it from scratch.
    Rebuild {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        verbose: bool,
    },

    /// Compact the database, refresh statistics, and remove orphaned data.
    Optimize {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Remove index entries for files that have been deleted from disk.
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
    /// Show daemon status (PID, uptime, last update).
    Status {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// (Internal) Run the daemon event loop in the foreground.
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
    /// List functions and methods that have no callers in the index.
    UnusedFunctions,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

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
        } => {
            let modified_since = modified_last
                .map(|days| chrono::Utc::now().timestamp() - (days as i64 * 86_400));
            let filter = SearchFilter {
                lang,
                path_prefix: path_filter,
                modified_since,
                exclude_tests,
            };

            // Decide output mode: TUI when stdout is a terminal and no override flags.
            let use_tui = !no_tui
                && format.is_none()
                && !show_context
                && std::io::stdout().is_terminal();

            cli::search::run(cli::search::SearchArgs {
                path,
                query,
                limit,
                filter,
                show_context,
                format,
                use_tui,
                semantic,
                best,
            })?;
        }
        Command::Report { path, kind } => {
            let report_kind = match kind {
                ReportCommand::UnusedFunctions => cli::report::ReportKind::UnusedFunctions,
            };
            cli::report::run(cli::report::ReportArgs { path, kind: report_kind })?;
        }
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
