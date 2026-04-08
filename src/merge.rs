// Content-addressed archive merging.
//
// `tdg merge a.tg b.tg -o merged.tg`
//
// Block union: all unique blocks (by hash) from both archives are included.
// File tree merge: union of all paths. Conflicts resolved by newer mtime
// (last-writer-wins). If mtimes equal, left archive wins.
//
// Dedup is automatic: shared blocks are stored once.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{Error, Result};
use crate::extract::{read_footer, read_index};
use crate::format::*;
use crate::hash::merkle_root;
use crate::index::serialize_index;

/// Merge two archives into one. Dedup across archives is automatic.
pub fn merge_archives(a_path: &Path, b_path: &Path, output_path: &Path) -> Result<MergeStats> {
    // Read both archives
    let a_file = File::open(a_path).map_err(|e| Error::io_path(a_path, e))?;
    let mut a_reader = BufReader::new(a_file);
    let a_header = ArchiveHeader::read_from(&mut a_reader)?;
    let a_footer = read_footer(&mut a_reader)?;
    let a_entries = read_index(&mut a_reader, &a_footer)?;

    let b_file = File::open(b_path).map_err(|e| Error::io_path(b_path, e))?;
    let mut b_reader = BufReader::new(b_file);
    let _b_header = ArchiveHeader::read_from(&mut b_reader)?;
    let b_footer = read_footer(&mut b_reader)?;
    let b_entries = read_index(&mut b_reader, &b_footer)?;

    // Build a merged file tree: path -> FileEntry
    // Use a map keyed by path bytes
    let mut file_map: HashMap<Vec<u8>, FileEntry> = HashMap::new();

    // Insert all entries from archive A
    for entry in &a_entries {
        file_map.insert(entry.path.clone(), entry.clone());
    }

    // Merge entries from archive B (newer mtime wins)
    let mut conflicts = 0u64;
    for entry in &b_entries {
        if let Some(existing) = file_map.get(&entry.path) {
            if entry.mtime_ns > existing.mtime_ns {
                file_map.insert(entry.path.clone(), entry.clone());
                conflicts += 1;
            } else if entry.mtime_ns == existing.mtime_ns {
                // Equal mtime: left archive wins (keep A)
                conflicts += 1;
            }
            // else A's entry is newer, keep it
        } else {
            file_map.insert(entry.path.clone(), entry.clone());
        }
    }

    // Collect all unique blocks needed across both archives
    let merged_entries: Vec<FileEntry> = {
        let mut entries: Vec<FileEntry> = file_map.into_values().collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries
    };

    // Build set of unique block hashes needed
    let mut needed_blocks: HashMap<Hash, (u64, bool)> = HashMap::new(); // hash -> (offset, is_from_b)
    for entry in &merged_entries {
        for bref in &entry.block_refs {
            needed_blocks.entry(bref.hash).or_insert_with(|| {
                // Check if this block exists in A or B
                let in_a = a_entries
                    .iter()
                    .any(|e| e.block_refs.iter().any(|r| r.hash == bref.hash));
                if in_a {
                    (bref.offset, false)
                } else {
                    (bref.offset, true)
                }
            });
        }
    }

    // Write merged archive
    let out_file = File::create(output_path).map_err(|e| Error::io_path(output_path, e))?;
    let mut writer = BufWriter::new(out_file);

    let flags = a_header.flags & !(FLAG_APPEND_ONLY | FLAG_INCREMENTAL); // Clear temporal/incremental flags
    let header = ArchiveHeader::new(flags);
    let mut header_bytes = Vec::with_capacity(ARCHIVE_HEADER_SIZE);
    header.write_to(&mut header_bytes)?;
    writer.write_all(&header_bytes)?;

    let mut current_offset = ARCHIVE_HEADER_SIZE as u64;
    let mut block_hashes: Vec<Hash> = Vec::new();
    let mut new_offsets: HashMap<Hash, u64> = HashMap::new();
    let mut unique_blocks = 0u64;

    // Copy blocks, deduplicating across archives
    for (&hash, &(old_offset, is_from_b)) in &needed_blocks {
        if new_offsets.contains_key(&hash) {
            continue;
        }

        let reader: &mut BufReader<File> = if is_from_b {
            &mut b_reader
        } else {
            &mut a_reader
        };

        reader.seek(SeekFrom::Start(old_offset))?;
        let block_header = BlockHeader::read_from(reader)?;
        let mut block_data = vec![0u8; block_header.compressed_size as usize];
        reader.read_exact(&mut block_data)?;

        let new_offset = current_offset;
        block_header.write_to(&mut writer)?;
        writer.write_all(&block_data)?;

        current_offset += BLOCK_HEADER_SIZE as u64 + block_data.len() as u64;
        new_offsets.insert(hash, new_offset);
        block_hashes.push(hash);
        unique_blocks += 1;
    }

    // Rewrite entries with updated block offsets
    let mut final_entries: Vec<FileEntry> = Vec::with_capacity(merged_entries.len());
    for mut entry in merged_entries {
        for bref in &mut entry.block_refs {
            if let Some(&new_offset) = new_offsets.get(&bref.hash) {
                bref.offset = new_offset;
            }
        }
        final_entries.push(entry);
    }

    // Write index
    let (index_data, index_hash) = serialize_index(&final_entries)?;
    let index_offset = current_offset;
    let index_length = index_data.len() as u64;
    writer.write_all(&index_data)?;
    current_offset += index_length;

    // Redundant index
    let redundant_index_offset = if unique_blocks > 100 {
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
        unique_blocks,
        root_hash,
    );
    footer.write_to(&mut writer)?;

    writer.flush()?;

    let file_count = final_entries
        .iter()
        .filter(|e| matches!(e.file_type, FileType::File))
        .count() as u64;
    let dir_count = final_entries
        .iter()
        .filter(|e| matches!(e.file_type, FileType::Directory))
        .count() as u64;

    Ok(MergeStats {
        file_count,
        dir_count,
        unique_blocks,
        conflicts,
    })
}

#[derive(Debug)]
pub struct MergeStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub unique_blocks: u64,
    pub conflicts: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{CreateOptions, create_archive};

    #[test]
    fn merge_disjoint_archives() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create two archives with different files
        let src_a = dir.path().join("a");
        std::fs::create_dir(&src_a).unwrap();
        std::fs::write(src_a.join("file_a.txt"), "content A").unwrap();

        let src_b = dir.path().join("b");
        std::fs::create_dir(&src_b).unwrap();
        std::fs::write(src_b.join("file_b.txt"), "content B").unwrap();

        let a_path = dir.path().join("a.tg");
        let b_path = dir.path().join("b.tg");
        create_archive(&a_path, &[src_a.as_path()], &CreateOptions::default()).unwrap();
        create_archive(&b_path, &[src_b.as_path()], &CreateOptions::default()).unwrap();

        let merged_path = dir.path().join("merged.tg");
        let _stats = merge_archives(&a_path, &b_path, &merged_path).unwrap();
        // Root directory entries may conflict, that's expected

        // Extract and verify both files present
        let dest = dir.path().join("extracted");
        crate::extract::extract_archive(&merged_path, &dest).unwrap();
        assert!(dest.join("file_a.txt").exists());
        assert!(dest.join("file_b.txt").exists());
    }

    #[test]
    fn merge_overlapping_archives_dedup() {
        let dir = tempfile::TempDir::new().unwrap();

        // Both archives contain the same large file
        let shared_data = "shared".repeat(5000);

        let src_a = dir.path().join("a");
        std::fs::create_dir(&src_a).unwrap();
        std::fs::write(src_a.join("shared.txt"), &shared_data).unwrap();
        std::fs::write(src_a.join("only_a.txt"), "only in A").unwrap();

        let src_b = dir.path().join("b");
        std::fs::create_dir(&src_b).unwrap();
        std::fs::write(src_b.join("shared.txt"), &shared_data).unwrap();
        std::fs::write(src_b.join("only_b.txt"), "only in B").unwrap();

        let a_path = dir.path().join("a.tg");
        let b_path = dir.path().join("b.tg");
        create_archive(&a_path, &[src_a.as_path()], &CreateOptions::default()).unwrap();
        create_archive(&b_path, &[src_b.as_path()], &CreateOptions::default()).unwrap();

        let merged_path = dir.path().join("merged.tg");
        let stats = merge_archives(&a_path, &b_path, &merged_path).unwrap();

        // Should have a conflict on shared.txt
        assert!(stats.conflicts >= 1);

        // Extract and verify
        let dest = dir.path().join("extracted");
        crate::extract::extract_archive(&merged_path, &dest).unwrap();
        assert!(dest.join("shared.txt").exists());
        assert!(dest.join("only_a.txt").exists());
        assert!(dest.join("only_b.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("shared.txt")).unwrap(),
            shared_data
        );
    }
}
