mod cli;
mod index;
mod search;
mod storage;
mod types;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    },

    /// Search the index (BM25 + RRF re-ranking).
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
    },

    /// Generate reports about the codebase.
    Report {
        /// Root directory of the repository.
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        #[command(subcommand)]
        kind: ReportCommand,
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
        Command::Index { path, verbose } => {
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
        } => {
            let modified_since = modified_last.map(|days| {
                chrono::Utc::now().timestamp() - (days as i64 * 86_400)
            });
            let filter = SearchFilter {
                lang,
                path_prefix: path_filter,
                modified_since,
                exclude_tests,
            };
            cli::search::run(cli::search::SearchArgs {
                path,
                query,
                limit,
                filter,
                show_context,
            })?;
        }
        Command::Report { path, kind } => {
            let report_kind = match kind {
                ReportCommand::UnusedFunctions => cli::report::ReportKind::UnusedFunctions,
            };
            cli::report::run(cli::report::ReportArgs { path, kind: report_kind })?;
        }
    }

    Ok(())
}
