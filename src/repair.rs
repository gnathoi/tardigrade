use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::erasure::{self, EccLevel};
use crate::error::{Error, Result};
use crate::extract::{read_footer, read_index};
use crate::format::*;

/// Metadata about one ECC group in the archive.
#[derive(Debug)]
pub struct EccGroupInfo {
    pub data_block_offsets: Vec<u64>,
    pub parity_block_offsets: Vec<u64>,
    pub shard_size: usize,
}

/// Result of a repair operation.
#[derive(Debug)]
pub struct RepairReport {
    pub scanned: u64,
    pub corrupted: u64,
    pub recovered: u64,
    pub unrecoverable: u64,
}

/// Scan an archive and return its ECC group structure.
pub fn scan_ecc_groups(archive_path: &Path) -> Result<Vec<EccGroupInfo>> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    let header = ArchiveHeader::read_from(&mut reader)?;
    if !header.is_erasure_coded() {
        return Ok(vec![]);
    }

    let footer = read_footer(&mut reader)?;

    // Find the first block offset — skip header and optional key encapsulation
    let first_block_offset = {
        let entries = read_index(&mut reader, &footer)?;
        entries
            .iter()
            .flat_map(|e| e.block_refs.iter().map(|r| r.offset))
            .min()
            .unwrap_or(ARCHIVE_HEADER_SIZE as u64)
    };

    scan_groups_from(&mut reader, first_block_offset, footer.index_offset)
}

/// Scan block headers linearly to identify ECC groups.
fn scan_groups_from(
    reader: &mut (impl Read + Seek),
    start_offset: u64,
    end_offset: u64,
) -> Result<Vec<EccGroupInfo>> {
    let mut groups = Vec::new();
    let mut current_data: Vec<u64> = Vec::new();
    let mut current_parity: Vec<u64> = Vec::new();
    let mut current_shard_size: usize = 0;
    let mut current_parity_count: usize = 0;
    let mut offset = start_offset;

    while offset < end_offset {
        reader.seek(SeekFrom::Start(offset))?;
        let block_header = match BlockHeader::read_from(reader) {
            Ok(h) => h,
            Err(_) => break,
        };

        let block_size = BLOCK_HEADER_SIZE as u64 + block_header.compressed_size as u64;

        if block_header.is_parity() {
            current_parity.push(offset);
            current_shard_size = block_header.uncompressed_size as usize;
            current_parity_count = block_header.ecc_shard_count as usize;

            if current_parity.len() == current_parity_count {
                groups.push(EccGroupInfo {
                    data_block_offsets: std::mem::take(&mut current_data),
                    parity_block_offsets: std::mem::take(&mut current_parity),
                    shard_size: current_shard_size,
                });
            }
        } else {
            current_data.push(offset);
        }

        offset += block_size;
    }

    Ok(groups)
}

/// Read the raw compressed data from a block (no decompression or hash check).
fn read_raw_block(
    reader: &mut (impl Read + Seek),
    offset: u64,
) -> Result<(BlockHeader, Vec<u8>)> {
    reader.seek(SeekFrom::Start(offset))?;
    let header = BlockHeader::read_from(reader)?;
    let mut data = vec![0u8; header.compressed_size as usize];
    reader.read_exact(&mut data)?;
    Ok((header, data))
}

/// Verify a single block's BLAKE3 hash. Returns Ok(()) if valid.
fn verify_block_hash(
    reader: &mut (impl Read + Seek),
    offset: u64,
) -> std::result::Result<(), u64> {
    let (header, raw) = read_raw_block(reader, offset).map_err(|_| offset)?;

    if header.is_parity() {
        let actual: Hash = blake3::hash(&raw).into();
        if actual != header.hash {
            return Err(offset);
        }
        return Ok(());
    }

    let data = crate::compress::decompress(&raw, header.codec, header.uncompressed_size as usize)
        .map_err(|_| offset)?;
    let actual: Hash = blake3::hash(&data).into();
    if actual != header.hash {
        return Err(offset);
    }
    Ok(())
}

/// A pending repair: offset in the archive + reconstructed compressed data.
struct PendingRepair {
    offset: u64,         // block offset in archive
    compressed_size: u32, // original compressed_size from header
    data: Vec<u8>,       // reconstructed shard (padded)
}

/// Repair an archive by reconstructing corrupted blocks using ECC parity.
pub fn repair_archive(archive_path: &Path) -> Result<RepairReport> {
    let mut report = RepairReport {
        scanned: 0,
        corrupted: 0,
        recovered: 0,
        unrecoverable: 0,
    };

    // Collect all repairs needed
    let repairs = {
        let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
        let mut reader = BufReader::new(file);

        let header = ArchiveHeader::read_from(&mut reader)?;
        let footer = read_footer(&mut reader)?;
        let entries = read_index(&mut reader, &footer)?;

        if !header.is_erasure_coded() {
            let mut checked = std::collections::HashSet::new();
            for entry in &entries {
                for block_ref in &entry.block_refs {
                    if checked.insert(block_ref.offset) {
                        report.scanned += 1;
                        if verify_block_hash(&mut reader, block_ref.offset).is_err() {
                            report.corrupted += 1;
                            report.unrecoverable += 1;
                        }
                    }
                }
            }
            return Ok(report);
        }

        let first_block_offset = entries
            .iter()
            .flat_map(|e| e.block_refs.iter().map(|r| r.offset))
            .min()
            .unwrap_or(ARCHIVE_HEADER_SIZE as u64);

        let groups = scan_groups_from(&mut reader, first_block_offset, footer.index_offset)?;
        let mut pending: Vec<PendingRepair> = Vec::new();

        for group_info in &groups {
            let data_count = group_info.data_block_offsets.len();
            let parity_count = group_info.parity_block_offsets.len();

            let level = EccLevel {
                data_shards: 10,
                parity_shards: parity_count,
            };

            let mut corrupted_indices = Vec::new();
            let mut shards: Vec<Option<Vec<u8>>> = Vec::new();

            // Read data shards
            for (i, &off) in group_info.data_block_offsets.iter().enumerate() {
                report.scanned += 1;
                match read_raw_block(&mut reader, off) {
                    Ok((hdr, raw)) => {
                        let valid = match crate::compress::decompress(
                            &raw,
                            hdr.codec,
                            hdr.uncompressed_size as usize,
                        ) {
                            Ok(data) => blake3::hash(&data).as_bytes() == &hdr.hash,
                            Err(_) => false,
                        };
                        if valid {
                            let mut padded = raw;
                            padded.resize(group_info.shard_size, 0);
                            shards.push(Some(padded));
                        } else {
                            report.corrupted += 1;
                            corrupted_indices.push(i);
                            shards.push(None);
                        }
                    }
                    Err(_) => {
                        report.corrupted += 1;
                        corrupted_indices.push(i);
                        shards.push(None);
                    }
                }
            }

            while shards.len() < level.data_shards {
                shards.push(Some(vec![0u8; group_info.shard_size]));
            }

            // Read parity shards
            for (i, &off) in group_info.parity_block_offsets.iter().enumerate() {
                report.scanned += 1;
                match read_raw_block(&mut reader, off) {
                    Ok((hdr, raw)) => {
                        let actual: Hash = blake3::hash(&raw).into();
                        if actual == hdr.hash {
                            let mut padded = raw;
                            padded.resize(group_info.shard_size, 0);
                            shards.push(Some(padded));
                        } else {
                            report.corrupted += 1;
                            corrupted_indices.push(data_count + i);
                            shards.push(None);
                        }
                    }
                    Err(_) => {
                        report.corrupted += 1;
                        corrupted_indices.push(data_count + i);
                        shards.push(None);
                    }
                }
            }

            if corrupted_indices.is_empty() {
                continue;
            }

            match erasure::reconstruct_shards(&mut shards, &level) {
                Ok(()) => {
                    for &idx in &corrupted_indices {
                        let shard = shards[idx].as_ref().unwrap().clone();
                        let (off, compressed_size) = if idx < data_count {
                            let off = group_info.data_block_offsets[idx];
                            let hdr = read_raw_block(&mut reader, off)
                                .map(|(h, _)| h)
                                .unwrap_or_else(|_| {
                                    // Header might also be damaged; use shard size as fallback
                                    BlockHeader::new([0; 32], group_info.shard_size as u32, 0, 0)
                                });
                            (off, hdr.compressed_size)
                        } else {
                            let parity_idx = idx - data_count;
                            let off = group_info.parity_block_offsets[parity_idx];
                            let hdr = read_raw_block(&mut reader, off)
                                .map(|(h, _)| h)
                                .unwrap_or_else(|_| {
                                    BlockHeader::new([0; 32], group_info.shard_size as u32, 0, 0)
                                });
                            (off, hdr.compressed_size)
                        };
                        pending.push(PendingRepair {
                            offset: off,
                            compressed_size,
                            data: shard,
                        });
                        report.recovered += 1;
                    }
                }
                Err(_) => {
                    report.unrecoverable += corrupted_indices.len() as u64;
                }
            }
        }

        pending
    }; // reader is dropped here

    // Write all repairs
    if !repairs.is_empty() {
        let mut write_file = OpenOptions::new()
            .write(true)
            .open(archive_path)
            .map_err(|e| Error::io_path(archive_path, e))?;

        for repair in &repairs {
            write_file.seek(SeekFrom::Start(
                repair.offset + BLOCK_HEADER_SIZE as u64,
            ))?;
            write_file.write_all(&repair.data[..repair.compressed_size as usize])?;
        }

        write_file.flush()?;
    }

    Ok(report)
}
