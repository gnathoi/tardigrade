/// Reed-Solomon erasure coding support.
///
/// ECC levels:
/// - Low:    RS(10,2) — recovers up to 2 lost shards per group (~17% overhead)
/// - Medium: RS(10,4) — recovers up to 4 lost shards per group (~40% overhead)
/// - High:   RS(10,6) — recovers up to 6 lost shards per group (~60% overhead)
///
/// This module is stubbed for the v0.1 foundation. The wire format
/// supports ECC (ecc_shard_count field in BlockHeader), and the CLI
/// accepts --ecc flags, but actual encoding/decoding is not yet wired
/// into the archive pipeline.

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

    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }

    pub fn overhead_percent(&self) -> f64 {
        (self.parity_shards as f64 / self.data_shards as f64) * 100.0
    }
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
    fn overhead_percentages() {
        assert!((EccLevel::LOW.overhead_percent() - 20.0).abs() < 0.1);
        assert!((EccLevel::MEDIUM.overhead_percent() - 40.0).abs() < 0.1);
        assert!((EccLevel::HIGH.overhead_percent() - 60.0).abs() < 0.1);
    }
}
