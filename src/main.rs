mod cli;
mod index;
mod search;
mod storage;
mod types;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

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

    /// Search the index (fast BM25 mode).
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
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Index { path, verbose } => {
            cli::index::run(cli::index::IndexArgs { path, verbose })?;
        }
        Command::Search { query, path, limit } => {
            cli::search::run(cli::search::SearchArgs { path, query, limit })?;
        }
    }

    Ok(())
}
