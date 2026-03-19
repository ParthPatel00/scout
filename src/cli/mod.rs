pub mod index;
pub mod report;
pub mod search;

use clap::ValueEnum;

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Plain,
    Json,
    Csv,
}
