use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{Error, Result};
use crate::extract::{read_footer, read_index};
use crate::format::*;

/// Detailed verification result
#[derive(Debug)]
pub struct VerifyReport {
    pub blocks_checked: u64,
    pub blocks_ok: u64,
    pub blocks_corrupted: u64,
    pub header_ok: bool,
    pub footer_ok: bool,
    pub index_ok: bool,
    pub corrupted_blocks: Vec<CorruptedBlock>,
    pub affected_files: Vec<String>,
}

#[derive(Debug)]
pub struct CorruptedBlock {
    pub offset: u64,
    pub expected_hash: String,
    #[allow(dead_code)]
    pub actual_hash: String,
    pub error: String,
}

/// Full integrity verification of an archive.
/// Checks header, every block's BLAKE3 hash and CRC32, index, and footer.
pub fn verify_full(archive_path: &Path) -> Result<VerifyReport> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    let mut report = VerifyReport {
        blocks_checked: 0,
        blocks_ok: 0,
        blocks_corrupted: 0,
        header_ok: false,
        footer_ok: false,
        index_ok: false,
        corrupted_blocks: vec![],
        affected_files: vec![],
    };

    // Check header
    match ArchiveHeader::read_from(&mut reader) {
        Ok(_) => report.header_ok = true,
        Err(e) => {
            return Err(Error::InvalidArchive(format!(
                "header verification failed: {e}"
            )));
        }
    }

    // Check footer
    let footer = match read_footer(&mut reader) {
        Ok(f) => {
            report.footer_ok = true;
            f
        }
        Err(e) => {
            return Err(Error::InvalidArchive(format!(
                "footer verification failed: {e}"
            )));
        }
    };

    // Check index
    let entries = match read_index(&mut reader, &footer) {
        Ok(e) => {
            report.index_ok = true;
            e
        }
        Err(e) => {
            return Err(Error::InvalidArchive(format!(
                "index verification failed: {e}"
            )));
        }
    };

    // Check all unique blocks
    let mut checked_offsets = HashSet::new();
    let mut corrupted_offsets = HashSet::new();

    for entry in &entries {
        for block_ref in &entry.block_refs {
            if checked_offsets.contains(&block_ref.offset) {
                // Already checked, just note if it was corrupted
                if corrupted_offsets.contains(&block_ref.offset)
                    && !report.affected_files.contains(&entry.path_display())
                {
                    report.affected_files.push(entry.path_display());
                }
                continue;
            }
            checked_offsets.insert(block_ref.offset);
            report.blocks_checked += 1;

            match verify_block(&mut reader, block_ref.offset) {
                Ok(()) => {
                    report.blocks_ok += 1;
                }
                Err(detail) => {
                    report.blocks_corrupted += 1;
                    corrupted_offsets.insert(block_ref.offset);
                    report.corrupted_blocks.push(detail);
                    if !report.affected_files.contains(&entry.path_display()) {
                        report.affected_files.push(entry.path_display());
                    }
                }
            }
        }
    }

    Ok(report)
}

fn verify_block(
    reader: &mut (impl Read + Seek),
    offset: u64,
) -> std::result::Result<(), CorruptedBlock> {
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|e| CorruptedBlock {
            offset,
            expected_hash: String::new(),
            actual_hash: String::new(),
            error: format!("seek failed: {e}"),
        })?;

    let header = BlockHeader::read_from(reader).map_err(|e| CorruptedBlock {
        offset,
        expected_hash: String::new(),
        actual_hash: String::new(),
        error: format!("header CRC failed: {e}"),
    })?;

    let mut compressed = vec![0u8; header.compressed_size as usize];
    reader
        .read_exact(&mut compressed)
        .map_err(|e| CorruptedBlock {
            offset,
            expected_hash: hex::encode(header.hash),
            actual_hash: String::new(),
            error: format!("read failed: {e}"),
        })?;

    let data =
        crate::compress::decompress(&compressed, header.codec, header.uncompressed_size as usize)
            .map_err(|e| CorruptedBlock {
            offset,
            expected_hash: hex::encode(header.hash),
            actual_hash: String::new(),
            error: format!("decompression failed: {e}"),
        })?;

    let actual_hash: Hash = blake3::hash(&data).into();
    if actual_hash != header.hash {
        return Err(CorruptedBlock {
            offset,
            expected_hash: hex::encode(header.hash),
            actual_hash: hex::encode(actual_hash),
            error: "BLAKE3 hash mismatch".into(),
        });
    }

    Ok(())
}
