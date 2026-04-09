use std::collections::HashMap;

use crate::format::Hash;

/// Maximum number of blocks tracked before the store stops accepting new entries.
/// At 32 bytes per hash + 8 bytes per offset + HashMap overhead (~80 bytes per entry),
/// 2M entries uses roughly 160-200 MB. This prevents OOM on memory-constrained systems
/// while covering archives up to ~500 GB at 256 KB average block size.
const DEFAULT_MAX_ENTRIES: usize = 2_000_000;

/// Content-addressed dedup store.
/// Maps block hash → offset in the archive file.
/// Stops tracking new blocks after `max_entries` to bound memory usage.
/// Blocks beyond the cap are written normally but miss dedup opportunities.
pub struct DedupStore {
    map: HashMap<Hash, u64>,
    max_entries: usize,
    overflow_count: u64,
}

impl DedupStore {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            max_entries: DEFAULT_MAX_ENTRIES,
            overflow_count: 0,
        }
    }

    /// Create a store with a custom capacity limit.
    #[allow(dead_code)]
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            map: HashMap::with_capacity(max_entries.min(1024)),
            max_entries,
            overflow_count: 0,
        }
    }

    /// Try to insert a block. Returns:
    /// - `None` if the block is new (inserted with given offset, or store is full)
    /// - `Some(existing_offset)` if the block was already stored
    pub fn insert(&mut self, hash: Hash, offset: u64) -> Option<u64> {
        if let Some(&existing) = self.map.get(&hash) {
            Some(existing)
        } else {
            if self.map.len() < self.max_entries {
                self.map.insert(hash, offset);
            } else {
                self.overflow_count += 1;
            }
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

    /// Number of blocks that couldn't be tracked due to the memory cap.
    /// These blocks were written but may have missed dedup opportunities.
    pub fn overflow_count(&self) -> u64 {
        self.overflow_count
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

    #[test]
    fn overflow_stops_tracking() {
        let mut store = DedupStore::with_capacity(2);
        store.insert([1u8; 32], 100);
        store.insert([2u8; 32], 200);
        // Store is now full
        store.insert([3u8; 32], 300);
        assert_eq!(store.len(), 2);
        assert_eq!(store.overflow_count(), 1);
        // Existing entries still dedup
        assert_eq!(store.insert([1u8; 32], 400), Some(100));
        // New entry still not tracked
        assert!(store.get(&[3u8; 32]).is_none());
    }

    #[test]
    fn overflow_count_accumulates() {
        let mut store = DedupStore::with_capacity(1);
        store.insert([1u8; 32], 100);
        store.insert([2u8; 32], 200);
        store.insert([3u8; 32], 300);
        assert_eq!(store.overflow_count(), 2);
        assert_eq!(store.len(), 1);
    }
}
