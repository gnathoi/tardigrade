use crate::compress;
use crate::error::{Error, Result};
use crate::format::{CODEC_ZSTD, FileEntry, Hash};
use crate::hash::hash_block;

/// Serialize the index (Vec<FileEntry>) to compressed msgpack bytes.
/// Returns (compressed_bytes, hash_of_uncompressed_index).
pub fn serialize_index(entries: &[FileEntry]) -> Result<(Vec<u8>, Hash)> {
    let raw = rmp_serde::to_vec(entries)
        .map_err(|e| Error::IndexDeserialize(format!("serialize: {e}")))?;

    let index_hash = hash_block(&raw);

    let compressed = compress::compress(&raw, CODEC_ZSTD, 3)?;

    Ok((compressed, index_hash))
}

/// Deserialize the index from compressed msgpack bytes.
/// Returns the file entries and verifies the hash if provided.
pub fn deserialize_index(compressed: &[u8], expected_size_hint: usize) -> Result<Vec<FileEntry>> {
    let raw = compress::decompress(compressed, CODEC_ZSTD, expected_size_hint)?;

    let entries: Vec<FileEntry> = rmp_serde::from_slice(&raw)
        .map_err(|e| Error::IndexDeserialize(format!("deserialize: {e}")))?;

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{BlockRef, FileType};
    use std::collections::HashMap;

    fn sample_entries() -> Vec<FileEntry> {
        vec![
            FileEntry {
                path: b"hello.txt".to_vec(),
                file_type: FileType::File,
                mode: 0o644,
                uid: 1000,
                gid: 1000,
                mtime_ns: 1_700_000_000_000_000_000,
                size: 42,
                block_refs: vec![BlockRef {
                    hash: [0xAA; 32],
                    offset: 16,
                    slice_start: 0,
                    slice_len: 42,
                    flags: 0,
                    reserved: [0; 3],
                }],
                xattrs: HashMap::new(),
                snapshot_id: None,
            },
            FileEntry {
                path: b"subdir".to_vec(),
                file_type: FileType::Directory,
                mode: 0o755,
                uid: 1000,
                gid: 1000,
                mtime_ns: 1_700_000_000_000_000_000,
                size: 0,
                block_refs: vec![],
                xattrs: HashMap::new(),
                snapshot_id: None,
            },
        ]
    }

    #[test]
    fn index_round_trip() {
        let entries = sample_entries();
        let (compressed, _hash) = serialize_index(&entries).unwrap();

        // Use a generous hint
        let decoded = deserialize_index(&compressed, 4096).unwrap();
        assert_eq!(decoded.len(), entries.len());
        assert_eq!(decoded[0].path, b"hello.txt");
        assert_eq!(decoded[1].file_type, FileType::Directory);
    }

    #[test]
    fn index_hash_is_deterministic() {
        let entries = sample_entries();
        let (_, hash1) = serialize_index(&entries).unwrap();
        let (_, hash2) = serialize_index(&entries).unwrap();
        assert_eq!(hash1, hash2);
    }
}
