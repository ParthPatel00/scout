mod cli;
mod config;
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
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::cli::OutputFormat;
use crate::config::Config;
use crate::search::SearchFilter;

/// Known subcommand names — anything else is treated as a search query.
const SUBCOMMANDS: &[&str] = &[
    "index", "search", "s", "repos", "report", "rebuild", "optimize",
    "cleanup", "daemon", "config", "init", "completions", "update", "stats",
    "help", "--help", "-h", "--version", "-V",
];

#[derive(Parser)]
#[command(
    name = "scout",
    about = "Semantic code search for your codebase.",
    long_about = "Scout indexes your code with BM25 + AI embeddings and lets you search\n\
                  by concept, not just keywords. Works offline, stays fast.\n\
                  \n\
                  Quick start:\n\
                  \n\
                  \x1b[1m  scout init\x1b[0m                        # one-time setup wizard\n\
                  \x1b[1m  scout index\x1b[0m                        # index the current repo\n\
                  \x1b[1m  scout \"authentication logic\"\x1b[0m       # search\n\
                  \x1b[1m  scout config list\x1b[0m                  # view / change settings",
    version,
    subcommand_help_heading = "Commands",
    after_help = "Run \x1b[1mscout init\x1b[0m for interactive first-time setup.\n\
                  Run \x1b[1mscout config list\x1b[0m to view persistent settings.",
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
    ///
    /// Examples:
    ///   scout "stripe payment flow"
    ///   scout "auth middleware" --lang go --limit 5
    ///   scout "database connection" --format json | jq '.[0].file_path'
    ///   scout "retry logic" --semantic
    #[command(alias = "s")]
    Search {
        /// What to search for (optional when --find-similar is used).
        #[arg(required_unless_present = "find_similar")]
        query: Option<String>,

        /// Repository root to search [default: current directory].
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Max results to show [config: search.limit].
        #[arg(short, long)]
        limit: Option<usize>,

        /// Filter by language: python, rust, go, java, typescript, javascript, cpp.
        #[arg(long)]
        lang: Option<String>,

        /// Only show results from files whose path contains this string.
        #[arg(long)]
        path_filter: Option<String>,

        /// Only show results from files indexed in the last N days.
        #[arg(long)]
        modified_last: Option<u64>,

        /// Exclude test files [config: search.exclude_tests].
        #[arg(long)]
        exclude_tests: bool,

        /// Show callers and callees of each result.
        #[arg(long)]
        show_context: bool,

        /// Output format: plain, json, csv [config: search.format].
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,

        /// Plain-text output only, never launch the TUI [config: search.no_tui].
        #[arg(long)]
        no_tui: bool,

        /// Force pure vector-only search (requires model).
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

    /// Check for a newer release and update the binary in-place.
    Update,

    /// Interactive first-time setup wizard.
    ///
    /// Sets preferences, optionally downloads the AI model, and installs shell
    /// completions. You can re-run this any time to reconfigure.
    Init,

    /// View and change persistent configuration.
    ///
    /// Settings are stored in ~/.config/scout/config.toml and override defaults
    /// without requiring flags on every command.
    ///
    /// Examples:
    ///   scout config list
    ///   scout config set search.limit 20
    ///   scout config set search.no_tui true
    ///   scout config set editor.command nvim
    ///   scout config get search.limit
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },

    /// Print shell completion script to stdout.
    ///
    /// Examples:
    ///   scout completions zsh > ~/.zsh/completions/_scout
    ///   scout completions bash > ~/.bash_completions/scout
    ///   scout completions fish > ~/.config/fish/completions/scout.fish
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
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

    /// Show index statistics: unit counts, languages, embeddings, disk usage.
    Stats {
        /// Repository root [default: current directory].
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
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
enum ConfigCommand {
    /// List all settings with their current values and descriptions.
    List,
    /// Get a single setting value.
    Get {
        /// Config key (e.g. search.limit).
        key: String,
    },
    /// Set a setting value persistently.
    Set {
        /// Config key (e.g. search.limit).
        key: String,
        /// New value.
        value: String,
    },
    /// Open the config file in your editor.
    Edit,
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

    // Load persistent config — used to provide defaults for search/index flags.
    let cfg = Config::load();

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
            // Merge CLI flags with config defaults.
            // Rule: explicit CLI flag > config file > built-in default.
            let query = query.unwrap_or_default();
            let effective_limit = limit.unwrap_or(cfg.search.limit);
            let effective_no_tui = no_tui || cfg.search.no_tui;
            let effective_exclude_tests = exclude_tests || cfg.search.exclude_tests;
            let effective_format = format.or_else(|| {
                cfg.search.format.as_deref().and_then(|s| match s {
                    "json" => Some(OutputFormat::Json),
                    "csv" => Some(OutputFormat::Csv),
                    _ => None,
                })
            });

            let modified_since = modified_last
                .map(|days| chrono::Utc::now().timestamp() - (days as i64 * 86_400));
            let filter = SearchFilter {
                lang,
                path_prefix: path_filter,
                modified_since,
                exclude_tests: effective_exclude_tests,
            };

            let use_tui = !effective_no_tui
                && effective_format.is_none()
                && !show_context
                && find_similar.is_none()
                && !all_repos
                && repos.is_none()
                && std::io::stdout().is_terminal();

            let _ = best; // accepted for backwards compatibility, no longer needed
            cli::search::run(cli::search::SearchArgs {
                path,
                query,
                limit: effective_limit,
                filter,
                show_context,
                format: effective_format,
                use_tui,
                semantic,
                all_repos,
                repos,
                find_similar,
                editor_cmd: cfg.editor.command.clone(),
                auto_index: cfg.index.auto_index,
            })?;
        }

        Command::Update => {
            cli::update::run()?;
        }

        Command::Init => {
            cli::init::run()?;
        }

        Command::Config { action } => match action {
            ConfigCommand::List => {
                cli::config_cmd::list()?;
            }
            ConfigCommand::Get { key } => {
                cli::config_cmd::get(cli::config_cmd::GetArgs { key })?;
            }
            ConfigCommand::Set { key, value } => {
                cli::config_cmd::set(cli::config_cmd::SetArgs { key, value })?;
            }
            ConfigCommand::Edit => {
                let path = config::config_path();
                // Ensure config file exists before opening.
                if !path.exists() {
                    Config::default().save()?;
                }
                editor::open_with(
                    &path.to_string_lossy(),
                    1,
                    &PathBuf::from("/"),
                    cfg.editor.command.as_deref(),
                )?;
            }
        },

        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
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

        Command::Stats { path } => {
            cli::stats::run(cli::stats::StatsArgs { path })?;
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
