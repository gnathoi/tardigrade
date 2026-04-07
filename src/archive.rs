use std::fs::{self, File};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use walkdir::WalkDir;

use crate::chunk::chunk_data;
use crate::compress;
use crate::dedup::DedupStore;
use crate::error::{Error, Result};
use crate::format::*;
use crate::hash::{hash_block, merkle_root};
use crate::index::serialize_index;
use crate::metadata::capture_metadata;

/// Options for archive creation
pub struct CreateOptions {
    pub codec: u8,
    pub level: i32,
    pub respect_ignore: bool,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            codec: CODEC_ZSTD,
            level: 3,
            respect_ignore: true,
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

/// Create an archive from the given source paths.
pub fn create_archive(
    archive_path: &Path,
    sources: &[&Path],
    opts: &CreateOptions,
) -> Result<CreateStats> {
    let file = File::create(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut writer = BufWriter::new(file);

    // Write archive header
    let header = ArchiveHeader::new(0); // no flags for basic archive
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

    for source in sources {
        let walker = WalkDir::new(source).follow_links(false);

        for entry in walker {
            let entry = entry.map_err(|e| {
                Error::IoPath {
                    path: source.to_path_buf(),
                    source: std::io::Error::new(std::io::ErrorKind::Other, e),
                }
            })?;

            let path = entry.path();
            let mut file_entry = capture_metadata(path, source)?;

            match &file_entry.file_type {
                FileType::Directory => {
                    stats.dir_count += 1;
                }
                FileType::File => {
                    stats.file_count += 1;
                    stats.total_input_size += file_entry.size;

                    // Read file data
                    let data =
                        fs::read(path).map_err(|e| Error::io_path(path, e))?;

                    // Chunk the data
                    let chunks = chunk_data(&data);

                    for chunk in chunks {
                        stats.block_count += 1;

                        // Check dedup
                        if let Some(existing_offset) = dedup.get(&chunk.hash) {
                            // Duplicate block — just reference the existing one
                            stats.dedup_savings += chunk.data.len() as u64;
                            file_entry.block_refs.push(BlockRef {
                                hash: chunk.hash,
                                offset: existing_offset,
                                slice_start: 0,
                                slice_len: chunk.data.len() as u32,
                                flags: 0,
                                reserved: [0; 3],
                            });
                        } else {
                            // New unique block — compress and write
                            let compressed =
                                compress::compress(&chunk.data, opts.codec, opts.level)?;

                            let block_header = BlockHeader::new(
                                chunk.hash,
                                compressed.len() as u32,
                                chunk.data.len() as u32,
                                opts.codec,
                            );

                            // Record offset before writing
                            let block_offset = current_offset;

                            // Write block header + data
                            block_header.write_to(&mut writer)?;
                            writer.write_all(&compressed)?;

                            current_offset +=
                                BLOCK_HEADER_SIZE as u64 + compressed.len() as u64;
                            stats.total_compressed_size += compressed.len() as u64;
                            stats.unique_blocks += 1;

                            // Register in dedup store
                            dedup.insert(chunk.hash, block_offset);
                            block_hashes.push(chunk.hash);

                            file_entry.block_refs.push(BlockRef {
                                hash: chunk.hash,
                                offset: block_offset,
                                slice_start: 0,
                                slice_len: chunk.data.len() as u32,
                                flags: 0,
                                reserved: [0; 3],
                            });
                        }
                    }
                }
                FileType::Symlink(_) => {
                    stats.file_count += 1;
                }
                FileType::Hardlink(_) => {
                    stats.file_count += 1;
                }
            }

            entries.push(file_entry);
        }
    }

    // Serialize and write index
    let (index_data, index_hash) = serialize_index(&entries)?;
    let index_offset = current_offset;
    let index_length = index_data.len() as u64;
    writer.write_all(&index_data)?;
    current_offset += index_length;

    // Write redundant index copy
    let redundant_index_offset = current_offset;
    writer.write_all(&index_data)?;
    current_offset += index_length;

    // Compute Merkle root (includes header bytes)
    let root_hash = merkle_root(&header_bytes, &block_hashes, &index_hash);

    // Write footer
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
        // 3 files: hello.txt, world.txt, subdir/nested.txt
        assert!(stats.file_count >= 3);
        // at least root + subdir
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

        // With dedup, we should have saved space
        assert!(stats.dedup_savings > 0);
        // Only one unique block for the identical content
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
}
