use crate::format::Hash;
use crate::hash::hash_block;

/// A chunk of data with its content hash
pub struct Chunk {
    pub hash: Hash,
    pub data: Vec<u8>,
}

/// Minimum chunk size for FastCDC
const MIN_CHUNK: u32 = 64 * 1024; // 64 KB
/// Target (average) chunk size
const AVG_CHUNK: u32 = 256 * 1024; // 256 KB
/// Maximum chunk size
const MAX_CHUNK: u32 = 1024 * 1024; // 1 MB

/// Split file data into content-defined chunks using FastCDC.
/// Returns chunks with their BLAKE3 hashes.
pub fn chunk_data(data: &[u8]) -> Vec<Chunk> {
    if data.is_empty() {
        return vec![];
    }

    let chunker = fastcdc::v2020::FastCDC::new(data, MIN_CHUNK, AVG_CHUNK, MAX_CHUNK);
    chunker
        .map(|chunk| {
            let slice = &data[chunk.offset..chunk.offset + chunk.length];
            let hash = hash_block(slice);
            Chunk {
                hash,
                data: slice.to_vec(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_data_no_chunks() {
        assert!(chunk_data(b"").is_empty());
    }

    #[test]
    fn small_data_single_chunk() {
        let data = vec![0u8; 1000];
        let chunks = chunk_data(&data);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data.len(), 1000);
    }

    #[test]
    fn large_data_multiple_chunks() {
        // 2MB of data should produce multiple chunks
        let data = vec![0x42u8; 2 * 1024 * 1024];
        let chunks = chunk_data(&data);
        assert!(chunks.len() > 1);

        // Reassemble and verify
        let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.data.iter().copied()).collect();
        assert_eq!(reassembled, data);
    }

    #[test]
    fn chunks_have_valid_hashes() {
        let data = vec![0xAB; 500_000];
        let chunks = chunk_data(&data);
        for chunk in &chunks {
            assert_eq!(chunk.hash, hash_block(&chunk.data));
        }
    }

    #[test]
    fn identical_data_same_chunks() {
        let data = vec![0x55; 300_000];
        let chunks1 = chunk_data(&data);
        let chunks2 = chunk_data(&data);
        assert_eq!(chunks1.len(), chunks2.len());
        for (a, b) in chunks1.iter().zip(chunks2.iter()) {
            assert_eq!(a.hash, b.hash);
        }
    }
}
