/// Append-only temporal archive support.
///
/// In temporal mode, each `tdg create --append` appends a new generation:
/// - New/changed blocks are written after the previous generation
/// - A new index + footer are appended
/// - Each footer has prev_footer_offset pointing to the prior generation
///
/// `tdg log archive.tg` scans backward through footer chain to list generations.
/// `tdg mount archive.tg@N` reads the Nth generation's index.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{Error, Result};
use crate::format::*;

/// A snapshot (generation) in a temporal archive
#[derive(Debug)]
pub struct Snapshot {
    pub generation: u64,
    pub footer: Footer,
    pub file_count: usize,
}

/// List all snapshots in a temporal archive by walking the footer chain.
pub fn list_snapshots(archive_path: &Path) -> Result<Vec<Snapshot>> {
    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let mut reader = BufReader::new(file);

    let file_size = reader.seek(SeekFrom::End(0))?;
    let mut snapshots = Vec::new();
    let mut footer_offset = file_size - FOOTER_SIZE as u64;
    let mut generation = 0u64;

    loop {
        reader.seek(SeekFrom::Start(footer_offset))?;
        let footer = match Footer::read_from(&mut reader) {
            Ok(f) => f,
            Err(_) => break,
        };

        // Read index to get file count
        reader.seek(SeekFrom::Start(footer.index_offset))?;
        let mut index_data = vec![0u8; footer.index_length as usize];
        reader.read_exact(&mut index_data)?;
        let entries = crate::index::deserialize_index(&index_data, footer.index_length as usize * 10)
            .unwrap_or_default();

        snapshots.push(Snapshot {
            generation,
            footer,
            file_count: entries.len(),
        });

        generation += 1;

        if footer.prev_footer_offset == 0 {
            break;
        }
        footer_offset = footer.prev_footer_offset;
    }

    snapshots.reverse(); // oldest first
    Ok(snapshots)
}
