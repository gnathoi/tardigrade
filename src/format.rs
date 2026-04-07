use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Read, Write};

use crate::error::{Error, Result};

// Magic bytes
pub const ARCHIVE_MAGIC: &[u8; 4] = b"TRDG";
pub const FOOTER_MAGIC: &[u8; 4] = b"TGFT";

// Current format version
pub const FORMAT_VERSION: u16 = 1;

// Header flags
pub const FLAG_ENCRYPTED: u16 = 0x0001;
pub const FLAG_ERASURE_CODED: u16 = 0x0002;
pub const FLAG_APPEND_ONLY: u16 = 0x0004;
pub const FLAG_INCREMENTAL: u16 = 0x0008;

// Block codec values
pub const CODEC_NONE: u8 = 0;
pub const CODEC_ZSTD: u8 = 1;
pub const CODEC_LZ4: u8 = 2;

// Block flag bits
pub const BLOCK_FLAG_ENCRYPTED: u8 = 0x01;
pub const BLOCK_FLAG_ECC: u8 = 0x02;

// BlockRef flag bits
pub const BLOCKREF_FLAG_EXTERNAL: u8 = 0x01;

// Sizes
pub const ARCHIVE_HEADER_SIZE: usize = 16;
pub const BLOCK_HEADER_SIZE: usize = 48;
pub const FOOTER_SIZE: usize = 76; // 8+8+8+8+32+8+4 = 76

/// Hash type used throughout (BLAKE3, 32 bytes)
pub type Hash = [u8; 32];

// ─── ArchiveHeader (16 bytes) ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub flags: u16,
    pub header_checksum: u64, // BLAKE3 truncated to 8 bytes
}

impl ArchiveHeader {
    pub fn new(flags: u16) -> Self {
        let mut header = Self {
            magic: *ARCHIVE_MAGIC,
            version: FORMAT_VERSION,
            flags,
            header_checksum: 0,
        };
        header.header_checksum = header.compute_checksum();
        header
    }

    fn compute_checksum(&self) -> u64 {
        let mut buf = [0u8; 8]; // magic + version + flags
        buf[..4].copy_from_slice(&self.magic);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..8].copy_from_slice(&self.flags.to_le_bytes());
        let hash = blake3::hash(&buf);
        let bytes = hash.as_bytes();
        u64::from_le_bytes(bytes[..8].try_into().unwrap())
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(&self.magic)?;
        w.write_u16::<LittleEndian>(self.version)?;
        w.write_u16::<LittleEndian>(self.flags)?;
        w.write_u64::<LittleEndian>(self.header_checksum)?;
        Ok(())
    }

    pub fn read_from(r: &mut impl Read) -> Result<Self> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != ARCHIVE_MAGIC {
            return Err(Error::InvalidArchive(format!(
                "bad magic: expected TRDG, got {:?}",
                magic
            )));
        }
        let version = r.read_u16::<LittleEndian>()?;
        if version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let flags = r.read_u16::<LittleEndian>()?;
        let header_checksum = r.read_u64::<LittleEndian>()?;

        let header = Self {
            magic,
            version,
            flags,
            header_checksum,
        };

        if header.header_checksum != header.compute_checksum() {
            return Err(Error::InvalidArchive(
                "archive header checksum mismatch".into(),
            ));
        }

        Ok(header)
    }

    pub fn is_encrypted(&self) -> bool {
        self.flags & FLAG_ENCRYPTED != 0
    }

    pub fn is_erasure_coded(&self) -> bool {
        self.flags & FLAG_ERASURE_CODED != 0
    }

    pub fn is_append_only(&self) -> bool {
        self.flags & FLAG_APPEND_ONLY != 0
    }

    pub fn is_incremental(&self) -> bool {
        self.flags & FLAG_INCREMENTAL != 0
    }
}

// ─── BlockHeader (48 bytes) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockHeader {
    pub hash: Hash,             // BLAKE3 of uncompressed data (32)
    pub compressed_size: u32,   // (4)
    pub uncompressed_size: u32, // (4)
    pub codec: u8,              // (1)
    pub flags: u8,              // (1)
    pub ecc_shard_count: u8,    // (1)
    pub reserved: u8,           // (1)
    pub checksum: u32,          // CRC32 of first 44 bytes (4)
} // total = 48

impl BlockHeader {
    pub fn new(hash: Hash, compressed_size: u32, uncompressed_size: u32, codec: u8) -> Self {
        let mut hdr = Self {
            hash,
            compressed_size,
            uncompressed_size,
            codec,
            flags: 0,
            ecc_shard_count: 0,
            reserved: 0,
            checksum: 0,
        };
        hdr.checksum = hdr.compute_crc();
        hdr
    }

    fn compute_crc(&self) -> u32 {
        let mut buf = [0u8; 44];
        buf[..32].copy_from_slice(&self.hash);
        buf[32..36].copy_from_slice(&self.compressed_size.to_le_bytes());
        buf[36..40].copy_from_slice(&self.uncompressed_size.to_le_bytes());
        buf[40] = self.codec;
        buf[41] = self.flags;
        buf[42] = self.ecc_shard_count;
        buf[43] = self.reserved;
        crc32fast::hash(&buf)
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(&self.hash)?;
        w.write_u32::<LittleEndian>(self.compressed_size)?;
        w.write_u32::<LittleEndian>(self.uncompressed_size)?;
        w.write_all(&[self.codec, self.flags, self.ecc_shard_count, self.reserved])?;
        w.write_u32::<LittleEndian>(self.checksum)?;
        Ok(())
    }

    pub fn read_from(r: &mut impl Read) -> Result<Self> {
        let mut hash = [0u8; 32];
        r.read_exact(&mut hash)?;
        let compressed_size = r.read_u32::<LittleEndian>()?;
        let uncompressed_size = r.read_u32::<LittleEndian>()?;
        let mut flags_buf = [0u8; 4];
        r.read_exact(&mut flags_buf)?;
        let checksum = r.read_u32::<LittleEndian>()?;

        let hdr = Self {
            hash,
            compressed_size,
            uncompressed_size,
            codec: flags_buf[0],
            flags: flags_buf[1],
            ecc_shard_count: flags_buf[2],
            reserved: flags_buf[3],
            checksum,
        };

        if hdr.checksum != hdr.compute_crc() {
            return Err(Error::HeaderCrcMismatch { offset: 0 });
        }

        Ok(hdr)
    }
}

// ─── FileEntry (msgpack serialized) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Raw bytes path (not necessarily UTF-8 on Linux)
    pub path: Vec<u8>,
    pub file_type: FileType,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime_ns: i64,
    pub size: u64,
    pub block_refs: Vec<BlockRef>,
    #[serde(default)]
    pub xattrs: HashMap<String, Vec<u8>>,
    #[serde(default)]
    pub snapshot_id: Option<u64>,
}

impl FileEntry {
    /// Get path as a lossy UTF-8 string for display
    pub fn path_display(&self) -> String {
        String::from_utf8_lossy(&self.path).into_owned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Symlink(Vec<u8>),  // target path as raw bytes
    Hardlink(Vec<u8>), // target path as raw bytes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRef {
    pub hash: Hash,
    pub offset: u64,      // offset of block in archive
    pub slice_start: u32, // offset within decompressed block
    pub slice_len: u32,   // length within decompressed block
    pub flags: u8,        // BLOCKREF_FLAG_EXTERNAL for incremental
    pub reserved: [u8; 3],
}

// ─── SparseExtent ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparseExtent {
    pub offset: u64,
    pub length: u64,
}

// ─── Footer (76 bytes) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
    pub index_offset: u64,
    pub index_length: u64,
    pub redundant_index_offset: u64,
    pub block_count: u64,
    pub root_hash: Hash,
    pub prev_footer_offset: u64, // for temporal archives, 0 if none
    pub magic: [u8; 4],
}

impl Footer {
    pub fn new(
        index_offset: u64,
        index_length: u64,
        redundant_index_offset: u64,
        block_count: u64,
        root_hash: Hash,
    ) -> Self {
        Self {
            index_offset,
            index_length,
            redundant_index_offset,
            block_count,
            root_hash,
            prev_footer_offset: 0,
            magic: *FOOTER_MAGIC,
        }
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_u64::<LittleEndian>(self.index_offset)?;
        w.write_u64::<LittleEndian>(self.index_length)?;
        w.write_u64::<LittleEndian>(self.redundant_index_offset)?;
        w.write_u64::<LittleEndian>(self.block_count)?;
        w.write_all(&self.root_hash)?;
        w.write_u64::<LittleEndian>(self.prev_footer_offset)?;
        w.write_all(&self.magic)?;
        Ok(())
    }

    pub fn read_from(r: &mut impl Read) -> Result<Self> {
        let index_offset = r.read_u64::<LittleEndian>()?;
        let index_length = r.read_u64::<LittleEndian>()?;
        let redundant_index_offset = r.read_u64::<LittleEndian>()?;
        let block_count = r.read_u64::<LittleEndian>()?;
        let mut root_hash = [0u8; 32];
        r.read_exact(&mut root_hash)?;
        let prev_footer_offset = r.read_u64::<LittleEndian>()?;
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;

        if &magic != FOOTER_MAGIC {
            return Err(Error::InvalidArchive(format!(
                "bad footer magic: expected TGFT, got {:?}",
                magic
            )));
        }

        Ok(Self {
            index_offset,
            index_length,
            redundant_index_offset,
            block_count,
            root_hash,
            prev_footer_offset,
            magic,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn archive_header_round_trip() {
        let header = ArchiveHeader::new(FLAG_ENCRYPTED | FLAG_ERASURE_CODED);
        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), ARCHIVE_HEADER_SIZE);

        let mut cursor = Cursor::new(&buf);
        let decoded = ArchiveHeader::read_from(&mut cursor).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn block_header_round_trip() {
        let hash = blake3::hash(b"test data").into();
        let hdr = BlockHeader::new(hash, 100, 256, CODEC_ZSTD);
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), BLOCK_HEADER_SIZE);

        let mut cursor = Cursor::new(&buf);
        let decoded = BlockHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr, decoded);
    }

    #[test]
    fn footer_round_trip() {
        let root_hash = blake3::hash(b"root").into();
        let footer = Footer::new(1000, 500, 2000, 42, root_hash);
        let mut buf = Vec::new();
        footer.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), FOOTER_SIZE);

        let mut cursor = Cursor::new(&buf);
        let decoded = Footer::read_from(&mut cursor).unwrap();
        assert_eq!(footer, decoded);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut buf = vec![0u8; 16];
        buf[..4].copy_from_slice(b"NOPE");
        let mut cursor = Cursor::new(&buf);
        assert!(ArchiveHeader::read_from(&mut cursor).is_err());
    }

    #[test]
    fn corrupted_header_checksum_rejected() {
        let header = ArchiveHeader::new(0);
        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();
        // Flip a bit in the flags
        buf[6] ^= 0x01;
        let mut cursor = Cursor::new(&buf);
        assert!(ArchiveHeader::read_from(&mut cursor).is_err());
    }

    #[test]
    fn corrupted_block_header_crc_rejected() {
        let hash = blake3::hash(b"data").into();
        let hdr = BlockHeader::new(hash, 50, 100, CODEC_NONE);
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        // Flip a bit in compressed_size
        buf[32] ^= 0x01;
        let mut cursor = Cursor::new(&buf);
        assert!(BlockHeader::read_from(&mut cursor).is_err());
    }

    #[test]
    fn file_entry_msgpack_round_trip() {
        let entry = FileEntry {
            path: b"/tmp/test.txt".to_vec(),
            file_type: FileType::File,
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            mtime_ns: 1700000000_000000000,
            size: 1024,
            block_refs: vec![BlockRef {
                hash: [0xAA; 32],
                offset: 100,
                slice_start: 0,
                slice_len: 1024,
                flags: 0,
                reserved: [0; 3],
            }],
            xattrs: HashMap::new(),
            snapshot_id: None,
        };

        let encoded = rmp_serde::to_vec(&entry).unwrap();
        let decoded: FileEntry = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(entry.path, decoded.path);
        assert_eq!(entry.size, decoded.size);
        assert_eq!(entry.block_refs.len(), decoded.block_refs.len());
    }
}
