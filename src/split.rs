// Volume splitting and reassembly.
//
// `tdg split archive.tg --size 4G` splits at block boundaries.
// `tdg join part1.tg part2.tg ... -o archive.tg` reassembles.
//
// Split produces numbered files: archive.001.tg, archive.002.tg, ...
// Each volume is a valid .tg archive fragment. Simple concatenation of
// the raw bytes (minus headers on volumes 2+) reconstructs the original.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::extract::read_footer;
use crate::format::*;

/// Split an archive into volumes of approximately `max_size` bytes.
/// Returns the paths of the created volumes.
pub fn split_archive(archive_path: &Path, max_size: u64) -> Result<Vec<PathBuf>> {
    if max_size < 1024 {
        return Err(Error::Volume("volume size must be at least 1 KB".into()));
    }

    let file = File::open(archive_path).map_err(|e| Error::io_path(archive_path, e))?;
    let file_size = file
        .metadata()
        .map_err(|e| Error::io_path(archive_path, e))?
        .len();
    let mut reader = BufReader::new(file);

    // If archive fits in one volume, just copy it
    if file_size <= max_size {
        return Err(Error::Volume("archive already fits in one volume".into()));
    }

    // Read and validate header
    let _header = ArchiveHeader::read_from(&mut reader)?;
    let footer = read_footer(&mut reader)?;

    // The block section starts after the header (and optional key encap)
    // and ends at the index offset.
    let block_section_end = footer.index_offset;

    // Collect block boundaries by scanning block headers
    let mut block_boundaries: Vec<u64> = Vec::new();
    let mut pos = ARCHIVE_HEADER_SIZE as u64;
    reader.seek(SeekFrom::Start(pos))?;

    while pos < block_section_end {
        block_boundaries.push(pos);
        let bh = match BlockHeader::read_from(&mut reader) {
            Ok(h) => h,
            Err(_) => break,
        };
        pos += BLOCK_HEADER_SIZE as u64 + bh.compressed_size as u64;
        reader.seek(SeekFrom::Start(pos))?;
    }

    // Determine split points at block boundaries
    let stem = archive_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = archive_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = archive_path.parent().unwrap_or(Path::new("."));

    let mut volumes: Vec<PathBuf> = Vec::new();
    let mut vol_start: u64 = 0;
    let mut vol_num: u32 = 1;

    // Walk through and find split points
    let mut split_points: Vec<u64> = Vec::new();
    for &boundary in &block_boundaries {
        if boundary - vol_start >= max_size && boundary > vol_start {
            split_points.push(boundary);
            vol_start = boundary;
        }
    }

    // Now split the file
    reader.seek(SeekFrom::Start(0))?;
    let mut file_pos: u64 = 0;

    let mut split_iter = split_points.iter().peekable();

    loop {
        let vol_path = parent.join(format!("{}.{:03}{}", stem, vol_num, ext));
        let vol_file = File::create(&vol_path).map_err(|e| Error::io_path(&vol_path, e))?;
        let mut writer = BufWriter::new(vol_file);

        let vol_end = if let Some(&&split_point) = split_iter.peek() {
            split_iter.next();
            split_point
        } else {
            file_size
        };

        let bytes_to_write = vol_end - file_pos;
        reader.seek(SeekFrom::Start(file_pos))?;
        copy_bytes(&mut reader, &mut writer, bytes_to_write)?;
        writer.flush()?;

        volumes.push(vol_path);
        file_pos = vol_end;
        vol_num += 1;

        if file_pos >= file_size {
            break;
        }
    }

    Ok(volumes)
}

/// Join volume files back into a single archive.
/// Volumes must be provided in order.
pub fn join_volumes(volume_paths: &[PathBuf], output_path: &Path) -> Result<()> {
    if volume_paths.is_empty() {
        return Err(Error::Volume("no volumes provided".into()));
    }

    let out_file = File::create(output_path).map_err(|e| Error::io_path(output_path, e))?;
    let mut writer = BufWriter::new(out_file);

    for vol_path in volume_paths {
        let file = File::open(vol_path).map_err(|e| Error::io_path(vol_path, e))?;
        let mut reader = BufReader::new(file);
        std::io::copy(&mut reader, &mut writer).map_err(|e| Error::io_path(vol_path, e))?;
    }

    writer.flush()?;

    // Verify the joined archive is valid
    let file = File::open(output_path).map_err(|e| Error::io_path(output_path, e))?;
    let mut reader = BufReader::new(file);
    ArchiveHeader::read_from(&mut reader)?;
    read_footer(&mut reader)?;

    Ok(())
}

fn copy_bytes(reader: &mut impl Read, writer: &mut impl Write, mut count: u64) -> Result<()> {
    let mut buf = [0u8; 64 * 1024];
    while count > 0 {
        let to_read = std::cmp::min(count, buf.len() as u64) as usize;
        reader.read_exact(&mut buf[..to_read])?;
        writer.write_all(&buf[..to_read])?;
        count -= to_read as u64;
    }
    Ok(())
}

/// Parse a human-readable size string (e.g., "4G", "100M", "500K").
pub fn parse_size(s: &str) -> std::result::Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size string".into());
    }

    let (num_str, multiplier) = if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024u64)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len() - 1], 1024 * 1024u64)
    } else if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len() - 1], 1024u64)
    } else {
        (s, 1u64)
    };

    let num: f64 = num_str.parse().map_err(|e| format!("invalid size: {e}"))?;
    Ok((num * multiplier as f64) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{CreateOptions, create_archive};

    #[test]
    fn parse_size_units() {
        assert_eq!(parse_size("1K").unwrap(), 1024);
        assert_eq!(parse_size("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("4G").unwrap(), 4 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("500").unwrap(), 500);
    }

    #[test]
    fn parse_size_invalid() {
        assert!(parse_size("").is_err());
        assert!(parse_size("abc").is_err());
    }

    #[test]
    fn split_and_join_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create a non-trivial archive (needs to be big enough to split)
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        for i in 0..50 {
            std::fs::write(
                src.join(format!("file_{i}.txt")),
                format!("content {i}").repeat(2000),
            )
            .unwrap();
        }

        let archive_path = dir.path().join("test.tg");
        create_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();

        let archive_size = std::fs::metadata(&archive_path).unwrap().len();

        // Split into small volumes (ensure each volume >= 1KB)
        let max_vol_size = std::cmp::max(archive_size / 3, 2048);
        let volumes = split_archive(&archive_path, max_vol_size).unwrap();
        assert!(volumes.len() >= 2);

        // Verify volume sizes
        for vol in &volumes[..volumes.len() - 1] {
            let size = std::fs::metadata(vol).unwrap().len();
            assert!(size <= max_vol_size + BLOCK_HEADER_SIZE as u64 + 1024 * 1024);
        }

        // Join back
        let joined_path = dir.path().join("joined.tg");
        join_volumes(&volumes, &joined_path).unwrap();

        // Verify the joined archive matches the original
        let original = std::fs::read(&archive_path).unwrap();
        let joined = std::fs::read(&joined_path).unwrap();
        assert_eq!(original, joined);

        // Extract and verify content
        let dest = dir.path().join("extracted");
        crate::extract::extract_archive(&joined_path, &dest).unwrap();
        for i in 0..50 {
            let content = std::fs::read_to_string(dest.join(format!("file_{i}.txt"))).unwrap();
            assert_eq!(content, format!("content {i}").repeat(2000));
        }
    }

    #[test]
    fn split_small_archive_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("tiny.txt"), "small").unwrap();

        let archive_path = dir.path().join("small.tg");
        create_archive(&archive_path, &[src.as_path()], &CreateOptions::default()).unwrap();

        // Archive fits in one volume
        let result = split_archive(&archive_path, 1024 * 1024 * 1024);
        assert!(result.is_err());
    }
}
