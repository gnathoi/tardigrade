use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "tdg",
    about = "tardigrade — modern archive tool",
    version,
    long_about = "Fast, multithreaded archiving with content-addressed dedup, \
                  self-healing erasure coding, and beautiful progress output."
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

        /// Allow dedup with encryption (leaks whether blocks have identical content)
        #[arg(long)]
        encrypt_allow_dedup: bool,

        /// Append to an existing archive (temporal mode)
        #[arg(long)]
        append: bool,

        /// Create incremental archive against a base
        #[arg(long, value_name = "BASE")]
        incremental: Option<PathBuf>,

        /// Reed-Solomon erasure coding: none, low (default), medium, high
        #[arg(long, value_name = "LEVEL", default_value = "low")]
        ecc: String,
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

    /// Print a file from an archive to stdout
    Cat {
        /// Archive file
        archive: PathBuf,

        /// Path of the file inside the archive
        path: String,

        /// Decrypt the archive (prompts for passphrase)
        #[arg(long, short, alias = "encrypt")]
        decrypt: bool,
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

    /// Generate shell tab-completion script
    ///
    /// Prints the completion script for the given shell to stdout. See
    /// `tdg completions --help` for install instructions per shell.
    Completions {
        /// Target shell: bash, zsh, fish, powershell, or elvish
        shell: Shell,
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
