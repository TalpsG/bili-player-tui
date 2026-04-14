use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "bili-player-cli", version, about = "Bilibili audio player in your terminal")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to config file
    #[arg(long, global = true)]
    pub config: Option<String>,

    /// Enable debug logging
    #[arg(long, global = true)]
    pub debug: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Play a video by BV number or URL
    Play {
        /// BV number (e.g. BV1xx411c7mD) or Bilibili video URL
        input: String,
    },
}
