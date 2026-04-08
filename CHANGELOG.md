# Changelog

All notable changes to tardigrade will be documented in this file.

## [0.2.0] - 2026-04-08

### Added
- tar.zst/tar.gz/tar read compatibility: `tdg extract` auto-detects legacy tar formats, `tdg convert` migrates to .tg with dedup
- Volume splitting: `tdg split --size 4G` splits at block boundaries, `tdg join` reassembles
- Archive merging: `tdg merge a.tg b.tg -o merged.tg` with automatic cross-archive dedup
- Incremental archives: `--incremental base.tg` stores only new/changed blocks, `--base` on extract
- Temporal/append-only archives: `--append` adds generations, `tdg log` lists them, `--generation N` extracts specific snapshots
- Reed-Solomon erasure coding: `--ecc low|medium|high` with RS(10,2/4/6) encode/decode/reconstruct
- 19 end-to-end integration tests exercising every CLI command

### Changed
- Default zstd level remains 9
- Version bump to 0.2.0

## [0.1.0] - 2026-04-07

### Added
- Initial release: parallel zstd/lz4 compression, content-addressed dedup, BLAKE3 integrity
- FastCDC content-defined chunking, ChaCha20-Poly1305 encryption
- .gitignore-aware file walking, detailed verification with damage mapping
- Beautiful CLI with progress bars, throughput, and compression ratios
