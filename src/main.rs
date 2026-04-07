mod archive;
mod chunk;
mod cli;
mod compat;
mod compress;
mod dedup;
mod encrypt;
mod erasure;
mod error;
mod extract;
mod format;
mod fuse_mount;
mod hash;
mod incremental;
mod index;
mod merge;
mod metadata;
mod progress;
mod split;
mod temporal;
mod verify;

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use console::style;
use humansize::{BINARY, format_size};

use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();

    if let Some(threads) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .ok();
    }

    let result = match cli.command {
        Command::Create {
            archive,
            paths,
            compress: codec_name,
            level,
            no_ignore,
            encrypt,
        } => cmd_create(
            &archive,
            &paths,
            &codec_name,
            level,
            no_ignore,
            encrypt,
            cli.quiet,
        ),
        Command::Extract { archive, output } => {
            let dest = output.unwrap_or_else(|| PathBuf::from("."));
            cmd_extract(&archive, &dest, cli.quiet)
        }
        Command::List { archive, long } => cmd_list(&archive, long),
        Command::Info { archive } => cmd_info(&archive),
        Command::Verify { archive } => cmd_verify(&archive, cli.quiet),
    };

    if let Err(e) = result {
        eprintln!("{} {}", style("error:").red().bold(), e);
        std::process::exit(1);
    }
}

fn cmd_create(
    archive: &PathBuf,
    paths: &[PathBuf],
    codec_name: &str,
    level: i32,
    no_ignore: bool,
    encrypt: bool,
    quiet: bool,
) -> error::Result<()> {
    let codec = compress::codec_from_str(codec_name)?;

    if encrypt && !quiet {
        println!(
            "  {} Encryption enabled (dedup disabled for privacy)",
            style("🔒").dim()
        );
    }

    let opts = archive::CreateOptions {
        codec,
        level,
        show_progress: !quiet,
        respect_gitignore: !no_ignore,
    };

    let source_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();

    let start = Instant::now();
    let stats = archive::create_archive(archive.as_path(), &source_refs, &opts)?;
    let elapsed = start.elapsed();

    if !quiet {
        let ratio = if stats.total_input_size > 0 {
            stats.total_input_size as f64 / stats.archive_size as f64
        } else {
            1.0
        };

        let throughput = if elapsed.as_secs_f64() > 0.0 {
            stats.total_input_size as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
        } else {
            0.0
        };

        println!(
            "\n{} Created {} ({} {} {}, {:.1}x ratio, {:.1}s, {:.0} MB/s)",
            style("✓").green().bold(),
            style(archive.display()).bold(),
            style(format_size(stats.archive_size, BINARY)).cyan(),
            style("←").dim(),
            style(format_size(stats.total_input_size, BINARY)).white(),
            ratio,
            elapsed.as_secs_f64(),
            throughput,
        );

        println!(
            "  {} files, {} dirs, {} blocks ({} unique)",
            style(stats.file_count).bold(),
            stats.dir_count,
            stats.block_count,
            stats.unique_blocks,
        );

        if stats.dedup_savings > 0 {
            println!(
                "  {} {} saved by dedup",
                style("↗").green(),
                style(format_size(stats.dedup_savings, BINARY))
                    .green()
                    .bold(),
            );
        }

        println!(
            "  Compression: {}  Codec: {}",
            style(format!("{:.1}x", ratio)).cyan(),
            compress::codec_name(codec),
        );
    }

    Ok(())
}

fn cmd_extract(archive: &PathBuf, dest: &PathBuf, quiet: bool) -> error::Result<()> {
    let start = Instant::now();
    let stats = extract::extract_archive(archive.as_path(), dest.as_path())?;
    let elapsed = start.elapsed();

    if !quiet {
        println!(
            "\n{} Extracted {} ({} files, {} dirs, {}, {:.1}s)",
            style("✓").green().bold(),
            style(archive.display()).bold(),
            style(stats.file_count).bold(),
            stats.dir_count,
            style(format_size(stats.total_size, BINARY)).cyan(),
            elapsed.as_secs_f64(),
        );
    }

    Ok(())
}

fn cmd_list(archive: &PathBuf, long: bool) -> error::Result<()> {
    let entries = extract::list_archive(archive.as_path())?;

    for entry in &entries {
        if long {
            let type_char = match &entry.file_type {
                format::FileType::Directory => 'd',
                format::FileType::File => '-',
                format::FileType::Symlink(_) => 'l',
                format::FileType::Hardlink(_) => 'h',
            };
            let size = if matches!(entry.file_type, format::FileType::File) {
                format_size(entry.size, BINARY)
            } else {
                "-".to_string()
            };
            println!(
                "{}{:o}  {:>10}  {}",
                type_char,
                entry.mode & 0o7777,
                size,
                entry.path_display()
            );
        } else {
            println!("{}", entry.path_display());
        }
    }

    if long {
        println!("\n{} entries", style(entries.len()).bold());
    }

    Ok(())
}

fn cmd_info(archive: &PathBuf) -> error::Result<()> {
    let file = std::fs::File::open(archive).map_err(|e| error::Error::io_path(archive, e))?;
    let mut reader = std::io::BufReader::new(file);

    let header = format::ArchiveHeader::read_from(&mut reader)?;
    let footer = extract::read_footer(&mut reader)?;
    let entries = extract::read_index(&mut reader, &footer)?;

    let file_count = entries
        .iter()
        .filter(|e| matches!(e.file_type, format::FileType::File))
        .count();
    let dir_count = entries
        .iter()
        .filter(|e| matches!(e.file_type, format::FileType::Directory))
        .count();
    let total_size: u64 = entries
        .iter()
        .filter(|e| matches!(e.file_type, format::FileType::File))
        .map(|e| e.size)
        .sum();

    let archive_size = std::fs::metadata(archive)
        .map_err(|e| error::Error::io_path(archive, e))?
        .len();

    println!("{}", style("Archive Info").bold().underlined());
    println!("  File:          {}", archive.display());
    println!("  Format:        TRDG v{}", header.version);
    println!("  Archive size:  {}", format_size(archive_size, BINARY));
    println!("  Original size: {}", format_size(total_size, BINARY));
    if total_size > 0 {
        println!(
            "  Ratio:         {:.1}x",
            total_size as f64 / archive_size as f64
        );
    }
    println!("  Files:         {}", file_count);
    println!("  Directories:   {}", dir_count);
    println!("  Blocks:        {}", footer.block_count);

    let mut flags = vec![];
    if header.is_encrypted() {
        flags.push("encrypted");
    }
    if header.is_erasure_coded() {
        flags.push("erasure-coded");
    }
    if header.is_append_only() {
        flags.push("append-only");
    }
    if flags.is_empty() {
        flags.push("none");
    }
    println!("  Flags:         {}", flags.join(", "));
    println!("  Root hash:     {}", hex::encode(&footer.root_hash[..8]));

    Ok(())
}

fn cmd_verify(archive: &PathBuf, quiet: bool) -> error::Result<()> {
    let start = Instant::now();
    let result = extract::verify_archive(archive.as_path())?;
    let elapsed = start.elapsed();

    if !quiet {
        if result.blocks_corrupted == 0 {
            println!(
                "\n{} Archive OK — {} blocks verified in {:.1}s",
                style("✓").green().bold(),
                style(result.blocks_checked).bold(),
                elapsed.as_secs_f64(),
            );
        } else {
            println!(
                "\n{} Archive CORRUPTED — {}/{} blocks damaged",
                style("✗").red().bold(),
                style(result.blocks_corrupted).red().bold(),
                result.blocks_checked,
            );
            for file in &result.corrupted_files {
                println!("  {} {}", style("✗").red(), file);
            }
        }
    }

    if result.blocks_corrupted > 0 {
        std::process::exit(1);
    }

    Ok(())
}
