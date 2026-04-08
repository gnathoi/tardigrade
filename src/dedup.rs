use std::collections::HashMap;

use crate::format::Hash;

/// Content-addressed dedup store.
/// Maps block hash → offset in the archive file.
/// Uses HashMap (single-threaded Phase 1). Will add spill-to-disk for >1M blocks later.
pub struct DedupStore {
    map: HashMap<Hash, u64>,
}

impl DedupStore {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Try to insert a block. Returns:
    /// - `None` if the block is new (inserted with given offset)
    /// - `Some(existing_offset)` if the block was already stored
    pub fn insert(&mut self, hash: Hash, offset: u64) -> Option<u64> {
        if let Some(&existing) = self.map.get(&hash) {
            Some(existing)
        } else {
            self.map.insert(hash, offset);
            None
        }
    }

    /// Look up a block by hash
    pub fn get(&self, hash: &Hash) -> Option<u64> {
        self.map.get(hash).copied()
    }

    /// Number of unique blocks stored
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_block_returns_none() {
        let mut store = DedupStore::new();
        assert!(store.insert([1u8; 32], 0).is_none());
    }

    #[test]
    fn duplicate_block_returns_existing_offset() {
        let mut store = DedupStore::new();
        let hash = [2u8; 32];
        store.insert(hash, 100);
        assert_eq!(store.insert(hash, 200), Some(100));
    }

    #[test]
    fn different_hashes_are_independent() {
        let mut store = DedupStore::new();
        store.insert([1u8; 32], 100);
        store.insert([2u8; 32], 200);
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(&[1u8; 32]), Some(100));
        assert_eq!(store.get(&[2u8; 32]), Some(200));
    }
}
