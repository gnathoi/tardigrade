use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "tdg",
    about = "tardigrade — modern archive tool",
    version,
    long_about = "Fast, multithreaded archiving with content-addressed dedup, \
                  checksums, and beautiful progress output."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Number of threads (default: all cores)
    #[arg(long, short = 'j', global = true)]
    pub threads: Option<usize>,

    /// Suppress all output
    #[arg(long, short, global = true)]
    pub quiet: bool,

    /// Verbose output
    #[arg(long, short, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create an archive
    #[command(alias = "c")]
    Create {
        /// Archive file to create
        archive: PathBuf,

        /// Paths to archive
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Compression algorithm: zstd (default), lz4, none
        #[arg(long, default_value = "zstd")]
        compress: String,

        /// Compression level (1-19 for zstd)
        #[arg(long, short, default_value = "3")]
        level: i32,

        /// Don't respect .gitignore files
        #[arg(long)]
        no_ignore: bool,

        /// Encrypt the archive (prompts for passphrase)
        #[arg(long, short)]
        encrypt: bool,
    },

    /// Extract an archive
    #[command(alias = "x")]
    Extract {
        /// Archive file to extract
        archive: PathBuf,

        /// Destination directory (default: current directory)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Decrypt the archive (prompts for passphrase)
        #[arg(long, short)]
        encrypt: bool,
    },

    /// List archive contents
    #[command(alias = "ls")]
    List {
        /// Archive file to list
        archive: PathBuf,

        /// Show detailed info (sizes, permissions, timestamps)
        #[arg(long, short)]
        long: bool,
    },

    /// Show archive info and statistics
    Info {
        /// Archive file to inspect
        archive: PathBuf,
    },

    /// Verify archive integrity
    Verify {
        /// Archive file to verify
        archive: PathBuf,
    },
}
