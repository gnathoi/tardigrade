/// Diff between temporal generations in an append-only archive.
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{Error, Result};
use crate::format::{FileEntry, FileType, Hash};
use crate::index::deserialize_index;
use crate::temporal::list_snapshots;

/// A single file change between two generations.
#[derive(Debug)]
pub enum DiffEntry {
    Added(FileEntry),
    Removed(FileEntry),
    Modified { old: FileEntry, new: FileEntry },
}

/// Result of diffing two generations.
#[derive(Debug)]
pub struct DiffResult {
    pub entries: Vec<DiffEntry>,
    pub unchanged_count: usize,
}

/// Compute the content fingerprint of a file entry by concatenating its block hashes.
fn content_fingerprint(entry: &FileEntry) -> Vec<Hash> {
    entry.block_refs.iter().map(|br| br.hash).collect()
}

/// Read the file index for a specific generation.
fn read_generation_index(archive_path: &Path, generation: u64) -> Result<Vec<FileEntry>> {
    let snapshots = list_snapshots(archive_path)?;

    let snapshot = snapshots
        .iter()
        .find(|s| s.generation == generation)
        .ok_or_else(|| {
            Error::InvalidArchive(format!(
                "generation {} not found (archive has {} generations: 0..{})",
                generation,
                snapshots.len(),
                snapshots.len().saturating_sub(1)
            ))
        })?;

    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    reader.seek(SeekFrom::Start(snapshot.footer.index_offset))?;
    let mut index_data = vec![0u8; snapshot.footer.index_length as usize];
    reader.read_exact(&mut index_data)?;

    deserialize_index(&index_data, snapshot.footer.index_length as usize * 10)
}

/// Diff two generations of a temporal archive.
pub fn diff_generations(archive_path: &Path, from_gen: u64, to_gen: u64) -> Result<DiffResult> {
    let entries_a = read_generation_index(archive_path, from_gen)?;
    let entries_b = read_generation_index(archive_path, to_gen)?;

    // Build maps keyed by path
    let map_a: HashMap<&[u8], &FileEntry> = entries_a
        .iter()
        .filter(|e| matches!(e.file_type, FileType::File))
        .map(|e| (e.path.as_slice(), e))
        .collect();

    let map_b: HashMap<&[u8], &FileEntry> = entries_b
        .iter()
        .filter(|e| matches!(e.file_type, FileType::File))
        .map(|e| (e.path.as_slice(), e))
        .collect();

    let mut diff_entries = Vec::new();
    let mut unchanged_count = 0;

    // Check entries in B against A
    for (path, entry_b) in &map_b {
        match map_a.get(path) {
            None => {
                // Added in B
                diff_entries.push(DiffEntry::Added((*entry_b).clone()));
            }
            Some(entry_a) => {
                // Exists in both — check if content changed
                let fp_a = content_fingerprint(entry_a);
                let fp_b = content_fingerprint(entry_b);
                if fp_a != fp_b || entry_a.size != entry_b.size {
                    diff_entries.push(DiffEntry::Modified {
                        old: (*entry_a).clone(),
                        new: (*entry_b).clone(),
                    });
                } else {
                    unchanged_count += 1;
                }
            }
        }
    }

    // Check for removals (in A but not in B)
    for (path, entry_a) in &map_a {
        if !map_b.contains_key(path) {
            diff_entries.push(DiffEntry::Removed((*entry_a).clone()));
        }
    }

    // Sort: removed first, then modified, then added. Within each group, sort by path.
    diff_entries.sort_by(|a, b| {
        let order = |e: &DiffEntry| match e {
            DiffEntry::Removed(_) => 0,
            DiffEntry::Modified { .. } => 1,
            DiffEntry::Added(_) => 2,
        };
        let path = |e: &DiffEntry| match e {
            DiffEntry::Added(f) => f.path.clone(),
            DiffEntry::Removed(f) => f.path.clone(),
            DiffEntry::Modified { new, .. } => new.path.clone(),
        };
        order(a).cmp(&order(b)).then_with(|| path(a).cmp(&path(b)))
    });

    Ok(DiffResult {
        entries: diff_entries,
        unchanged_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{CreateOptions, create_archive};
    use crate::temporal::append_archive;

    fn setup_temporal_archive(dir: &tempfile::TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let archive_path = dir.path().join("test.tg");
        (src, archive_path)
    }

    #[test]
    fn test_diff_added_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let (src, archive) = setup_temporal_archive(&dir);

        std::fs::write(src.join("base.txt"), "base content").unwrap();
        create_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        std::fs::write(src.join("new.txt"), "new content").unwrap();
        append_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        let result = diff_generations(&archive, 0, 1).unwrap();
        let added: Vec<_> = result
            .entries
            .iter()
            .filter(|e| matches!(e, DiffEntry::Added(_)))
            .collect();
        assert_eq!(added.len(), 1);
        if let DiffEntry::Added(f) = &added[0] {
            assert!(f.path_display().contains("new.txt"));
        }
    }

    #[test]
    fn test_diff_removed_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let (src, archive) = setup_temporal_archive(&dir);

        std::fs::write(src.join("keep.txt"), "keep").unwrap();
        std::fs::write(src.join("remove.txt"), "remove me").unwrap();
        create_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        std::fs::remove_file(src.join("remove.txt")).unwrap();
        append_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        let result = diff_generations(&archive, 0, 1).unwrap();
        let removed: Vec<_> = result
            .entries
            .iter()
            .filter(|e| matches!(e, DiffEntry::Removed(_)))
            .collect();
        assert_eq!(removed.len(), 1);
        if let DiffEntry::Removed(f) = &removed[0] {
            assert!(f.path_display().contains("remove.txt"));
        }
    }

    #[test]
    fn test_diff_modified_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let (src, archive) = setup_temporal_archive(&dir);

        std::fs::write(src.join("file.txt"), "version 1").unwrap();
        create_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        std::fs::write(src.join("file.txt"), "version 2 with more content").unwrap();
        append_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        let result = diff_generations(&archive, 0, 1).unwrap();
        let modified: Vec<_> = result
            .entries
            .iter()
            .filter(|e| matches!(e, DiffEntry::Modified { .. }))
            .collect();
        assert_eq!(modified.len(), 1);
    }

    #[test]
    fn test_diff_no_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let (src, archive) = setup_temporal_archive(&dir);

        std::fs::write(src.join("stable.txt"), "unchanged").unwrap();
        create_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        // Append same content
        append_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        let result = diff_generations(&archive, 0, 1).unwrap();
        assert!(result.entries.is_empty());
        assert!(result.unchanged_count > 0);
    }

    #[test]
    fn test_diff_invalid_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        let (src, archive) = setup_temporal_archive(&dir);

        std::fs::write(src.join("file.txt"), "content").unwrap();
        create_archive(&archive, &[src.as_path()], &CreateOptions::default()).unwrap();

        let err = diff_generations(&archive, 0, 5).unwrap_err();
        assert!(err.to_string().contains("generation 5 not found"));
    }
}
