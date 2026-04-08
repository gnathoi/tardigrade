/// Append-only temporal archive support.
///
/// In temporal mode, each `tdg create --append` appends a new generation:
/// - New/changed blocks are written after the previous generation
/// - A new index + footer are appended
/// - Each footer has prev_footer_offset pointing to the prior generation
///
/// `tdg log archive.tg` scans backward through footer chain to list generations.
/// `tdg extract archive.tg --generation N` reads the Nth generation's index.
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
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
use crate::extract::read_footer;
use crate::format::*;
use crate::hash::{hash_block, merkle_root};
use crate::index::{deserialize_index, serialize_index};
use crate::metadata::{capture_metadata, restore_metadata, validate_extraction_path};

const MIN_CHUNK_SIZE: usize = 64 * 1024;
const MIN_COMPRESS_SIZE: usize = 64;
const WRITE_BUFFER_SIZE: usize = 256 * 1024;

/// A snapshot (generation) in a temporal archive
#[derive(Debug)]
pub struct Snapshot {
    pub generation: u64,
    pub footer: Footer,
    #[allow(dead_code)]
    pub footer_offset: u64,
    pub file_count: usize,
    pub dir_count: usize,
    pub total_size: u64,
}

/// List all snapshots in a temporal archive by walking the footer chain.
pub fn list_snapshots(archive_path: &Path) -> Result<Vec<Snapshot>> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    let file_size = reader.seek(SeekFrom::End(0))?;
    let mut snapshots = Vec::new();
    let mut footer_offset = file_size - FOOTER_SIZE as u64;
    let mut generation = 0u64;

    loop {
        reader.seek(SeekFrom::Start(footer_offset))?;
        let footer = match Footer::read_from(&mut reader) {
            Ok(f) => f,
            Err(_) => break,
        };

        // Read index to get file count and stats
        reader.seek(SeekFrom::Start(footer.index_offset))?;
        let mut index_data = vec![0u8; footer.index_length as usize];
        reader.read_exact(&mut index_data)?;
        let entries =
            deserialize_index(&index_data, footer.index_length as usize * 10).unwrap_or_default();

        let file_count = entries
            .iter()
            .filter(|e| matches!(e.file_type, FileType::File))
            .count();
        let dir_count = entries
            .iter()
            .filter(|e| matches!(e.file_type, FileType::Directory))
            .count();
        let total_size: u64 = entries
            .iter()
            .filter(|e| matches!(e.file_type, FileType::File))
            .map(|e| e.size)
            .sum();

        snapshots.push(Snapshot {
            generation,
            footer,
            footer_offset,
            file_count,
            dir_count,
            total_size,
        });

        generation += 1;

        if footer.prev_footer_offset == 0 {
            break;
        }
        footer_offset = footer.prev_footer_offset;
    }

    snapshots.reverse(); // oldest first
    // Fix generation numbers: 0 = oldest, N = newest
    for (i, snap) in snapshots.iter_mut().enumerate() {
        snap.generation = i as u64;
    }
    Ok(snapshots)
}

/// Append a new generation to an existing archive.
pub fn append_archive(
    archive_path: &Path,
    sources: &[&Path],
    opts: &CreateOptions,
) -> Result<AppendStats> {
    // Read existing archive to get current state
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    let _header = ArchiveHeader::read_from(&mut reader)?;
    let file_size = reader.seek(SeekFrom::End(0))?;
    let old_footer_offset = file_size - FOOTER_SIZE as u64;

    // Read old footer to get existing block info
    let old_footer = read_footer(&mut reader)?;

    // Build dedup store from existing blocks
    reader.seek(SeekFrom::Start(old_footer.index_offset))?;
    let mut index_data = vec![0u8; old_footer.index_length as usize];
    reader.read_exact(&mut index_data)?;
    let old_entries = deserialize_index(&index_data, old_footer.index_length as usize * 10)?;

    let mut dedup = DedupStore::new();
    for entry in &old_entries {
        for bref in &entry.block_refs {
            dedup.insert(bref.hash, bref.offset);
        }
    }

    drop(reader);

    // Walk source files
    let mut walk_entries: Vec<(std::path::PathBuf, std::path::PathBuf, u64)> = Vec::new();
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
            walk_entries.push((path, source.to_path_buf(), size));
        }
    }

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

    // Open archive for appending
    let file = OpenOptions::new()
        .write(true)
        .open(archive_path)
        .map_err(|e| Error::io_path(archive_path, e))?;
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);

    // Seek to end of blocks section (before old index)
    // We append after all existing data (including old index/footer)
    writer.seek(SeekFrom::Start(file_size))?;
    let mut current_offset = file_size;

    let mut entries: Vec<FileEntry> = Vec::with_capacity(walk_entries.len());
    let mut block_hashes: Vec<Hash> = Vec::new();

    let mut stats = AppendStats {
        file_count: 0,
        dir_count: 0,
        total_input_size: 0,
        new_blocks: 0,
        reused_blocks: 0,
        generation: 0,
    };

    // Count existing generations
    let snapshots = {
        let f = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
        let mut r = BufReader::new(f);
        let size = r.seek(SeekFrom::End(0))?;
        let mut count = 0u64;
        let mut foff = size - FOOTER_SIZE as u64;
        loop {
            r.seek(SeekFrom::Start(foff))?;
            let ft = match Footer::read_from(&mut r) {
                Ok(f) => f,
                Err(_) => break,
            };
            count += 1;
            if ft.prev_footer_offset == 0 {
                break;
            }
            foff = ft.prev_footer_offset;
        }
        count
    };
    stats.generation = snapshots;

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

        // Tag with snapshot generation
        pf.entry.snapshot_id = Some(stats.generation);

        for chunk in pf.chunks {
            // Check dedup against existing blocks
            if let Some(existing_offset) = dedup.get(&chunk.hash) {
                stats.reused_blocks += 1;
                pf.entry.block_refs.push(BlockRef {
                    hash: chunk.hash,
                    offset: existing_offset,
                    slice_start: 0,
                    slice_len: chunk.uncompressed_size,
                    flags: 0,
                    reserved: [0; 3],
                });
                continue;
            }

            // Write new block
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
        }

        entries.push(pf.entry);
    }

    // Write new index
    let (index_data, index_hash) = serialize_index(&entries)?;
    let index_offset = current_offset;
    let index_length = index_data.len() as u64;
    writer.write_all(&index_data)?;

    // Use a dummy header for merkle root (reuse archive header)
    let header_bytes = vec![0u8; ARCHIVE_HEADER_SIZE]; // placeholder
    let root_hash = merkle_root(&header_bytes, &block_hashes, &index_hash);

    let mut footer = Footer::new(
        index_offset,
        index_length,
        index_offset, // no redundant index for appended generations
        old_footer.block_count + stats.new_blocks,
        root_hash,
    );
    footer.prev_footer_offset = old_footer_offset;

    footer.write_to(&mut writer)?;
    writer.flush()?;

    // Now update the archive header to set FLAG_APPEND_ONLY if not already
    {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(archive_path)
            .map_err(|e| Error::io_path(archive_path, e))?;
        let mut rw = BufReader::new(file);
        let header = ArchiveHeader::read_from(&mut rw)?;
        if !header.is_append_only() {
            let new_header = ArchiveHeader::new(header.flags | FLAG_APPEND_ONLY);
            let file = rw.into_inner();
            let mut writer = BufWriter::new(file);
            writer.seek(SeekFrom::Start(0))?;
            new_header.write_to(&mut writer)?;
            writer.flush()?;
        }
    }

    Ok(stats)
}

/// Extract a specific generation from a temporal archive.
pub fn extract_generation(
    archive_path: &Path,
    generation: u64,
    dest: &Path,
) -> Result<crate::extract::ExtractStats> {
    let snapshots = list_snapshots(archive_path)?;

    if snapshots.is_empty() {
        return Err(Error::NoSnapshots);
    }

    let snapshot = snapshots
        .iter()
        .find(|s| s.generation == generation)
        .ok_or_else(|| {
            Error::InvalidArchive(format!(
                "generation {} not found (archive has {} generations)",
                generation,
                snapshots.len()
            ))
        })?;

    // Read the generation's index
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    reader.seek(SeekFrom::Start(snapshot.footer.index_offset))?;
    let mut index_data = vec![0u8; snapshot.footer.index_length as usize];
    reader.read_exact(&mut index_data)?;
    let entries = deserialize_index(&index_data, snapshot.footer.index_length as usize * 10)?;

    // Extract using the generation's file entries
    // Blocks may be from any generation (deduped)
    fs::create_dir_all(dest).map_err(|e| Error::io_path(dest, e))?;

    let mut stats = crate::extract::ExtractStats {
        file_count: 0,
        dir_count: 0,
        total_size: 0,
    };

    let mut block_cache: HashMap<u64, Arc<Vec<u8>>> = HashMap::new();

    // Directories first
    for entry in &entries {
        if entry.file_type == FileType::Directory {
            let target = validate_extraction_path(&entry.path, dest)?;
            fs::create_dir_all(&target).map_err(|e| Error::io_path(&target, e))?;
            stats.dir_count += 1;
        }
    }

    // Files
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
            let block_data = get_block(&mut reader, &mut block_cache, bref.offset)?;
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

    // Directory metadata
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

#[derive(Debug)]
pub struct AppendStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub total_input_size: u64,
    pub new_blocks: u64,
    pub reused_blocks: u64,
    pub generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{CreateOptions, create_archive};

    #[test]
    fn append_and_list_generations() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create initial archive
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("v1.txt"), "version 1").unwrap();

        let archive_path = dir.path().join("temporal.tg");
        create_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();

        // Append generation 1
        std::fs::write(src.join("v2.txt"), "version 2").unwrap();
        let stats =
            append_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();
        assert_eq!(stats.generation, 1);

        // Append generation 2
        std::fs::write(src.join("v3.txt"), "version 3").unwrap();
        let stats =
            append_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();
        assert_eq!(stats.generation, 2);

        // List generations
        let snapshots = list_snapshots(&archive_path).unwrap();
        assert_eq!(snapshots.len(), 3); // gen 0 + gen 1 + gen 2
    }

    #[test]
    fn extract_specific_generation() {
        let dir = tempfile::TempDir::new().unwrap();

        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), "gen 0 content").unwrap();

        let archive_path = dir.path().join("temporal.tg");
        create_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();

        // Append with different content
        std::fs::write(src.join("file.txt"), "gen 1 content").unwrap();
        std::fs::write(src.join("new.txt"), "new in gen 1").unwrap();
        append_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();

        // Extract generation 0
        let dest0 = dir.path().join("gen0");
        extract_generation(&archive_path, 0, &dest0).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest0.join("file.txt")).unwrap(),
            "gen 0 content"
        );
        assert!(!dest0.join("new.txt").exists());

        // Extract generation 1
        let dest1 = dir.path().join("gen1");
        extract_generation(&archive_path, 1, &dest1).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest1.join("file.txt")).unwrap(),
            "gen 1 content"
        );
        assert_eq!(
            std::fs::read_to_string(dest1.join("new.txt")).unwrap(),
            "new in gen 1"
        );
    }

    #[test]
    fn append_dedup_across_generations() {
        let dir = tempfile::TempDir::new().unwrap();

        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let big_data = "shared data".repeat(5000);
        std::fs::write(src.join("big.txt"), &big_data).unwrap();

        let archive_path = dir.path().join("dedup.tg");
        create_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();

        let size_before = std::fs::metadata(&archive_path).unwrap().len();

        // Append same content — blocks should be reused
        let stats =
            append_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();

        assert!(stats.reused_blocks > 0);

        let size_after = std::fs::metadata(&archive_path).unwrap().len();
        // Size increase should be small (just new index + footer, no new blocks)
        let growth = size_after - size_before;
        assert!(growth < 2048, "grew by {growth} bytes, expected < 2048");
    }
}
