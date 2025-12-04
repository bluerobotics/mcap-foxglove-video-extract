use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "mcap-foxglove-video-extract")]
#[command(about = "List foxglove.CompressedVideo topics or extract them to MP4", long_about = None)]
pub struct Cli {
    /// Path to MCAP file
    pub mcap_file: PathBuf,

    /// Topic name to extract video from, use 'all' to extract all topics
    pub topic: Option<String>,

    /// Output directory
    #[arg(long, default_value = ".")]
    pub output: PathBuf,
}
