use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::Ordering;

use rayon::prelude::*;
use walkdir::WalkDir;

use crate::chunk::{chunk_data, Chunk};
use crate::compress;
use crate::dedup::DedupStore;
use crate::error::{Error, Result};
use crate::format::*;
use crate::hash::merkle_root;
use crate::index::serialize_index;
use crate::metadata::capture_metadata;
use crate::progress::{CreateProgress, ProgressStats};

/// Options for archive creation
pub struct CreateOptions {
    pub codec: u8,
    pub level: i32,
    pub show_progress: bool,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            codec: CODEC_ZSTD,
            level: 3,
            show_progress: false,
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
}

/// A file that has been read and chunked, ready for dedup + write
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

/// Create an archive from the given source paths.
pub fn create_archive(
    archive_path: &Path,
    sources: &[&Path],
    opts: &CreateOptions,
) -> Result<CreateStats> {
    // Phase 1: Walk and collect all file paths
    let mut walk_entries = Vec::new();
    for source in sources {
        let walker = WalkDir::new(source).follow_links(false);
        for entry in walker {
            let entry = entry.map_err(|e| Error::IoPath {
                path: source.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e),
            })?;
            walk_entries.push((entry.path().to_path_buf(), source.to_path_buf()));
        }
    }

    // Scan total size for progress bar
    let total_bytes: u64 = walk_entries
        .iter()
        .filter_map(|(path, _)| fs::metadata(path).ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();

    let progress = if opts.show_progress {
        Some(CreateProgress::new(total_bytes))
    } else {
        None
    };

    // Phase 2: Read, chunk, and compress files in parallel
    let codec = opts.codec;
    let level = opts.level;

    let processed: Vec<Result<ProcessedFile>> = walk_entries
        .par_iter()
        .map(|(path, source)| {
            let mut file_entry = capture_metadata(path, source)?;

            let chunks = match &file_entry.file_type {
                FileType::File => {
                    let data = fs::read(path).map_err(|e| Error::io_path(path, e))?;
                    let raw_chunks = chunk_data(&data);

                    // Compress each chunk in parallel (already within a rayon task)
                    raw_chunks
                        .into_iter()
                        .map(|chunk| {
                            let compressed = compress::compress(&chunk.data, codec, level)?;
                            Ok(CompressedChunk {
                                hash: chunk.hash,
                                uncompressed_size: chunk.data.len() as u32,
                                compressed_data: compressed,
                                codec,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?
                }
                _ => vec![],
            };

            Ok(ProcessedFile {
                entry: file_entry,
                chunks,
            })
        })
        .collect();

    // Phase 3: Sequential write with dedup
    let file = File::create(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut writer = BufWriter::new(file);

    let header = ArchiveHeader::new(0);
    let mut header_bytes = Vec::with_capacity(ARCHIVE_HEADER_SIZE);
    header.write_to(&mut header_bytes)?;
    writer.write_all(&header_bytes)?;

    let mut dedup = DedupStore::new();
    let mut entries: Vec<FileEntry> = Vec::new();
    let mut block_hashes: Vec<Hash> = Vec::new();
    let mut current_offset = ARCHIVE_HEADER_SIZE as u64;

    let mut stats = CreateStats {
        file_count: 0,
        dir_count: 0,
        total_input_size: 0,
        total_compressed_size: 0,
        block_count: 0,
        unique_blocks: 0,
        dedup_savings: 0,
        archive_size: 0,
    };

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

            if let Some(existing_offset) = dedup.get(&chunk.hash) {
                // Deduplicated block
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
            } else {
                // New unique block
                let block_header = BlockHeader::new(
                    chunk.hash,
                    chunk.compressed_data.len() as u32,
                    chunk.uncompressed_size,
                    chunk.codec,
                );

                let block_offset = current_offset;
                block_header.write_to(&mut writer)?;
                writer.write_all(&chunk.compressed_data)?;

                current_offset += BLOCK_HEADER_SIZE as u64 + chunk.compressed_data.len() as u64;
                stats.total_compressed_size += chunk.compressed_data.len() as u64;
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
                        .fetch_add(chunk.compressed_data.len() as u64, Ordering::Relaxed);
                    p.inc_compressed(chunk.uncompressed_size as u64);
                }
            }
        }

        entries.push(pf.entry);
    }

    if let Some(ref p) = progress {
        p.finish_scan();
    }

    // Write index
    let (index_data, index_hash) = serialize_index(&entries)?;
    let index_offset = current_offset;
    let index_length = index_data.len() as u64;
    writer.write_all(&index_data)?;
    current_offset += index_length;

    // Write redundant index
    let redundant_index_offset = current_offset;
    writer.write_all(&index_data)?;
    current_offset += index_length;

    // Merkle root
    let root_hash = merkle_root(&header_bytes, &block_hashes, &index_hash);

    // Footer
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
        fs::write(
            dir.path().join("subdir/nested.txt"),
            "Nested file content.",
        )
        .unwrap();
        dir
    }

    #[test]
    fn create_archive_basic() {
        let test_dir = create_test_dir();
        let archive_path = test_dir.path().join("test.tg");

        let stats = create_archive(
            &archive_path,
            &[test_dir.path()],
            &CreateOptions::default(),
        )
        .unwrap();

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
        let stats = create_archive(
            &archive_path,
            &[dir.path()],
            &CreateOptions::default(),
        )
        .unwrap();

        assert!(stats.dedup_savings > 0);
        assert_eq!(stats.unique_blocks, 1);
        assert_eq!(stats.block_count, 3);
    }

    #[test]
    fn archive_starts_with_magic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "data").unwrap();
        let archive_path = dir.path().join("magic.tg");

        create_archive(
            &archive_path,
            &[dir.path()],
            &CreateOptions::default(),
        )
        .unwrap();

        let bytes = fs::read(&archive_path).unwrap();
        assert_eq!(&bytes[..4], b"TRDG");
    }

    #[test]
    fn parallel_archive_matches_content() {
        let dir = TempDir::new().unwrap();
        // Create enough data to exercise parallelism
        for i in 0..20 {
            fs::write(
                dir.path().join(format!("file_{i}.txt")),
                format!("content of file {i}").repeat(100),
            )
            .unwrap();
        }

        let archive_path = dir.path().join("parallel.tg");
        let stats = create_archive(
            &archive_path,
            &[dir.path()],
            &CreateOptions::default(),
        )
        .unwrap();

        assert_eq!(stats.file_count, 20);

        // Extract and verify
        let dest = TempDir::new().unwrap();
        crate::extract::extract_archive(&archive_path, dest.path()).unwrap();

        for i in 0..20 {
            let content = fs::read_to_string(dest.path().join(format!("file_{i}.txt"))).unwrap();
            assert_eq!(content, format!("content of file {i}").repeat(100));
        }
    }
}
