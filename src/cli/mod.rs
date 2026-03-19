pub mod config_cmd;
pub mod daemon;
pub mod index;
pub mod init;
pub mod maintenance;
pub mod report;
pub mod repos;
pub mod search;
pub mod stats;
pub mod update;

use clap::ValueEnum;

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Plain,
    Json,
    Csv,
}
