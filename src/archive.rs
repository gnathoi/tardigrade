use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::Ordering;

use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::chunk::chunk_data;
use crate::compress;
use crate::dedup::DedupStore;
use crate::encrypt::{self, KeyEncapsulation};
use crate::erasure::{self, EccGroup, EccLevel};
use crate::error::{Error, Result};
use crate::format::*;
use crate::hash::{hash_block, merkle_root};
use crate::index::serialize_index;
use crate::metadata::capture_metadata;
use crate::progress::CreateProgress;

/// Minimum chunk size — files smaller than this skip FastCDC entirely
const MIN_CHUNK_SIZE: usize = 64 * 1024;

/// Minimum size where compression is likely to help
const MIN_COMPRESS_SIZE: usize = 64;

/// BufWriter buffer size (256 KB for better sequential write performance)
const WRITE_BUFFER_SIZE: usize = 256 * 1024;

/// Options for archive creation
pub struct CreateOptions {
    pub codec: u8,
    pub level: i32,
    pub show_progress: bool,
    pub respect_gitignore: bool,
    pub passphrase: Option<Vec<u8>>,
    pub ecc_level: Option<EccLevel>,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            codec: CODEC_ZSTD,
            level: 9,
            show_progress: false,
            respect_gitignore: true,
            passphrase: None,
            ecc_level: None,
        }
    }
}

/// Stats returned after archive creation
#[derive(Debug)]
pub struct CreateStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub total_input_size: u64,
    pub total_compressed_size: u64,
    pub block_count: u64,
    pub unique_blocks: u64,
    pub dedup_savings: u64,
    pub archive_size: u64,
    pub parity_blocks: u64,
}

struct ProcessedFile {
    entry: FileEntry,
    chunks: Vec<CompressedChunk>,
}

struct CompressedChunk {
    hash: Hash,
    uncompressed_size: u32,
    compressed_data: Vec<u8>,
    codec: u8,
}

/// Chunk and compress a file's data. Optimized for common cases:
/// - Empty files: no chunks
/// - Small files (< 64KB): single block, skip FastCDC gear hash
/// - Tiny blocks (< 64B): store uncompressed (zstd overhead > savings)
/// - Large files: FastCDC content-defined chunking
fn process_file_data(data: &[u8], codec: u8, level: i32) -> Result<Vec<CompressedChunk>> {
    if data.is_empty() {
        return Ok(vec![]);
    }

    // Small files: skip FastCDC entirely — one block, just hash + compress
    if data.len() < MIN_CHUNK_SIZE {
        let hash = hash_block(data);
        let compressed = if data.len() >= MIN_COMPRESS_SIZE {
            let c = compress::compress(data, codec, level)?;
            // Only use compressed if it's actually smaller
            if c.len() < data.len() {
                (c, codec)
            } else {
                (data.to_vec(), CODEC_NONE)
            }
        } else {
            (data.to_vec(), CODEC_NONE) // too small to compress
        };
        return Ok(vec![CompressedChunk {
            hash,
            uncompressed_size: data.len() as u32,
            compressed_data: compressed.0,
            codec: compressed.1,
        }]);
    }

    // Large files: FastCDC content-defined chunking
    let raw_chunks = chunk_data(data);
    raw_chunks
        .into_iter()
        .map(|chunk| {
            let orig_size = chunk.data.len() as u32;
            let (compressed_data, actual_codec) = if chunk.data.len() >= MIN_COMPRESS_SIZE {
                let c = compress::compress(&chunk.data, codec, level)?;
                if c.len() < chunk.data.len() {
                    (c, codec)
                } else {
                    (chunk.data, CODEC_NONE)
                }
            } else {
                (chunk.data, CODEC_NONE)
            };
            Ok(CompressedChunk {
                hash: chunk.hash,
                uncompressed_size: orig_size,
                compressed_data,
                codec: actual_codec,
            })
        })
        .collect()
}

/// Write parity blocks for a completed ECC group.
fn flush_ecc_group(
    group: &EccGroup,
    level: &EccLevel,
    writer: &mut impl Write,
    mut offset: u64,
    block_hashes: &mut Vec<Hash>,
    stats: &mut CreateStats,
) -> Result<u64> {
    let parity_shards = erasure::encode_parity(group, level)?;

    for parity_data in &parity_shards {
        let parity_hash: Hash = blake3::hash(parity_data).into();
        let block_header = BlockHeader::new_parity(
            parity_hash,
            parity_data.len() as u32,
            group.shard_size as u32,
            level.parity_shards as u8,
        );

        block_header.write_to(writer)?;
        writer.write_all(parity_data)?;

        offset += BLOCK_HEADER_SIZE as u64 + parity_data.len() as u64;
        block_hashes.push(parity_hash);
        stats.parity_blocks += 1;
    }

    Ok(offset)
}

/// Create an archive from the given source paths.
pub fn create_archive(
    archive_path: &Path,
    sources: &[&Path],
    opts: &CreateOptions,
) -> Result<CreateStats> {
    // Phase 1: Walk and collect file paths + sizes (parallel walk)
    let walk_entries: Vec<(std::path::PathBuf, std::path::PathBuf, u64)>;
    let total_bytes: u64;

    {
        use std::sync::Mutex;
        use std::sync::atomic::AtomicU64;

        // Show scanning spinner if progress is enabled
        let scan_spinner = if opts.show_progress {
            let sp = indicatif::ProgressBar::new_spinner();
            sp.set_style(
                indicatif::ProgressStyle::with_template("  {spinner} {msg}")
                    .unwrap()
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]),
            );
            sp.enable_steady_tick(std::time::Duration::from_millis(80));
            sp.set_message("scanning…");
            Some(sp)
        } else {
            None
        };

        let entries = Mutex::new(Vec::new());
        let bytes = AtomicU64::new(0);
        let file_count = AtomicU64::new(0);

        for source in sources {
            let source_path = source.to_path_buf();
            let walker = WalkBuilder::new(source)
                .git_ignore(opts.respect_gitignore)
                .git_global(opts.respect_gitignore)
                .git_exclude(opts.respect_gitignore)
                .hidden(false)
                .threads(rayon::current_num_threads().max(4))
                .filter_entry(|e| {
                    !(e.file_type().is_some_and(|ft| ft.is_dir()) && e.file_name() == ".git")
                })
                .follow_links(false)
                .build_parallel();

            walker.run(|| {
                let entries = &entries;
                let bytes = &bytes;
                let file_count = &file_count;
                let source_path = &source_path;
                let scan_spinner = &scan_spinner;
                Box::new(move |result| {
                    let entry = match result {
                        Ok(e) => e,
                        Err(_) => return ignore::WalkState::Continue,
                    };
                    let path = entry.path().to_path_buf();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    if entry.file_type().is_some_and(|ft| ft.is_file()) {
                        bytes.fetch_add(size, Ordering::Relaxed);
                        let count = file_count.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(sp) = scan_spinner
                            && count.is_multiple_of(1000) {
                                sp.set_message(format!("scanning… {} files", count));
                            }
                    }
                    entries.lock().unwrap().push((path, source_path.clone(), size));
                    ignore::WalkState::Continue
                })
            });
        }

        walk_entries = entries.into_inner().unwrap();
        total_bytes = bytes.load(Ordering::Relaxed);

        if let Some(sp) = scan_spinner {
            sp.finish_and_clear();
        }
    }

    let progress = if opts.show_progress {
        let p = CreateProgress::new(total_bytes);
        p.stats.bytes_scanned.store(total_bytes, Ordering::Relaxed);
        p.stats
            .files_scanned
            .store(walk_entries.len() as u64, Ordering::Relaxed);
        Some(p)
    } else {
        None
    };

    // Phase 2: Read, chunk, and compress files in parallel
    let codec = opts.codec;
    let level = opts.level;

    // Phase 2+3: Process files in parallel batches, write in walk order.
    // Each batch is compressed in parallel via rayon, then written sequentially
    // before the next batch starts. Memory is bounded to O(batch_size × file_size).
    let batch_size = rayon::current_num_threads().max(4) * 4;

    let progress_ref = &progress;
    let process_one =
        |(path, source, _size): &(std::path::PathBuf, std::path::PathBuf, u64)| -> Result<ProcessedFile> {
            let file_entry = capture_metadata(path, source)?;

            let chunks = match &file_entry.file_type {
                FileType::File => {
                    let data = fs::read(path).map_err(|e| Error::io_path(path, e))?;
                    let len = data.len() as u64;
                    let chunks = process_file_data(&data, codec, level)?;
                    if let Some(p) = progress_ref {
                        p.inc_processed(len);
                    }
                    chunks
                }
                _ => vec![],
            };

            Ok(ProcessedFile {
                entry: file_entry,
                chunks,
            })
        };

    // Writer consumes from channel
    let file = File::create(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);

    // Encryption setup
    let encryption = opts.passphrase.as_ref().map(|pass| {
        let archive_key = encrypt::generate_key();
        let encap = KeyEncapsulation::from_passphrase(&archive_key, pass).unwrap();
        (archive_key, encap)
    });
    let encrypted = encryption.is_some();

    let mut flags = if encrypted { FLAG_ENCRYPTED } else { 0 };
    if opts.ecc_level.is_some() {
        flags |= FLAG_ERASURE_CODED;
    }
    let header = ArchiveHeader::new(flags);
    let mut header_bytes = Vec::with_capacity(ARCHIVE_HEADER_SIZE);
    header.write_to(&mut header_bytes)?;
    writer.write_all(&header_bytes)?;

    let mut current_offset = ARCHIVE_HEADER_SIZE as u64;

    if let Some((_, ref encap)) = encryption {
        let mut encap_bytes = Vec::new();
        encap.write_to(&mut encap_bytes)?;
        writer.write_all(&encap_bytes)?;
        current_offset += encap_bytes.len() as u64;
    }

    let mut dedup = DedupStore::new();
    let mut entries: Vec<FileEntry> = Vec::with_capacity(256);
    let mut block_hashes: Vec<Hash> = Vec::new();

    let mut stats = CreateStats {
        file_count: 0,
        dir_count: 0,
        total_input_size: 0,
        total_compressed_size: 0,
        block_count: 0,
        unique_blocks: 0,
        dedup_savings: 0,
        archive_size: 0,
        parity_blocks: 0,
    };

    // ECC group buffer
    let mut ecc_group = opts.ecc_level.as_ref().map(|_| EccGroup::new());

    for batch in walk_entries.chunks(batch_size) {
        // Process batch in parallel, preserving walk order
        let processed: Vec<Result<ProcessedFile>> = batch.par_iter().map(process_one).collect();

        for result in processed {
            let mut pf = result?;

            match &pf.entry.file_type {
                FileType::Directory => stats.dir_count += 1,
                FileType::File => {
                    stats.file_count += 1;
                    stats.total_input_size += pf.entry.size;
                }
                FileType::Symlink(_) | FileType::Hardlink(_) => {
                    stats.file_count += 1;
                }
            }

            for chunk in pf.chunks {
                stats.block_count += 1;

                if !encrypted && let Some(existing_offset) = dedup.get(&chunk.hash) {
                    stats.dedup_savings += chunk.uncompressed_size as u64;
                    pf.entry.block_refs.push(BlockRef {
                        hash: chunk.hash,
                        offset: existing_offset,
                        slice_start: 0,
                        slice_len: chunk.uncompressed_size,
                        flags: 0,
                        reserved: [0; 3],
                    });

                    if let Some(ref p) = progress {
                        p.stats.blocks_deduped.fetch_add(1, Ordering::Relaxed);
                        p.stats
                            .dedup_savings
                            .fetch_add(chunk.uncompressed_size as u64, Ordering::Relaxed);
                        p.inc_compressed(chunk.uncompressed_size as u64);
                    }
                    continue;
                }

                let write_data = if let Some((ref key, _)) = encryption {
                    encrypt::encrypt_block(&chunk.compressed_data, key, &chunk.hash)?
                } else {
                    chunk.compressed_data
                };

                let block_header = BlockHeader::new(
                    chunk.hash,
                    write_data.len() as u32,
                    chunk.uncompressed_size,
                    chunk.codec,
                );

                let block_offset = current_offset;
                block_header.write_to(&mut writer)?;
                writer.write_all(&write_data)?;

                current_offset += BLOCK_HEADER_SIZE as u64 + write_data.len() as u64;
                stats.total_compressed_size += write_data.len() as u64;
                stats.unique_blocks += 1;

                dedup.insert(chunk.hash, block_offset);
                block_hashes.push(chunk.hash);

                pf.entry.block_refs.push(BlockRef {
                    hash: chunk.hash,
                    offset: block_offset,
                    slice_start: 0,
                    slice_len: chunk.uncompressed_size,
                    flags: 0,
                    reserved: [0; 3],
                });

                if let Some(ref p) = progress {
                    p.stats
                        .bytes_written
                        .fetch_add(write_data.len() as u64, Ordering::Relaxed);
                    p.inc_compressed(chunk.uncompressed_size as u64);
                }

                // Buffer shard for ECC group
                if let (Some(group), Some(level)) = (&mut ecc_group, &opts.ecc_level) {
                    group.add_shard(write_data);

                    if group.len() >= level.data_shards {
                        current_offset = flush_ecc_group(
                            group,
                            level,
                            &mut writer,
                            current_offset,
                            &mut block_hashes,
                            &mut stats,
                        )?;
                        *group = EccGroup::new();
                    }
                }
            }

            entries.push(pf.entry);
        }
    } // end batch loop

    // Flush any remaining partial ECC group
    if let (Some(group), Some(level)) = (&mut ecc_group, &opts.ecc_level)
        && !group.is_empty()
    {
        current_offset = flush_ecc_group(
            group,
            level,
            &mut writer,
            current_offset,
            &mut block_hashes,
            &mut stats,
        )?;
    }

    if let Some(ref p) = progress {
        p.start_finishing();
    }

    // Write index
    let (index_data, index_hash) = serialize_index(&entries)?;
    let index_offset = current_offset;
    let index_length = index_data.len() as u64;
    writer.write_all(&index_data)?;
    current_offset += index_length;

    // Redundant index only for larger archives
    let redundant_index_offset = if stats.unique_blocks > 100 {
        let offset = current_offset;
        writer.write_all(&index_data)?;
        current_offset += index_length;
        offset
    } else {
        index_offset
    };

    let root_hash = merkle_root(&header_bytes, &block_hashes, &index_hash);

    let footer = Footer::new(
        index_offset,
        index_length,
        redundant_index_offset,
        stats.unique_blocks,
        root_hash,
    );
    footer.write_to(&mut writer)?;
    current_offset += FOOTER_SIZE as u64;

    writer.flush()?;
    stats.archive_size = current_offset;

    if let Some(ref p) = progress {
        p.finish();
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "Hello, tardigrade!").unwrap();
        fs::write(dir.path().join("world.txt"), "World data here.").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        fs::write(dir.path().join("subdir/nested.txt"), "Nested file content.").unwrap();
        dir
    }

    #[test]
    fn create_archive_basic() {
        let test_dir = create_test_dir();
        let archive_path = test_dir.path().join("test.tg");

        let stats =
            create_archive(&archive_path, &[test_dir.path()], &CreateOptions::default()).unwrap();

        assert!(archive_path.exists());
        assert!(stats.archive_size > 0);
        assert!(stats.file_count >= 3);
        assert!(stats.dir_count >= 2);
    }

    #[test]
    fn create_archive_dedup() {
        let dir = TempDir::new().unwrap();
        let data = "x".repeat(100_000);
        fs::write(dir.path().join("file1.txt"), &data).unwrap();
        fs::write(dir.path().join("file2.txt"), &data).unwrap();
        fs::write(dir.path().join("file3.txt"), &data).unwrap();

        let archive_path = dir.path().join("dedup.tg");
        let stats =
            create_archive(&archive_path, &[dir.path()], &CreateOptions::default()).unwrap();

        assert!(stats.dedup_savings > 0);
        assert_eq!(stats.unique_blocks, 1);
        assert_eq!(stats.block_count, 3);
    }

    #[test]
    fn archive_starts_with_magic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "data").unwrap();
        let archive_path = dir.path().join("magic.tg");

        create_archive(&archive_path, &[dir.path()], &CreateOptions::default()).unwrap();

        let bytes = fs::read(&archive_path).unwrap();
        assert_eq!(&bytes[..4], b"TRDG");
    }

    #[test]
    fn parallel_archive_matches_content() {
        let dir = TempDir::new().unwrap();
        for i in 0..20 {
            fs::write(
                dir.path().join(format!("file_{i}.txt")),
                format!("content of file {i}").repeat(100),
            )
            .unwrap();
        }

        let archive_path = dir.path().join("parallel.tg");
        let stats =
            create_archive(&archive_path, &[dir.path()], &CreateOptions::default()).unwrap();

        assert_eq!(stats.file_count, 20);

        let dest = TempDir::new().unwrap();
        crate::extract::extract_archive(&archive_path, dest.path()).unwrap();

        for i in 0..20 {
            let content = fs::read_to_string(dest.path().join(format!("file_{i}.txt"))).unwrap();
            assert_eq!(content, format!("content of file {i}").repeat(100));
        }
    }

    #[test]
    fn small_archive_no_redundant_index() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("tiny.txt"), "small").unwrap();

        let archive_path = dir.path().join("small.tg");
        create_archive(&archive_path, &[dir.path()], &CreateOptions::default()).unwrap();

        let file = std::fs::File::open(&archive_path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let _header = ArchiveHeader::read_from(&mut reader).unwrap();
        let footer = crate::extract::read_footer(&mut reader).unwrap();
        assert_eq!(footer.index_offset, footer.redundant_index_offset);
    }

    #[test]
    fn small_files_skip_fastcdc() {
        // Files under 64KB should produce exactly one chunk each
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("tiny.txt"), "hello").unwrap();
        fs::write(dir.path().join("medium.txt"), "x".repeat(1000)).unwrap();

        let archive_path = dir.path().join("small.tg");
        let stats =
            create_archive(&archive_path, &[dir.path()], &CreateOptions::default()).unwrap();

        // Each small file = exactly 1 block
        assert_eq!(stats.block_count, stats.file_count);
    }

    #[test]
    fn incompressible_data_stored_raw() {
        // Random data shouldn't be compressed (would make it bigger)
        let dir = TempDir::new().unwrap();
        let mut data = vec![0u8; 10_000];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i * 31 + 17) as u8; // pseudo-random but deterministic
        }
        fs::write(dir.path().join("random.bin"), &data).unwrap();

        let archive_path = dir.path().join("raw.tg");
        create_archive(&archive_path, &[dir.path()], &CreateOptions::default()).unwrap();

        // Should still round-trip
        let dest = TempDir::new().unwrap();
        crate::extract::extract_archive(&archive_path, dest.path()).unwrap();
        let extracted = fs::read(dest.path().join("random.bin")).unwrap();
        assert_eq!(extracted, data);
    }
}
