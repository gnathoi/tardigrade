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
use crate::error::{Error, Result};
use crate::format::*;
use crate::hash::merkle_root;
use crate::index::serialize_index;
use crate::metadata::capture_metadata;
use crate::progress::CreateProgress;

/// Options for archive creation
pub struct CreateOptions {
    pub codec: u8,
    pub level: i32,
    pub show_progress: bool,
    pub respect_gitignore: bool,
    pub passphrase: Option<Vec<u8>>,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            codec: CODEC_ZSTD,
            level: 3,
            show_progress: false,
            respect_gitignore: true,
            passphrase: None,
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

/// Threshold below which we skip rayon (thread pool overhead not worth it)
const PARALLEL_THRESHOLD: u64 = 10 * 1024 * 1024; // 10 MB

/// Create an archive from the given source paths.
pub fn create_archive(
    archive_path: &Path,
    sources: &[&Path],
    opts: &CreateOptions,
) -> Result<CreateStats> {
    // Phase 1: Walk and collect all file paths
    let mut walk_entries = Vec::new();
    for source in sources {
        let walker = WalkBuilder::new(source)
            .git_ignore(opts.respect_gitignore)
            .git_global(opts.respect_gitignore)
            .git_exclude(opts.respect_gitignore)
            .hidden(false)
            .filter_entry(|e| {
                !(e.file_type().is_some_and(|ft| ft.is_dir()) && e.file_name() == ".git")
            })
            .follow_links(false)
            .build();

        for entry in walker {
            let entry = entry.map_err(|e| Error::IoPath {
                path: source.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e),
            })?;
            walk_entries.push((entry.path().to_path_buf(), source.to_path_buf()));
        }
    }

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

    // Phase 2: Read, chunk, and compress files
    // Adaptive: use rayon only when data is large enough to benefit
    let codec = opts.codec;
    let level = opts.level;
    let use_parallel = total_bytes >= PARALLEL_THRESHOLD;

    let process_one =
        |(path, source): &(std::path::PathBuf, std::path::PathBuf)| -> Result<ProcessedFile> {
            let file_entry = capture_metadata(path, source)?;

            let chunks = match &file_entry.file_type {
                FileType::File => {
                    let data = fs::read(path).map_err(|e| Error::io_path(path, e))?;
                    if data.is_empty() {
                        vec![]
                    } else {
                        let raw_chunks = chunk_data(&data);
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
                }
                _ => vec![],
            };

            Ok(ProcessedFile {
                entry: file_entry,
                chunks,
            })
        };

    let processed: Vec<Result<ProcessedFile>> = if use_parallel {
        walk_entries.par_iter().map(process_one).collect()
    } else {
        walk_entries.iter().map(process_one).collect()
    };

    // Phase 3: Sequential write with dedup
    let file = File::create(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut writer = BufWriter::new(file);

    // Encryption setup
    let encryption = opts.passphrase.as_ref().map(|pass| {
        let archive_key = encrypt::generate_key();
        let encap = KeyEncapsulation::from_passphrase(&archive_key, pass).unwrap();
        (archive_key, encap)
    });
    let encrypted = encryption.is_some();

    let flags = if encrypted { FLAG_ENCRYPTED } else { 0 };
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
    let mut entries: Vec<FileEntry> = Vec::new();
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

            if !encrypted {
                if let Some(existing_offset) = dedup.get(&chunk.hash) {
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

    // Redundant index only for larger archives (saves overhead on small ones)
    let redundant_index_offset = if stats.unique_blocks > 100 {
        let offset = current_offset;
        writer.write_all(&index_data)?;
        current_offset += index_length;
        offset
    } else {
        index_offset // point to same location
    };

    // Merkle root
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

        // Read footer and check redundant index points to same location
        let file = std::fs::File::open(&archive_path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let _header = ArchiveHeader::read_from(&mut reader).unwrap();
        let footer = crate::extract::read_footer(&mut reader).unwrap();
        assert_eq!(footer.index_offset, footer.redundant_index_offset);
    }
}
