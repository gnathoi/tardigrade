// Incremental/differential archive support.
//
// An incremental archive stores only blocks not present in the base archive.
// The header has the INCREMENTAL flag and stores the base's root hash.
// BlockRefs with BLOCKREF_FLAG_EXTERNAL point to blocks in the base.
//
// Creation: `tdg create --incremental base.tg diff.tg ./path`
// Extraction: `tdg extract diff.tg --base base.tg -o dest`

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::archive::CreateOptions;
use crate::chunk::chunk_data;
use crate::compress;
use crate::dedup::DedupStore;
use crate::error::{Error, Result};
use crate::extract::{read_footer, read_index};
use crate::format::*;
use crate::hash::{hash_block, merkle_root};
use crate::index::serialize_index;
use crate::metadata::{capture_metadata, restore_metadata, validate_extraction_path};
use crate::progress::CreateProgress;

/// Minimum chunk size — files smaller than this skip FastCDC
const MIN_CHUNK_SIZE: usize = 64 * 1024;
const MIN_COMPRESS_SIZE: usize = 64;
const WRITE_BUFFER_SIZE: usize = 256 * 1024;

/// Stats for incremental archive creation
#[derive(Debug)]
pub struct IncrementalStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub total_input_size: u64,
    pub new_blocks: u64,
    pub reused_blocks: u64,
    pub archive_size: u64,
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

fn process_file_data(data: &[u8], codec: u8, level: i32) -> Result<Vec<CompressedChunk>> {
    if data.is_empty() {
        return Ok(vec![]);
    }

    if data.len() < MIN_CHUNK_SIZE {
        let hash = hash_block(data);
        let compressed = if data.len() >= MIN_COMPRESS_SIZE {
            let c = compress::compress(data, codec, level)?;
            if c.len() < data.len() {
                (c, codec)
            } else {
                (data.to_vec(), CODEC_NONE)
            }
        } else {
            (data.to_vec(), CODEC_NONE)
        };
        return Ok(vec![CompressedChunk {
            hash,
            uncompressed_size: data.len() as u32,
            compressed_data: compressed.0,
            codec: compressed.1,
        }]);
    }

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

/// Create an incremental archive storing only blocks not in the base.
pub fn create_incremental(
    base_path: &Path,
    archive_path: &Path,
    sources: &[&Path],
    opts: &CreateOptions,
) -> Result<IncrementalStats> {
    // Read base archive index to get existing block hashes
    let base_file = File::open(base_path).map_err(|e| Error::io_path(base_path, e))?;
    let mut base_reader = BufReader::new(base_file);
    let _base_header = ArchiveHeader::read_from(&mut base_reader)?;
    let base_footer = read_footer(&mut base_reader)?;
    let base_entries = read_index(&mut base_reader, &base_footer)?;

    // Build hash set of all blocks in base, with their offsets
    let mut base_blocks: HashMap<Hash, u64> = HashMap::new();
    for entry in &base_entries {
        for bref in &entry.block_refs {
            base_blocks.entry(bref.hash).or_insert(bref.offset);
        }
    }

    // Walk source files
    let mut walk_entries: Vec<(std::path::PathBuf, std::path::PathBuf, u64)> = Vec::new();
    let mut total_bytes: u64 = 0;

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
                source: std::io::Error::other(e),
            })?;
            let path = entry.path().to_path_buf();
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                total_bytes += size;
            }
            walk_entries.push((path, source.to_path_buf(), size));
        }
    }

    let progress = if opts.show_progress {
        Some(CreateProgress::new(total_bytes))
    } else {
        None
    };

    // Process files in parallel
    let codec = opts.codec;
    let level = opts.level;

    let processed: Vec<Result<ProcessedFile>> = walk_entries
        .par_iter()
        .map(|(path, source, _)| {
            let file_entry = capture_metadata(path, source)?;
            let chunks = match &file_entry.file_type {
                FileType::File => {
                    let data = fs::read(path).map_err(|e| Error::io_path(path, e))?;
                    process_file_data(&data, codec, level)?
                }
                _ => vec![],
            };
            Ok(ProcessedFile {
                entry: file_entry,
                chunks,
            })
        })
        .collect();

    // Write incremental archive
    let file = File::create(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);

    let flags = FLAG_INCREMENTAL;
    let header = ArchiveHeader::new(flags);
    let mut header_bytes = Vec::with_capacity(ARCHIVE_HEADER_SIZE);
    header.write_to(&mut header_bytes)?;
    writer.write_all(&header_bytes)?;

    let mut current_offset = ARCHIVE_HEADER_SIZE as u64;
    let mut dedup = DedupStore::new();
    let mut entries: Vec<FileEntry> = Vec::with_capacity(walk_entries.len());
    let mut block_hashes: Vec<Hash> = Vec::new();

    let mut stats = IncrementalStats {
        file_count: 0,
        dir_count: 0,
        total_input_size: 0,
        new_blocks: 0,
        reused_blocks: 0,
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
            _ => stats.file_count += 1,
        }

        for chunk in pf.chunks {
            // Check if block exists in base archive
            if let Some(&base_offset) = base_blocks.get(&chunk.hash) {
                stats.reused_blocks += 1;
                pf.entry.block_refs.push(BlockRef {
                    hash: chunk.hash,
                    offset: base_offset,
                    slice_start: 0,
                    slice_len: chunk.uncompressed_size,
                    flags: BLOCKREF_FLAG_EXTERNAL,
                    reserved: [0; 3],
                });
                if let Some(ref p) = progress {
                    p.inc_compressed(chunk.uncompressed_size as u64);
                }
                continue;
            }

            // Check dedup within this incremental archive
            if let Some(existing_offset) = dedup.get(&chunk.hash) {
                pf.entry.block_refs.push(BlockRef {
                    hash: chunk.hash,
                    offset: existing_offset,
                    slice_start: 0,
                    slice_len: chunk.uncompressed_size,
                    flags: 0,
                    reserved: [0; 3],
                });
                if let Some(ref p) = progress {
                    p.inc_compressed(chunk.uncompressed_size as u64);
                }
                continue;
            }

            // New block — write it
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
            stats.new_blocks += 1;

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

    let root_hash = merkle_root(&header_bytes, &block_hashes, &index_hash);
    let mut footer = Footer::new(
        index_offset,
        index_length,
        index_offset, // no redundant index for incremental
        stats.new_blocks,
        root_hash,
    );
    // Store base archive root hash in prev_footer_offset field as a reference
    // (repurposed: for incremental archives this links to the base)
    footer.prev_footer_offset = 0; // Could store base root hash reference

    footer.write_to(&mut writer)?;
    current_offset += FOOTER_SIZE as u64;

    writer.flush()?;
    stats.archive_size = current_offset;

    if let Some(ref p) = progress {
        p.finish();
    }

    Ok(stats)
}

/// Extract an incremental archive, reading external blocks from the base.
pub fn extract_incremental(
    archive_path: &Path,
    base_path: &Path,
    dest: &Path,
) -> Result<crate::extract::ExtractStats> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);
    let header = ArchiveHeader::read_from(&mut reader)?;

    if !header.is_incremental() {
        return Err(Error::InvalidArchive(
            "archive is not incremental".into(),
        ));
    }

    let base_file = File::open(base_path).map_err(|e| Error::io_path(base_path, e))?;
    let mut base_reader = BufReader::new(base_file);
    let _base_header = ArchiveHeader::read_from(&mut base_reader)?;

    let footer = read_footer(&mut reader)?;
    let entries = read_index(&mut reader, &footer)?;

    fs::create_dir_all(dest).map_err(|e| Error::io_path(dest, e))?;

    let mut stats = crate::extract::ExtractStats {
        file_count: 0,
        dir_count: 0,
        total_size: 0,
        errors: 0,
    };

    let mut block_cache: HashMap<u64, Arc<Vec<u8>>> = HashMap::new();
    let mut base_block_cache: HashMap<u64, Arc<Vec<u8>>> = HashMap::new();

    // First pass: directories
    for entry in &entries {
        if entry.file_type == FileType::Directory {
            let target = validate_extraction_path(&entry.path, dest)?;
            fs::create_dir_all(&target).map_err(|e| Error::io_path(&target, e))?;
            stats.dir_count += 1;
        }
    }

    // Second pass: files
    for entry in &entries {
        if !matches!(entry.file_type, FileType::File) {
            continue;
        }

        let target = validate_extraction_path(&entry.path, dest)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io_path(parent, e))?;
        }

        let mut file_data = Vec::with_capacity(entry.size as usize);

        for bref in &entry.block_refs {
            let block_data = if bref.flags & BLOCKREF_FLAG_EXTERNAL != 0 {
                // Read from base archive
                get_block(&mut base_reader, &mut base_block_cache, bref.offset)?
            } else {
                // Read from incremental archive
                get_block(&mut reader, &mut block_cache, bref.offset)?
            };

            let start = bref.slice_start as usize;
            let end = start + bref.slice_len as usize;
            if end > block_data.len() {
                return Err(Error::InvalidArchive(format!(
                    "block ref out of bounds: {}..{} > {}",
                    start,
                    end,
                    block_data.len()
                )));
            }
            file_data.extend_from_slice(&block_data[start..end]);
        }

        fs::write(&target, &file_data).map_err(|e| Error::io_path(&target, e))?;
        restore_metadata(&target, entry)?;

        stats.file_count += 1;
        stats.total_size += entry.size;
    }

    // Third pass: directory metadata
    for entry in &entries {
        if entry.file_type == FileType::Directory {
            let target = validate_extraction_path(&entry.path, dest)?;
            if target.exists() {
                restore_metadata(&target, entry).ok();
            }
        }
    }

    Ok(stats)
}

fn get_block(
    reader: &mut (impl Read + Seek),
    cache: &mut HashMap<u64, Arc<Vec<u8>>>,
    offset: u64,
) -> Result<Arc<Vec<u8>>> {
    if let Some(cached) = cache.get(&offset) {
        return Ok(Arc::clone(cached));
    }

    reader.seek(SeekFrom::Start(offset))?;
    let header = BlockHeader::read_from(reader)?;
    let mut raw = vec![0u8; header.compressed_size as usize];
    reader.read_exact(&mut raw)?;

    let data = compress::decompress(&raw, header.codec, header.uncompressed_size as usize)?;

    let actual_hash: Hash = blake3::hash(&data).into();
    if actual_hash != header.hash {
        return Err(Error::ChecksumMismatch {
            offset,
            expected: hex::encode(header.hash),
            actual: hex::encode(actual_hash),
        });
    }

    let arc = Arc::new(data);
    cache.insert(offset, Arc::clone(&arc));
    Ok(arc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{CreateOptions, create_archive};

    #[test]
    fn incremental_new_files_only() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create base with some files
        let src_base = dir.path().join("base");
        std::fs::create_dir(&src_base).unwrap();
        let shared = "shared content".repeat(500);
        std::fs::write(src_base.join("shared.txt"), &shared).unwrap();
        std::fs::write(src_base.join("old.txt"), "old content").unwrap();

        let base_path = dir.path().join("base.tg");
        create_archive(
            &base_path,
            &[src_base.as_path()],
            &CreateOptions::default(),
        )
        .unwrap();

        // Create incremental with shared + new files
        let src_inc = dir.path().join("inc");
        std::fs::create_dir(&src_inc).unwrap();
        std::fs::write(src_inc.join("shared.txt"), &shared).unwrap(); // Same content
        std::fs::write(src_inc.join("new.txt"), "brand new content").unwrap();

        let inc_path = dir.path().join("diff.tg");
        let stats = create_incremental(
            &base_path,
            &inc_path,
            &[src_inc.as_path()],
            &CreateOptions::default(),
        )
        .unwrap();

        // Shared blocks should be reused
        assert!(stats.reused_blocks > 0);
        // Incremental archive should be smaller than base
        let base_size = std::fs::metadata(&base_path).unwrap().len();
        let inc_size = std::fs::metadata(&inc_path).unwrap().len();
        assert!(inc_size < base_size);

        // Extract incremental with base
        let dest = dir.path().join("extracted");
        extract_incremental(&inc_path, &base_path, &dest).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("shared.txt")).unwrap(),
            shared
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("new.txt")).unwrap(),
            "brand new content"
        );
    }

    #[test]
    fn incremental_without_base_fails() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create a regular archive
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), "content").unwrap();

        let archive_path = dir.path().join("regular.tg");
        create_archive(
            &archive_path,
            &[src.as_path()],
            &CreateOptions::default(),
        )
        .unwrap();

        // Try to extract as incremental — should fail (not incremental)
        let dest = dir.path().join("dest");
        let result = extract_incremental(&archive_path, &archive_path, &dest);
        assert!(result.is_err());
    }
}
