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

        /// Compression level (1-19 for zstd, default 9)
        #[arg(long, short, default_value = "9")]
        level: i32,

        /// Don't respect .gitignore files
        #[arg(long)]
        no_ignore: bool,

        /// Encrypt the archive (prompts for passphrase)
        #[arg(long, short)]
        encrypt: bool,

        /// Append to an existing archive (temporal mode)
        #[arg(long)]
        append: bool,

        /// Create incremental archive against a base
        #[arg(long, value_name = "BASE")]
        incremental: Option<PathBuf>,

        /// Reed-Solomon erasure coding level: low, medium, high
        #[arg(long, value_name = "LEVEL")]
        ecc: Option<String>,
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
        #[arg(long, short, alias = "encrypt")]
        decrypt: bool,

        /// Base archive for incremental extraction
        #[arg(long, value_name = "BASE")]
        base: Option<PathBuf>,

        /// Extract a specific generation (temporal archives)
        #[arg(long, value_name = "N")]
        generation: Option<u64>,
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

    /// List temporal archive generations
    Log {
        /// Archive file to inspect
        archive: PathBuf,
    },

    /// Merge two archives into one
    Merge {
        /// First archive
        a: PathBuf,

        /// Second archive
        b: PathBuf,

        /// Output archive
        #[arg(short, long, required = true)]
        output: PathBuf,
    },

    /// Split an archive into volumes
    Split {
        /// Archive file to split
        archive: PathBuf,

        /// Maximum volume size (e.g., 4G, 100M, 500K)
        #[arg(long, required = true)]
        size: String,
    },

    /// Join split volumes back into a single archive
    Join {
        /// Volume files in order
        #[arg(required = true)]
        volumes: Vec<PathBuf>,

        /// Output archive
        #[arg(short, long, required = true)]
        output: PathBuf,
    },

    /// Repair corrupted blocks using ECC parity data
    Repair {
        /// Archive file to repair
        archive: PathBuf,
    },

    /// Diff between temporal generations
    Diff {
        /// Archive file
        archive: PathBuf,

        /// First generation number
        #[arg(long)]
        from: u64,

        /// Second generation number
        #[arg(long)]
        to: u64,
    },

    /// Update tdg to the latest release
    Update {
        /// Only check for updates, don't install
        #[arg(long)]
        check: bool,
    },

    /// Convert a tar/tar.gz/tar.zst archive to .tg format
    Convert {
        /// Legacy archive to convert
        input: PathBuf,

        /// Output .tg archive
        output: PathBuf,

        /// Compression algorithm: zstd (default), lz4, none
        #[arg(long, default_value = "zstd")]
        compress: String,

        /// Compression level (1-19 for zstd, default 9)
        #[arg(long, short, default_value = "9")]
        level: i32,
    },
}
