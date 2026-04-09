use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::block;
use crate::compress;
use crate::encrypt::{self, KeyEncapsulation, SymmetricKey};
use crate::erasure;
use crate::error::{Error, Result};
use crate::format::*;
use crate::index::deserialize_index;
use crate::metadata::{restore_metadata, validate_extraction_path};

use std::path::PathBuf;
use std::sync::Arc;

/// Get a block from cache or read it. Returns Arc to avoid cloning.
/// If reading fails and the archive is erasure-coded, attempts ECC reconstruction.
fn get_block(
    reader: &mut (impl Read + Seek),
    cache: &mut HashMap<u64, Arc<Vec<u8>>>,
    offset: u64,
    key: Option<&SymmetricKey>,
    ecc_archive_path: Option<&Path>,
) -> Result<Arc<Vec<u8>>> {
    if let Some(cached) = cache.get(&offset) {
        return Ok(Arc::clone(cached));
    }
    let data = match block::read_block(reader, offset, key) {
        Ok((_, data)) => data,
        Err(Error::ChecksumMismatch { .. }) if ecc_archive_path.is_some() => {
            let (_, data) =
                reconstruct_block_via_ecc(reader, ecc_archive_path.unwrap(), offset, key)?;
            data
        }
        Err(e) => return Err(e),
    };
    let arc = Arc::new(data);
    cache.insert(offset, Arc::clone(&arc));
    Ok(arc)
}

/// Read the footer from the end of an archive file.
pub fn read_footer(reader: &mut (impl Read + Seek)) -> Result<Footer> {
    reader.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
    Footer::read_from(reader)
}

/// Read and deserialize the index from an archive.
pub fn read_index(reader: &mut (impl Read + Seek), footer: &Footer) -> Result<Vec<FileEntry>> {
    reader.seek(SeekFrom::Start(footer.index_offset))?;
    let mut index_data = vec![0u8; footer.index_length as usize];
    reader.read_exact(&mut index_data)?;

    // Try primary index
    match deserialize_index(&index_data, footer.index_length as usize * 10) {
        Ok(entries) => Ok(entries),
        Err(_) => {
            // Fall back to redundant index
            reader.seek(SeekFrom::Start(footer.redundant_index_offset))?;
            let mut redundant_data = vec![0u8; footer.index_length as usize];
            reader.read_exact(&mut redundant_data)?;
            deserialize_index(&redundant_data, footer.index_length as usize * 10)
        }
    }
}

/// Attempt to reconstruct a corrupted block using ECC parity data.
/// If the archive is encrypted, `key` must be provided to decrypt after reconstruction.
fn reconstruct_block_via_ecc(
    reader: &mut (impl Read + Seek),
    archive_path: &Path,
    offset: u64,
    key: Option<&SymmetricKey>,
) -> Result<(BlockHeader, Vec<u8>)> {
    let groups = crate::repair::scan_ecc_groups(archive_path)?;

    // Find the group containing this block
    let group = groups
        .iter()
        .find(|g| g.data_block_offsets.contains(&offset))
        .ok_or_else(|| Error::Ecc("corrupted block not in any ECC group".into()))?;

    let block_idx = group
        .data_block_offsets
        .iter()
        .position(|&o| o == offset)
        .unwrap();

    let level = erasure::EccLevel {
        data_shards: 10,
        parity_shards: group.parity_block_offsets.len(),
    };

    // Read all shards
    let mut shards: Vec<Option<Vec<u8>>> = Vec::new();

    for &off in &group.data_block_offsets {
        if off == offset {
            shards.push(None); // the corrupted one
        } else {
            reader.seek(SeekFrom::Start(off))?;
            let hdr = BlockHeader::read_from(reader)?;
            let mut raw = vec![0u8; hdr.compressed_size as usize];
            reader.read_exact(&mut raw)?;
            raw.resize(group.shard_size, 0);
            shards.push(Some(raw));
        }
    }

    // Pad to full data_shards
    while shards.len() < level.data_shards {
        shards.push(Some(vec![0u8; group.shard_size]));
    }

    // Read parity shards
    for &off in &group.parity_block_offsets {
        reader.seek(SeekFrom::Start(off))?;
        let hdr = BlockHeader::read_from(reader)?;
        let mut raw = vec![0u8; hdr.compressed_size as usize];
        reader.read_exact(&mut raw)?;
        raw.resize(group.shard_size, 0);
        shards.push(Some(raw));
    }

    erasure::reconstruct_shards(&mut shards, &level)?;

    // Get the original header for metadata
    reader.seek(SeekFrom::Start(offset))?;
    let header = BlockHeader::read_from(reader)?;
    let reconstructed = shards[block_idx].take().unwrap();
    let raw = &reconstructed[..header.compressed_size as usize];

    // Decrypt if encrypted, then decompress
    let compressed = if let Some(k) = key {
        encrypt::decrypt_block(raw, k, &header.hash)?
    } else {
        raw.to_vec()
    };
    let data = compress::decompress(&compressed, header.codec, header.uncompressed_size as usize)?;

    // Verify the reconstructed data
    let actual_hash: Hash = blake3::hash(&data).into();
    if actual_hash != header.hash {
        return Err(Error::Ecc("ECC reconstruction produced wrong hash".into()));
    }

    Ok((header, data))
}

/// Read a single file from the archive and return its contents.
pub fn cat_file(
    archive_path: &Path,
    file_path: &str,
    passphrase: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    let header = ArchiveHeader::read_from(&mut reader)?;

    let key: Option<encrypt::SymmetricKey> = if header.is_encrypted() {
        let pass = passphrase.ok_or(Error::EncryptedArchive)?;
        let encap = KeyEncapsulation::read_from(&mut reader)?;
        Some(encap.unwrap_with_passphrase(pass)?)
    } else {
        None
    };

    let footer = read_footer(&mut reader)?;
    let entries = read_index(&mut reader, &footer)?;

    // Find the entry matching the requested path.
    // Normalize separators: archives may store paths with \ on Windows.
    let normalized = file_path
        .trim_start_matches('/')
        .trim_start_matches('\\')
        .replace('\\', "/");
    let entry = entries
        .iter()
        .find(|e| {
            let p = e.path_display().replace('\\', "/");
            let p = p.trim_start_matches('/');
            p == normalized
        })
        .ok_or_else(|| Error::InvalidArchive(format!("file not found in archive: {file_path}")))?;

    if !matches!(entry.file_type, FileType::File) {
        return Err(Error::InvalidArchive(format!(
            "{file_path} is not a regular file"
        )));
    }

    let ecc_path: Option<PathBuf> = if header.is_erasure_coded() {
        Some(archive_path.to_path_buf())
    } else {
        None
    };
    let ecc_ref = ecc_path.as_deref();

    let mut block_cache: HashMap<u64, Arc<Vec<u8>>> = HashMap::new();

    if entry.block_refs.len() == 1 {
        let bref = &entry.block_refs[0];
        let block_data = get_block(
            &mut reader,
            &mut block_cache,
            bref.offset,
            key.as_ref(),
            ecc_ref,
        )?;
        Ok(block_data
            [bref.slice_start as usize..bref.slice_start as usize + bref.slice_len as usize]
            .to_vec())
    } else {
        let mut file_data = Vec::with_capacity(entry.size as usize);
        for bref in &entry.block_refs {
            let block_data = get_block(
                &mut reader,
                &mut block_cache,
                bref.offset,
                key.as_ref(),
                ecc_ref,
            )?;
            file_data.extend_from_slice(
                &block_data[bref.slice_start as usize
                    ..bref.slice_start as usize + bref.slice_len as usize],
            );
        }
        Ok(file_data)
    }
}

/// Extract an archive to a destination directory.
pub fn extract_archive(archive_path: &Path, dest: &Path) -> Result<ExtractStats> {
    extract_archive_inner(archive_path, dest, None)
}

/// Extract an encrypted archive with a passphrase.
pub fn extract_archive_encrypted(
    archive_path: &Path,
    dest: &Path,
    passphrase: &[u8],
) -> Result<ExtractStats> {
    extract_archive_inner(archive_path, dest, Some(passphrase))
}

fn extract_archive_inner(
    archive_path: &Path,
    dest: &Path,
    passphrase: Option<&[u8]>,
) -> Result<ExtractStats> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    // Read and validate header
    let header = ArchiveHeader::read_from(&mut reader)?;

    // Handle encryption
    let key: Option<SymmetricKey> = if header.is_encrypted() {
        let pass = passphrase.ok_or(Error::EncryptedArchive)?;
        let encap = KeyEncapsulation::read_from(&mut reader)?;
        Some(encap.unwrap_with_passphrase(pass)?)
    } else {
        None
    };

    // Read footer and index
    let footer = read_footer(&mut reader)?;
    let entries = read_index(&mut reader, &footer)?;

    // Create destination
    fs::create_dir_all(dest).map_err(|e| Error::io_path(dest, e))?;

    let mut stats = ExtractStats {
        file_count: 0,
        dir_count: 0,
        total_size: 0,
    };

    // Cache for decompressed blocks — uses Arc to avoid cloning large buffers
    let mut block_cache: HashMap<u64, std::sync::Arc<Vec<u8>>> = HashMap::new();

    // ECC recovery path (only for erasure-coded, non-encrypted archives)
    let ecc_path: Option<PathBuf> = if header.is_erasure_coded() {
        Some(archive_path.to_path_buf())
    } else {
        None
    };
    let ecc_ref = ecc_path.as_deref();

    // First pass: create all directories
    for entry in &entries {
        if entry.file_type == FileType::Directory {
            let target = validate_extraction_path(&entry.path, dest)?;
            fs::create_dir_all(&target).map_err(|e| Error::io_path(&target, e))?;
            stats.dir_count += 1;
        }
    }

    // Second pass: extract files
    for entry in &entries {
        match &entry.file_type {
            FileType::File => {
                let target = validate_extraction_path(&entry.path, dest)?;

                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| Error::io_path(parent, e))?;
                }

                // Fast path: single-block file (very common for source code)
                if entry.block_refs.len() == 1 {
                    let block_ref = &entry.block_refs[0];
                    let block_data = get_block(
                        &mut reader,
                        &mut block_cache,
                        block_ref.offset,
                        key.as_ref(),
                        ecc_ref,
                    )?;

                    let start = block_ref.slice_start as usize;
                    let end = start + block_ref.slice_len as usize;
                    if end > block_data.len() {
                        return Err(Error::InvalidArchive(format!(
                            "block ref out of bounds: {}..{} > {}",
                            start,
                            end,
                            block_data.len()
                        )));
                    }

                    fs::write(&target, &block_data[start..end])
                        .map_err(|e| Error::io_path(&target, e))?;
                } else {
                    // Multi-block file: reassemble
                    let mut file_data = Vec::with_capacity(entry.size as usize);
                    for block_ref in &entry.block_refs {
                        let block_data = get_block(
                            &mut reader,
                            &mut block_cache,
                            block_ref.offset,
                            key.as_ref(),
                            ecc_ref,
                        )?;

                        let start = block_ref.slice_start as usize;
                        let end = start + block_ref.slice_len as usize;
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
                }
                restore_metadata(&target, entry)?;

                stats.file_count += 1;
                stats.total_size += entry.size;
            }
            FileType::Symlink(target_bytes) => {
                let target = validate_extraction_path(&entry.path, dest)?;
                let link_target = String::from_utf8_lossy(target_bytes);

                // Validate symlink target doesn't escape destination
                if link_target.contains("..") {
                    let resolved = dest.join(link_target.as_ref());
                    if let Ok(canonical_dest) = dest.canonicalize()
                        && let Ok(canonical_target) = resolved.canonicalize()
                        && !canonical_target.starts_with(&canonical_dest)
                    {
                        return Err(Error::SymlinkEscape {
                            path: target.clone(),
                            target: resolved,
                        });
                    }
                }

                #[cfg(unix)]
                {
                    if target.exists() || target.symlink_metadata().is_ok() {
                        fs::remove_file(&target).ok();
                    }
                    std::os::unix::fs::symlink(link_target.as_ref(), &target)
                        .map_err(|e| Error::io_path(&target, e))?;
                }

                stats.file_count += 1;
            }
            FileType::Directory => {} // Already handled
            FileType::Hardlink(target_bytes) => {
                let target = validate_extraction_path(&entry.path, dest)?;
                let link_target_str = String::from_utf8_lossy(target_bytes);
                let link_target = dest.join(link_target_str.as_ref());

                if link_target.exists() {
                    fs::hard_link(&link_target, &target).map_err(|e| Error::io_path(&target, e))?;
                }

                stats.file_count += 1;
            }
        }
    }

    // Third pass: restore directory metadata (after all files are written)
    for entry in &entries {
        if entry.file_type == FileType::Directory {
            let target = validate_extraction_path(&entry.path, dest)?;
            if target.exists() {
                restore_metadata(&target, entry).ok(); // best effort for dirs
            }
        }
    }

    Ok(stats)
}

/// List entries in an archive (just paths and metadata, no extraction).
pub fn list_archive(archive_path: &Path) -> Result<Vec<FileEntry>> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    let _header = ArchiveHeader::read_from(&mut reader)?;
    let footer = read_footer(&mut reader)?;
    read_index(&mut reader, &footer)
}

#[derive(Debug)]
pub struct ExtractStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub total_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{CreateOptions, create_archive};
    use tempfile::TempDir;

    #[test]
    fn create_extract_round_trip() {
        let src = TempDir::new().unwrap();
        fs::write(src.path().join("hello.txt"), "Hello, tardigrade!").unwrap();
        fs::create_dir(src.path().join("subdir")).unwrap();
        fs::write(src.path().join("subdir/nested.txt"), "Nested content.").unwrap();

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("test.tg");

        create_archive(&archive_path, &[src.path()], &CreateOptions::default()).unwrap();

        let dest = TempDir::new().unwrap();
        let stats = extract_archive(&archive_path, dest.path()).unwrap();

        assert_eq!(stats.file_count, 2);

        // Verify file contents
        let hello = fs::read_to_string(dest.path().join("hello.txt")).unwrap();
        assert_eq!(hello, "Hello, tardigrade!");

        let nested = fs::read_to_string(dest.path().join("subdir/nested.txt")).unwrap();
        assert_eq!(nested, "Nested content.");
    }

    #[test]
    fn create_extract_dedup_round_trip() {
        let src = TempDir::new().unwrap();
        let data = "deduplicated content".repeat(5000);
        fs::write(src.path().join("a.txt"), &data).unwrap();
        fs::write(src.path().join("b.txt"), &data).unwrap();

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("dedup.tg");

        let create_stats =
            create_archive(&archive_path, &[src.path()], &CreateOptions::default()).unwrap();

        assert!(create_stats.dedup_savings > 0);

        let dest = TempDir::new().unwrap();
        extract_archive(&archive_path, dest.path()).unwrap();

        let a = fs::read_to_string(dest.path().join("a.txt")).unwrap();
        let b = fs::read_to_string(dest.path().join("b.txt")).unwrap();
        assert_eq!(a, data);
        assert_eq!(b, data);
    }

    #[test]
    fn list_archive_returns_entries() {
        let src = TempDir::new().unwrap();
        fs::write(src.path().join("file.txt"), "content").unwrap();

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("list.tg");

        create_archive(&archive_path, &[src.path()], &CreateOptions::default()).unwrap();

        let entries = list_archive(&archive_path).unwrap();
        assert!(!entries.is_empty());
        let paths: Vec<String> = entries.iter().map(|e| e.path_display()).collect();
        assert!(paths.iter().any(|p| p.contains("file.txt")));
    }

    #[test]
    fn encrypted_round_trip() {
        let src = TempDir::new().unwrap();
        let data = "secret data that must survive encryption round-trip".repeat(100);
        fs::write(src.path().join("secret.txt"), &data).unwrap();

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("encrypted.tg");

        let opts = CreateOptions {
            passphrase: Some(b"test-passphrase-123".to_vec()),
            ..CreateOptions::default()
        };
        create_archive(&archive_path, &[src.path()], &opts).unwrap();

        // Verify it's actually encrypted (flag set)
        let file = std::fs::File::open(&archive_path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let header = crate::format::ArchiveHeader::read_from(&mut reader).unwrap();
        assert!(header.is_encrypted());

        // Extract with correct passphrase
        let dest = TempDir::new().unwrap();
        extract_archive_encrypted(&archive_path, dest.path(), b"test-passphrase-123").unwrap();
        let content = fs::read_to_string(dest.path().join("secret.txt")).unwrap();
        assert_eq!(content, data);
    }

    #[test]
    fn encrypted_with_ecc() {
        let src = TempDir::new().unwrap();
        let data = "encrypted + ECC data".repeat(500);
        fs::write(src.path().join("file.txt"), &data).unwrap();

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("enc_ecc.tg");

        let opts = CreateOptions {
            passphrase: Some(b"test-pass".to_vec()),
            ecc_level: Some(crate::erasure::EccLevel::LOW),
            ..CreateOptions::default()
        };
        create_archive(&archive_path, &[src.path()], &opts).unwrap();

        // Verify both flags are set
        let file = std::fs::File::open(&archive_path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let header = crate::format::ArchiveHeader::read_from(&mut reader).unwrap();
        assert!(header.is_encrypted(), "should be encrypted");
        assert!(header.is_erasure_coded(), "should have ECC");

        // Extract with correct passphrase
        let dest = TempDir::new().unwrap();
        extract_archive_encrypted(&archive_path, dest.path(), b"test-pass").unwrap();
        let content = fs::read_to_string(dest.path().join("file.txt")).unwrap();
        assert_eq!(content, data);
    }

    #[test]
    fn encrypted_wrong_passphrase_fails() {
        let src = TempDir::new().unwrap();
        fs::write(src.path().join("data.txt"), "content").unwrap();

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("enc.tg");

        let opts = CreateOptions {
            passphrase: Some(b"correct".to_vec()),
            ..CreateOptions::default()
        };
        create_archive(&archive_path, &[src.path()], &opts).unwrap();

        let dest = TempDir::new().unwrap();
        assert!(extract_archive_encrypted(&archive_path, dest.path(), b"wrong").is_err());
    }

    #[test]
    fn encrypted_without_passphrase_fails() {
        let src = TempDir::new().unwrap();
        fs::write(src.path().join("data.txt"), "content").unwrap();

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("enc2.tg");

        let opts = CreateOptions {
            passphrase: Some(b"pass".to_vec()),
            ..CreateOptions::default()
        };
        create_archive(&archive_path, &[src.path()], &opts).unwrap();

        let dest = TempDir::new().unwrap();
        assert!(extract_archive(&archive_path, dest.path()).is_err());
    }

    #[test]
    fn empty_directory_archive() {
        let src = TempDir::new().unwrap();
        // Just the empty temp dir itself

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("empty.tg");

        let stats =
            create_archive(&archive_path, &[src.path()], &CreateOptions::default()).unwrap();

        assert_eq!(stats.file_count, 0);
        assert_eq!(stats.dir_count, 1);

        let dest = TempDir::new().unwrap();
        extract_archive(&archive_path, dest.path()).unwrap();
    }
}
