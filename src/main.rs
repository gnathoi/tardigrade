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

use std::path::{Path, PathBuf};
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
            append,
            incremental: incremental_base,
            ecc,
        } => {
            if append {
                cmd_append(&archive, &paths, &codec_name, level, no_ignore, cli.quiet)
            } else if let Some(base) = incremental_base {
                cmd_create_incremental(
                    &base,
                    &archive,
                    &paths,
                    &codec_name,
                    level,
                    no_ignore,
                    cli.quiet,
                )
            } else {
                cmd_create(
                    &archive,
                    &paths,
                    &codec_name,
                    level,
                    no_ignore,
                    encrypt,
                    ecc,
                    cli.quiet,
                )
            }
        }
        Command::Extract {
            archive,
            output,
            encrypt,
            base,
            generation,
        } => {
            let dest = output.unwrap_or_else(|| PathBuf::from("."));
            if let Some(base_path) = base {
                cmd_extract_incremental(&archive, &base_path, &dest, cli.quiet)
            } else if let Some(g) = generation {
                cmd_extract_generation(&archive, g, &dest, cli.quiet)
            } else {
                cmd_extract(&archive, &dest, encrypt, cli.quiet)
            }
        }
        Command::List { archive, long } => cmd_list(&archive, long),
        Command::Info { archive } => cmd_info(&archive),
        Command::Verify { archive } => cmd_verify(&archive, cli.quiet),
        Command::Log { archive } => cmd_log(&archive),
        Command::Merge { a, b, output } => cmd_merge(&a, &b, &output, cli.quiet),
        Command::Split { archive, size } => cmd_split(&archive, &size, cli.quiet),
        Command::Join { volumes, output } => cmd_join(&volumes, &output, cli.quiet),
        Command::Convert {
            input,
            output,
            compress: codec_name,
            level,
        } => cmd_convert(&input, &output, &codec_name, level, cli.quiet),
    };

    if let Err(e) = result {
        eprintln!("{} {}", style("error:").red().bold(), e);
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_create(
    archive: &Path,
    paths: &[PathBuf],
    codec_name: &str,
    level: i32,
    no_ignore: bool,
    encrypt: bool,
    ecc: Option<String>,
    quiet: bool,
) -> error::Result<()> {
    let codec = compress::codec_from_str(codec_name)?;

    // Validate ECC level if provided
    let ecc_level = if let Some(ref ecc_str) = ecc {
        Some(
            erasure::EccLevel::from_str(ecc_str)
                .ok_or_else(|| error::Error::Ecc(format!("unknown ECC level: {ecc_str}")))?,
        )
    } else {
        None
    };

    let passphrase = if encrypt {
        if !quiet {
            println!(
                "  {}",
                style("encryption enabled (dedup disabled for privacy)").dim()
            );
        }
        let pass = rpassword::prompt_password("Passphrase: ")
            .map_err(|e| error::Error::Io { source: e })?;
        let confirm = rpassword::prompt_password("Confirm passphrase: ")
            .map_err(|e| error::Error::Io { source: e })?;
        if pass != confirm {
            return Err(error::Error::InvalidArchive(
                "passphrases do not match".into(),
            ));
        }
        Some(pass.into_bytes())
    } else {
        None
    };

    let opts = archive::CreateOptions {
        codec,
        level,
        show_progress: !quiet,
        respect_gitignore: !no_ignore,
        passphrase,
    };

    let source_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();

    let start = Instant::now();
    let stats = archive::create_archive(archive, &source_refs, &opts)?;
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

        println!();
        println!(
            "  {} {}",
            style("created").green().bold(),
            style(archive.display()).white().bold(),
        );
        println!();
        println!(
            "  {}  {}  {}",
            style(format!(
                "{} -> {}",
                format_size(stats.total_input_size, BINARY),
                format_size(stats.archive_size, BINARY)
            ))
            .white(),
            style(format!("{:.1}x", ratio)).cyan().bold(),
            style(compress::codec_name(codec)).dim(),
        );
        println!(
            "  {}  {}",
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
            style(format!(
                "{} blocks ({} unique)",
                stats.block_count, stats.unique_blocks
            ))
            .dim(),
        );

        if stats.dedup_savings > 0 {
            println!(
                "  {} {}",
                style(format!(
                    "{} saved by dedup",
                    format_size(stats.dedup_savings, BINARY)
                ))
                .green()
                .bold(),
                style(format!(
                    "({} duplicate blocks eliminated)",
                    stats.block_count - stats.unique_blocks
                ))
                .dim(),
            );
        }

        if let Some(ref level) = ecc_level {
            println!(
                "  {}",
                style(format!(
                    "ecc: RS({},{}) ~{:.0}% overhead",
                    level.data_shards,
                    level.parity_shards,
                    level.overhead_percent()
                ))
                .dim(),
            );
        }

        println!(
            "  {}",
            style(format!(
                "{:.2}s  {:.0} MB/s",
                elapsed.as_secs_f64(),
                throughput
            ))
            .dim(),
        );
    }

    Ok(())
}

fn cmd_append(
    archive: &Path,
    paths: &[PathBuf],
    codec_name: &str,
    level: i32,
    no_ignore: bool,
    quiet: bool,
) -> error::Result<()> {
    let codec = compress::codec_from_str(codec_name)?;

    let opts = archive::CreateOptions {
        codec,
        level,
        show_progress: !quiet,
        respect_gitignore: !no_ignore,
        passphrase: None,
    };

    let source_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();

    let start = Instant::now();
    let stats = temporal::append_archive(archive, &source_refs, &opts)?;
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        println!(
            "  {} {} {}",
            style("appended").green().bold(),
            style(format!("generation {}", stats.generation))
                .cyan()
                .bold(),
            style(archive.display()).white().bold(),
        );
        println!();
        println!(
            "  {}  {}",
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
            style(format!(
                "{} new blocks, {} reused",
                stats.new_blocks, stats.reused_blocks
            ))
            .dim(),
        );
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_create_incremental(
    base: &Path,
    archive: &Path,
    paths: &[PathBuf],
    codec_name: &str,
    level: i32,
    no_ignore: bool,
    quiet: bool,
) -> error::Result<()> {
    let codec = compress::codec_from_str(codec_name)?;

    let opts = archive::CreateOptions {
        codec,
        level,
        show_progress: !quiet,
        respect_gitignore: !no_ignore,
        passphrase: None,
    };

    let source_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();

    let start = Instant::now();
    let stats = incremental::create_incremental(base, archive, &source_refs, &opts)?;
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        println!(
            "  {} {}",
            style("created incremental").green().bold(),
            style(archive.display()).white().bold(),
        );
        println!();
        println!(
            "  {}  {}",
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
            style(format!(
                "{} new blocks, {} reused from base",
                stats.new_blocks, stats.reused_blocks
            ))
            .dim(),
        );
        println!(
            "  {}  {}",
            style(format_size(stats.archive_size, BINARY)).white(),
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim(),
        );
    }

    Ok(())
}

fn cmd_extract(archive: &Path, dest: &Path, encrypt: bool, quiet: bool) -> error::Result<()> {
    // Auto-detect tar format
    if let Some(format) = compat::detect_legacy_format(archive)? {
        return cmd_extract_legacy(archive, dest, format, quiet);
    }

    let start = Instant::now();
    let stats = if encrypt {
        let pass = rpassword::prompt_password("Passphrase: ")
            .map_err(|e| error::Error::Io { source: e })?;
        extract::extract_archive_encrypted(archive, dest, pass.as_bytes())?
    } else {
        extract::extract_archive(archive, dest)?
    };
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        println!(
            "  {} {} {} {}",
            style("extracted").green().bold(),
            style(archive.display()).white().bold(),
            style("->").dim(),
            style(dest.display()).white(),
        );
        println!();
        println!(
            "  {}  {}",
            style(format_size(stats.total_size, BINARY)).white(),
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
        );
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_extract_legacy(
    archive: &Path,
    dest: &Path,
    format: compat::LegacyFormat,
    quiet: bool,
) -> error::Result<()> {
    let start = Instant::now();
    let stats = compat::extract_legacy(archive, dest)?;
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        println!(
            "  {} {} {} {}",
            style(format!("extracted ({})", format)).green().bold(),
            style(archive.display()).white().bold(),
            style("->").dim(),
            style(dest.display()).white(),
        );
        println!();
        println!(
            "  {}  {}",
            style(format_size(stats.total_size, BINARY)).white(),
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
        );
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_extract_incremental(
    archive: &Path,
    base: &Path,
    dest: &Path,
    quiet: bool,
) -> error::Result<()> {
    let start = Instant::now();
    let stats = incremental::extract_incremental(archive, base, dest)?;
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        println!(
            "  {} {} {} {}",
            style("extracted (incremental)").green().bold(),
            style(archive.display()).white().bold(),
            style("->").dim(),
            style(dest.display()).white(),
        );
        println!();
        println!(
            "  {}  {}",
            style(format_size(stats.total_size, BINARY)).white(),
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
        );
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_extract_generation(
    archive: &Path,
    generation: u64,
    dest: &Path,
    quiet: bool,
) -> error::Result<()> {
    let start = Instant::now();
    let stats = temporal::extract_generation(archive, generation, dest)?;
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        println!(
            "  {} {} {} {} {}",
            style("extracted").green().bold(),
            style(archive.display()).white().bold(),
            style(format!("@{generation}")).cyan().bold(),
            style("->").dim(),
            style(dest.display()).white(),
        );
        println!();
        println!(
            "  {}  {}",
            style(format_size(stats.total_size, BINARY)).white(),
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
        );
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_list(archive: &Path, long: bool) -> error::Result<()> {
    let entries = extract::list_archive(archive)?;

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

fn cmd_info(archive: &Path) -> error::Result<()> {
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
    if header.is_incremental() {
        flags.push("incremental");
    }
    if flags.is_empty() {
        flags.push("none");
    }
    println!("  Flags:         {}", flags.join(", "));
    println!("  Root hash:     {}", hex::encode(&footer.root_hash[..8]));

    // Show generation count for temporal archives
    if header.is_append_only()
        && let Ok(snapshots) = temporal::list_snapshots(archive)
    {
        println!("  Generations:   {}", snapshots.len());
    }

    Ok(())
}

fn cmd_verify(archive: &Path, quiet: bool) -> error::Result<()> {
    let start = Instant::now();
    let report = verify::verify_full(archive)?;
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        if report.blocks_corrupted == 0 {
            println!(
                "  {} {}",
                style("verified").green().bold(),
                style(archive.display()).white().bold(),
            );
        } else {
            println!(
                "  {} {}",
                style("CORRUPTED").red().bold(),
                style(archive.display()).white().bold(),
            );
        }
        println!();
        println!(
            "  header {}  footer {}  index {}",
            if report.header_ok {
                style("ok").green()
            } else {
                style("FAIL").red().bold()
            },
            if report.footer_ok {
                style("ok").green()
            } else {
                style("FAIL").red().bold()
            },
            if report.index_ok {
                style("ok").green()
            } else {
                style("FAIL").red().bold()
            },
        );
        println!(
            "  blocks {}/{} ok, {} corrupted",
            style(report.blocks_ok).green(),
            report.blocks_checked,
            if report.blocks_corrupted > 0 {
                style(report.blocks_corrupted).red().bold()
            } else {
                style(0u64).green().bold()
            },
        );
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );

        if !report.corrupted_blocks.is_empty() {
            println!();
            println!("  {}", style("damage map:").red().bold());
            for block in &report.corrupted_blocks {
                println!(
                    "    offset {}: {} (expected {})",
                    block.offset, block.error, block.expected_hash
                );
            }
        }

        if !report.affected_files.is_empty() {
            println!();
            println!("  {}", style("affected files:").red().bold());
            for file in &report.affected_files {
                println!("    {}", style(file).red());
            }
        }

        if report.blocks_corrupted == 0 {
            println!();
        }
    }

    if report.blocks_corrupted > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn cmd_log(archive: &Path) -> error::Result<()> {
    let snapshots = temporal::list_snapshots(archive)?;

    if snapshots.is_empty() {
        println!("  no generations found");
        return Ok(());
    }

    println!(
        "  {} {}",
        style(format!("{} generations", snapshots.len()))
            .white()
            .bold(),
        style(archive.display()).dim(),
    );
    println!();

    for snap in &snapshots {
        println!(
            "  {}  {} files, {} dirs  {}",
            style(format!("@{}", snap.generation)).cyan().bold(),
            snap.file_count,
            snap.dir_count,
            style(format_size(snap.total_size, BINARY)).dim(),
        );
    }

    Ok(())
}

fn cmd_merge(a: &Path, b: &Path, output: &Path, quiet: bool) -> error::Result<()> {
    let start = Instant::now();
    let stats = merge::merge_archives(a, b, output)?;
    let elapsed = start.elapsed();

    if !quiet {
        let archive_size = std::fs::metadata(output)
            .map_err(|e| error::Error::io_path(output, e))?
            .len();

        println!();
        println!(
            "  {} {}",
            style("merged").green().bold(),
            style(output.display()).white().bold(),
        );
        println!();
        println!(
            "  {}  {}  {}",
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
            style(format!("{} unique blocks", stats.unique_blocks)).dim(),
            style(format_size(archive_size, BINARY)).white(),
        );
        if stats.conflicts > 0 {
            println!(
                "  {}",
                style(format!(
                    "{} path conflicts resolved (newer mtime wins)",
                    stats.conflicts
                ))
                .dim(),
            );
        }
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_split(archive: &Path, size_str: &str, quiet: bool) -> error::Result<()> {
    let max_size = split::parse_size(size_str)
        .map_err(|e| error::Error::Volume(format!("invalid size: {e}")))?;

    let start = Instant::now();
    let volumes = split::split_archive(archive, max_size)?;
    let elapsed = start.elapsed();

    if !quiet {
        println!();
        println!(
            "  {} {} {} {} volumes",
            style("split").green().bold(),
            style(archive.display()).white().bold(),
            style("->").dim(),
            style(volumes.len()).cyan().bold(),
        );
        println!();
        for vol in &volumes {
            let size = std::fs::metadata(vol).map(|m| m.len()).unwrap_or(0);
            println!(
                "    {}  {}",
                style(vol.display()).white(),
                style(format_size(size, BINARY)).dim(),
            );
        }
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_join(volumes: &[PathBuf], output: &Path, quiet: bool) -> error::Result<()> {
    let start = Instant::now();
    split::join_volumes(volumes, output)?;
    let elapsed = start.elapsed();

    if !quiet {
        let size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
        println!();
        println!(
            "  {} {} {} {}",
            style("joined").green().bold(),
            style(format!("{} volumes", volumes.len())).dim(),
            style("->").dim(),
            style(output.display()).white().bold(),
        );
        println!();
        println!("  {}", style(format_size(size, BINARY)).white(),);
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}

fn cmd_convert(
    input: &Path,
    output: &Path,
    codec_name: &str,
    level: i32,
    quiet: bool,
) -> error::Result<()> {
    let codec = compress::codec_from_str(codec_name)?;

    let format = compat::detect_legacy_format(input)?
        .ok_or_else(|| error::Error::InvalidArchive("not a recognized tar format".into()))?;

    let start = Instant::now();
    let stats = compat::convert_to_tg(input, output, codec, level, quiet)?;
    let elapsed = start.elapsed();

    if !quiet {
        let archive_size = std::fs::metadata(output)
            .map_err(|e| error::Error::io_path(output, e))?
            .len();

        println!();
        println!(
            "  {} {} {} {}",
            style(format!("converted ({})", format)).green().bold(),
            style(input.display()).white().bold(),
            style("->").dim(),
            style(output.display()).white().bold(),
        );
        println!();
        println!(
            "  {}  {}  {}",
            style(format!(
                "{} files, {} dirs",
                stats.file_count, stats.dir_count
            ))
            .dim(),
            style(format!("{} unique blocks", stats.unique_blocks)).dim(),
            style(format_size(archive_size, BINARY)).white(),
        );
        if stats.dedup_savings > 0 {
            println!(
                "  {}",
                style(format!(
                    "{} saved by dedup",
                    format_size(stats.dedup_savings, BINARY)
                ))
                .green()
                .bold(),
            );
        }
        println!(
            "  {}",
            style(format!("{:.2}s", elapsed.as_secs_f64())).dim()
        );
    }

    Ok(())
}
