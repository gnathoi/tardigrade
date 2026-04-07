use crate::error::{Error, Result};
use crate::format::{CODEC_LZ4, CODEC_NONE, CODEC_ZSTD};

/// Compress data using the specified codec
pub fn compress(data: &[u8], codec: u8, level: i32) -> Result<Vec<u8>> {
    match codec {
        CODEC_NONE => Ok(data.to_vec()),
        CODEC_ZSTD => {
            zstd::bulk::compress(data, level).map_err(|e| Error::Compression(format!("zstd: {e}")))
        }
        CODEC_LZ4 => Ok(lz4_flex::compress_prepend_size(data)),
        _ => Err(Error::UnknownCodec(codec)),
    }
}

/// Decompress data using the specified codec
pub fn decompress(data: &[u8], codec: u8, expected_size: usize) -> Result<Vec<u8>> {
    match codec {
        CODEC_NONE => Ok(data.to_vec()),
        CODEC_ZSTD => zstd::bulk::decompress(data, expected_size)
            .map_err(|e| Error::Decompression(format!("zstd: {e}"))),
        CODEC_LZ4 => lz4_flex::decompress_size_prepended(data)
            .map_err(|e| Error::Decompression(format!("lz4: {e}"))),
        _ => Err(Error::UnknownCodec(codec)),
    }
}

/// Parse a codec name string to codec byte
pub fn codec_from_str(s: &str) -> Result<u8> {
    match s.to_lowercase().as_str() {
        "zstd" | "zstandard" => Ok(CODEC_ZSTD),
        "lz4" => Ok(CODEC_LZ4),
        "none" | "store" => Ok(CODEC_NONE),
        _ => Err(Error::UnknownCodec(0)),
    }
}

/// Get human-readable codec name
pub fn codec_name(codec: u8) -> &'static str {
    match codec {
        CODEC_NONE => "none",
        CODEC_ZSTD => "zstd",
        CODEC_LZ4 => "lz4",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zstd_round_trip() {
        let data = b"hello tardigrade! this is some test data for compression.";
        let compressed = compress(data, CODEC_ZSTD, 3).unwrap();
        let decompressed = decompress(&compressed, CODEC_ZSTD, data.len()).unwrap();
        assert_eq!(data.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn lz4_round_trip() {
        let data = b"lz4 compression test data repeated repeated repeated";
        let compressed = compress(data, CODEC_LZ4, 0).unwrap();
        let decompressed = decompress(&compressed, CODEC_LZ4, data.len()).unwrap();
        assert_eq!(data.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn none_round_trip() {
        let data = b"no compression";
        let compressed = compress(data, CODEC_NONE, 0).unwrap();
        assert_eq!(compressed, data);
        let decompressed = decompress(&compressed, CODEC_NONE, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn unknown_codec_error() {
        assert!(compress(b"data", 99, 0).is_err());
        assert!(decompress(b"data", 99, 4).is_err());
    }

    #[test]
    fn zstd_compresses_repetitive_data() {
        let data = vec![0x42u8; 100_000];
        let compressed = compress(&data, CODEC_ZSTD, 3).unwrap();
        assert!(compressed.len() < data.len() / 10);
    }
}
