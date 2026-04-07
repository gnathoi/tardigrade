use crate::format::Hash;

/// Hash a block of data with BLAKE3
pub fn hash_block(data: &[u8]) -> Hash {
    blake3::hash(data).into()
}

/// Compute Merkle root over a set of block hashes + header bytes.
/// Binary tree: pairs of hashes are concatenated and hashed.
/// If odd number, last hash is promoted.
/// The archive header bytes are included as the first leaf to prevent
/// flag-flipping attacks.
pub fn merkle_root(header_bytes: &[u8], block_hashes: &[Hash], index_hash: &Hash) -> Hash {
    let mut leaves: Vec<Hash> = Vec::with_capacity(block_hashes.len() + 2);

    // First leaf: hash of archive header
    leaves.push(blake3::hash(header_bytes).into());

    // Block hashes as leaves
    leaves.extend_from_slice(block_hashes);

    // Last leaf: hash of the index
    leaves.push(*index_hash);

    if leaves.is_empty() {
        return [0u8; 32];
    }

    // Build tree bottom-up
    let mut current = leaves;
    while current.len() > 1 {
        let mut next = Vec::with_capacity((current.len() + 1) / 2);
        for pair in current.chunks(2) {
            if pair.len() == 2 {
                let mut hasher = blake3::Hasher::new();
                hasher.update(&pair[0]);
                hasher.update(&pair[1]);
                next.push(hasher.finalize().into());
            } else {
                // Odd one out, promote
                next.push(pair[0]);
            }
        }
        current = next;
    }

    current[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_block_deterministic() {
        let data = b"hello tardigrade";
        assert_eq!(hash_block(data), hash_block(data));
    }

    #[test]
    fn hash_block_different_data() {
        assert_ne!(hash_block(b"a"), hash_block(b"b"));
    }

    #[test]
    fn merkle_root_includes_header() {
        let header = b"TRDG\x01\x00\x00\x00";
        let hashes = vec![[1u8; 32], [2u8; 32]];
        let index_hash = [3u8; 32];

        let root1 = merkle_root(header, &hashes, &index_hash);
        let root2 = merkle_root(b"DIFFERENT", &hashes, &index_hash);

        // Different headers produce different roots
        assert_ne!(root1, root2);
    }

    #[test]
    fn merkle_root_single_block() {
        let header = b"HDR";
        let hashes = vec![[0xAA; 32]];
        let index_hash = [0xBB; 32];
        let root = merkle_root(header, &hashes, &index_hash);
        assert_ne!(root, [0u8; 32]);
    }
}
