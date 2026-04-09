// Shared block-level utilities used by archive, temporal, incremental, and extract.
//
// Deduplicates read_block, get_block (non-ECC), process_file_data, and WalkBuilder
// configuration that were previously copy-pasted across modules.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use ignore::WalkBuilder;

use crate::chunk::chunk_data;
use crate::compress;
use crate::encrypt::{self, SymmetricKey};
use crate::error::{Error, Result};
use crate::format::*;
use crate::hash::hash_block;

pub const MIN_CHUNK_SIZE: usize = 64 * 1024;
pub const MIN_COMPRESS_SIZE: usize = 64;

pub struct CompressedChunk {
    pub hash: Hash,
    pub uncompressed_size: u32,
    pub compressed_data: Vec<u8>,
    pub codec: u8,
}

/// Read and decompress a single block from the archive.
/// If `key` is Some, decrypts before decompressing. Verifies BLAKE3 hash.
pub fn read_block(
    reader: &mut (impl Read + Seek),
    offset: u64,
    key: Option<&SymmetricKey>,
) -> Result<(BlockHeader, Vec<u8>)> {
    reader.seek(SeekFrom::Start(offset))?;
    let header = BlockHeader::read_from(reader)?;

    let mut raw = vec![0u8; header.compressed_size as usize];
    reader.read_exact(&mut raw)?;

    let compressed = if let Some(k) = key {
        encrypt::decrypt_block(&raw, k, &header.hash)?
    } else {
        raw
    };

    let data = compress::decompress(&compressed, header.codec, header.uncompressed_size as usize)?;

    let actual_hash: Hash = blake3::hash(&data).into();
    if actual_hash != header.hash {
        return Err(Error::ChecksumMismatch {
            offset,
            expected: hex::encode(header.hash),
            actual: hex::encode(actual_hash),
        });
    }

    Ok((header, data))
}

/// Get a block from cache or read it (no encryption, no ECC).
/// Used by temporal and incremental paths.
pub fn get_block(
    reader: &mut (impl Read + Seek),
    cache: &mut HashMap<u64, Arc<Vec<u8>>>,
    offset: u64,
) -> Result<Arc<Vec<u8>>> {
    if let Some(cached) = cache.get(&offset) {
        return Ok(Arc::clone(cached));
    }
    let (_, data) = read_block(reader, offset, None)?;
    let arc = Arc::new(data);
    cache.insert(offset, Arc::clone(&arc));
    Ok(arc)
}

/// Chunk and compress a file's data. Optimized for common cases:
/// - Empty files: no chunks
/// - Small files (< 64KB): single block, skip FastCDC gear hash
/// - Tiny blocks (< 64B): store uncompressed (zstd overhead > savings)
/// - Large files: FastCDC content-defined chunking
pub fn process_file_data(data: &[u8], codec: u8, level: i32) -> Result<Vec<CompressedChunk>> {
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

/// Configure a WalkBuilder with standard tardigrade settings.
/// Caller decides whether to call .build() or .build_parallel().
pub fn configure_walker(source: &Path, respect_gitignore: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(source);
    builder
        .git_ignore(respect_gitignore)
        .git_global(respect_gitignore)
        .git_exclude(respect_gitignore)
        .hidden(false)
        .filter_entry(|e| !(e.file_type().is_some_and(|ft| ft.is_dir()) && e.file_name() == ".git"))
        .follow_links(false);
    builder
}
