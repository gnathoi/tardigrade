// Reed-Solomon erasure coding support.
//
// ECC levels:
// - Low:    RS(10,2) — recovers up to 2 lost shards per group (~20% overhead)
// - Medium: RS(10,4) — recovers up to 4 lost shards per group (~40% overhead)
// - High:   RS(10,6) — recovers up to 6 lost shards per group (~60% overhead)
//
// Groups of data blocks are processed together. After each group, parity
// shards are written as additional blocks with BLOCK_FLAG_ECC set.

use reed_solomon_erasure::galois_8::ReedSolomon;

use crate::error::{Error, Result};

/// ECC level configuration
#[derive(Debug, Clone, Copy)]
pub struct EccLevel {
    pub data_shards: usize,
    pub parity_shards: usize,
}

impl EccLevel {
    pub const LOW: Self = Self {
        data_shards: 10,
        parity_shards: 2,
    };
    pub const MEDIUM: Self = Self {
        data_shards: 10,
        parity_shards: 4,
    };
    pub const HIGH: Self = Self {
        data_shards: 10,
        parity_shards: 6,
    };

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(Self::LOW),
            "medium" | "med" => Some(Self::MEDIUM),
            "high" => Some(Self::HIGH),
            _ => None,
        }
    }

    /// Returns true if the string explicitly disables ECC.
    pub fn is_none(s: &str) -> bool {
        matches!(s.to_lowercase().as_str(), "none" | "off" | "false")
    }

    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }

    pub fn overhead_percent(&self) -> f64 {
        (self.parity_shards as f64 / self.data_shards as f64) * 100.0
    }

    pub fn name(&self) -> &'static str {
        match self.parity_shards {
            2 => "low",
            4 => "medium",
            6 => "high",
            _ => "custom",
        }
    }
}

/// A group of data shards ready for ECC encoding.
#[derive(Debug)]
pub struct EccGroup {
    /// The compressed block data for each shard in the group
    pub data_shards: Vec<Vec<u8>>,
    /// Maximum shard size (all shards are padded to this)
    pub shard_size: usize,
}

impl EccGroup {
    pub fn new() -> Self {
        Self {
            data_shards: Vec::new(),
            shard_size: 0,
        }
    }

    pub fn add_shard(&mut self, data: Vec<u8>) {
        if data.len() > self.shard_size {
            self.shard_size = data.len();
        }
        self.data_shards.push(data);
    }

    pub fn len(&self) -> usize {
        self.data_shards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data_shards.is_empty()
    }
}

/// Encode parity shards for a group of data blocks.
/// Returns the parity shard data (each exactly `shard_size` bytes).
pub fn encode_parity(group: &EccGroup, level: &EccLevel) -> Result<Vec<Vec<u8>>> {
    if group.is_empty() {
        return Ok(vec![]);
    }

    let shard_size = group.shard_size;

    if shard_size == 0 {
        return Ok(vec![]);
    }

    // Pad all data shards to the same size
    let mut shards: Vec<Vec<u8>> = group
        .data_shards
        .iter()
        .map(|d| {
            let mut padded = d.clone();
            padded.resize(shard_size, 0);
            padded
        })
        .collect();

    // If we have fewer data shards than the configured amount,
    // pad with empty shards
    while shards.len() < level.data_shards {
        shards.push(vec![0u8; shard_size]);
    }

    // Add empty parity shards
    for _ in 0..level.parity_shards {
        shards.push(vec![0u8; shard_size]);
    }

    let rs = ReedSolomon::new(level.data_shards, level.parity_shards)
        .map_err(|e| Error::Ecc(format!("failed to create RS encoder: {e}")))?;

    rs.encode(&mut shards)
        .map_err(|e| Error::Ecc(format!("RS encode failed: {e}")))?;

    // Return only the parity shards
    let parity: Vec<Vec<u8>> = shards[level.data_shards..].to_vec();
    Ok(parity)
}

/// Reconstruct corrupted/missing shards using RS decoding.
/// `shards` has length `data_shards + parity_shards`.
/// Entries that are `None` are missing and will be reconstructed.
pub fn reconstruct_shards(shards: &mut [Option<Vec<u8>>], level: &EccLevel) -> Result<()> {
    let rs = ReedSolomon::new(level.data_shards, level.parity_shards)
        .map_err(|e| Error::Ecc(format!("failed to create RS decoder: {e}")))?;

    rs.reconstruct(shards)
        .map_err(|e| Error::Ecc(format!("RS reconstruct failed: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecc_levels() {
        assert_eq!(EccLevel::LOW.total_shards(), 12);
        assert_eq!(EccLevel::MEDIUM.total_shards(), 14);
        assert_eq!(EccLevel::HIGH.total_shards(), 16);
    }

    #[test]
    fn ecc_from_str() {
        assert!(EccLevel::from_str("low").is_some());
        assert!(EccLevel::from_str("medium").is_some());
        assert!(EccLevel::from_str("high").is_some());
        assert!(EccLevel::from_str("invalid").is_none());
    }

    #[test]
    fn ecc_is_none() {
        assert!(EccLevel::is_none("none"));
        assert!(EccLevel::is_none("off"));
        assert!(EccLevel::is_none("false"));
        assert!(EccLevel::is_none("NONE"));
        assert!(!EccLevel::is_none("low"));
        assert!(!EccLevel::is_none("medium"));
        assert!(!EccLevel::is_none("high"));
    }

    #[test]
    fn overhead_percentages() {
        assert!((EccLevel::LOW.overhead_percent() - 20.0).abs() < 0.1);
        assert!((EccLevel::MEDIUM.overhead_percent() - 40.0).abs() < 0.1);
        assert!((EccLevel::HIGH.overhead_percent() - 60.0).abs() < 0.1);
    }

    #[test]
    fn encode_decode_round_trip() {
        let level = EccLevel::LOW; // RS(10,2)

        // Create 10 data shards of varying sizes
        let mut group = EccGroup::new();
        for i in 0..10 {
            let data = vec![i as u8; 1000 + i * 100];
            group.add_shard(data);
        }

        let parity = encode_parity(&group, &level).unwrap();
        assert_eq!(parity.len(), 2); // 2 parity shards

        // Simulate losing 2 data shards
        let shard_size = group.shard_size;
        let mut recovery_shards: Vec<Option<Vec<u8>>> = group
            .data_shards
            .iter()
            .map(|d| {
                let mut padded = d.clone();
                padded.resize(shard_size, 0);
                Some(padded)
            })
            .collect();

        // Add parity shards
        for p in &parity {
            recovery_shards.push(Some(p.clone()));
        }

        // Save originals for comparison
        let original_0 = recovery_shards[0].clone().unwrap();
        let original_3 = recovery_shards[3].clone().unwrap();

        // Mark 2 shards as missing
        recovery_shards[0] = None;
        recovery_shards[3] = None;

        // Reconstruct
        reconstruct_shards(&mut recovery_shards, &level).unwrap();

        // Verify reconstruction
        assert_eq!(recovery_shards[0].as_ref().unwrap(), &original_0);
        assert_eq!(recovery_shards[3].as_ref().unwrap(), &original_3);
    }

    #[test]
    fn encode_partial_group() {
        let level = EccLevel::LOW;

        // Only 3 data shards (less than 10)
        let mut group = EccGroup::new();
        for i in 0..3 {
            group.add_shard(vec![i as u8; 500]);
        }

        let parity = encode_parity(&group, &level).unwrap();
        assert_eq!(parity.len(), 2);

        // Verify we can reconstruct
        let shard_size = group.shard_size;
        let mut shards: Vec<Option<Vec<u8>>> = group
            .data_shards
            .iter()
            .map(|d| {
                let mut p = d.clone();
                p.resize(shard_size, 0);
                Some(p)
            })
            .collect();

        // Pad to full data_shards count
        while shards.len() < level.data_shards {
            shards.push(Some(vec![0u8; shard_size]));
        }
        for p in &parity {
            shards.push(Some(p.clone()));
        }

        let original = shards[1].clone().unwrap();
        shards[1] = None;

        reconstruct_shards(&mut shards, &level).unwrap();
        assert_eq!(shards[1].as_ref().unwrap(), &original);
    }

    #[test]
    fn too_many_missing_fails() {
        let level = EccLevel::LOW; // Can recover 2

        let mut group = EccGroup::new();
        for i in 0..10 {
            group.add_shard(vec![i as u8; 100]);
        }

        let parity = encode_parity(&group, &level).unwrap();

        let shard_size = group.shard_size;
        let mut shards: Vec<Option<Vec<u8>>> = group
            .data_shards
            .iter()
            .map(|d| {
                let mut p = d.clone();
                p.resize(shard_size, 0);
                Some(p)
            })
            .collect();
        for p in &parity {
            shards.push(Some(p.clone()));
        }

        // Remove 3 shards (more than 2 parity can handle)
        shards[0] = None;
        shards[1] = None;
        shards[2] = None;

        assert!(reconstruct_shards(&mut shards, &level).is_err());
    }
}
