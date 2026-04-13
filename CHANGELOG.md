# Changelog

All notable changes to tardigrade will be documented in this file.

## [0.7.2] - 2026-04-13

### Added
- `tdg completions <shell>` prints a tab-completion script for `bash`, `zsh`, `fish`, `powershell`, or `elvish`. After install, the shell completes subcommands, flags, and paths.
- README now has a full per-command reference covering every subcommand, every flag, arguments, defaults, and examples. Run `tdg <command> --help` for the same information from the CLI.

## [0.7.1] - 2026-04-13

### Added
- `extract` now shows a live progress bar, spinner, elapsed time, and ETA — same style as `create`. Covers plain, encrypted, incremental, temporal, and tar/tar.gz/tar.zst extraction. For streaming tar formats, progress is measured against on-disk file size. Suppressed with `--quiet`.

## [0.7.0] - 2026-04-09

### Added
- ECC now works with encrypted archives. Self-healing parity is computed over ciphertext (encrypt-then-ECC). Previously encryption silently disabled ECC.
- `--encrypt-allow-dedup` flag: opt in to dedup with encryption if you accept the privacy tradeoff (content-equality leakage). Off by default.
- Argon2id key derivation for encryption (64 MB memory, 3 iterations). Replaces BLAKE3 derive_key. **BREAKING:** encrypted archives from prior versions cannot be decrypted.

### Changed
- Encrypted archives now get ECC protection by default (no flag change needed).

## [0.6.1] - 2026-04-09

### Fixed
- Dedup store now capped at 2M entries (~200 MB) to prevent OOM on very large archives. Blocks beyond the cap are written normally but miss dedup opportunities. A warning is shown if overflow occurs.
- Tar-compat extraction now propagates errors instead of silently swallowing directory creation and symlink failures.

### Changed
- Shared block utilities extracted to `src/block.rs`, removing ~160 lines of duplicated code across archive, temporal, incremental, and extract modules.

## [0.6.0] - 2026-04-09

### Added
- `tdg cat archive.tg path/to/file.txt` — print a single file from an archive to stdout without extracting the whole archive. Pipe-friendly, supports encrypted archives (`--decrypt`), multi-block files, and ECC recovery.

## [0.5.7] - 2026-04-09

### Changed
- Reed-Solomon ECC is now on by default. Every archive can detect and repair its own corruption out of the box. Use `--ecc none` to disable.
- `--ecc` flag accepts `none|low|medium|high` (default: `low`). Replaces the old opt-in `--ecc low|medium|high`.
- `--append` and `--incremental` now respect the ECC default (previously silently created archives without ECC).

### Added
- `EccLevel::is_none()` for unified ECC disable detection (`none`, `off`, `false`).
- README: self-healing demo section showing corruption, detection, repair, and recovery.
- Integration tests for `--ecc none` and default ECC behavior.

## [0.5.2] - 2026-04-09

### Fixed
- Progress bar compression ratio no longer stuck at 0.0x (scan stats were never populated).
- Progress bar ETA no longer oscillates between seconds and days. Uses stable linear extrapolation instead of indicatif's exponential moving average.
- Progress bar now updates continuously during compression instead of in bursts between batches.

### Added
- Elapsed time display in the progress bar.

## [0.5.1] - 2026-04-09

### Fixed
- Archive creation no longer buffers the entire dataset in memory. Files are now processed in parallel batches with bounded memory usage, making it practical to archive datasets larger than available RAM.
- Progress bar no longer stuck at 0B/s during compression. Progress updates flow as files are processed, not just during the final write phase.
- Release `.tg` archive now contains a single platform binary instead of bundling all platform archives (was 5x larger than equivalent tar.gz).

### Changed
- `tdg update` now downloads `.tg` archives (dogfooding the format), with fallback to `.tar.gz` for older releases.
- Release workflow creates per-platform `.tg` archives alongside `.tar.gz`/`.zip`.

## [0.5.0] - 2026-04-08

### Added
- `tdg diff --from N --to M` command: compare two temporal generations, showing added, removed, and modified files with sizes. Compares content by BLAKE3 block hashes (no block data read needed).
- Claude Code skill updated with `tdg diff` reference

## [0.4.1] - 2026-04-08

### Changed
- README rewritten: removed AI slop, grouped features by category, tightened copy, removed stale What's Next section (issues tracked in GitHub), added Release badge

## [0.4.0] - 2026-04-08

### Added
- `tdg update` command: self-update via GitHub releases with BLAKE3 checksum verification and atomic binary replacement (via `self_replace`)
- `tdg update --check`: check for updates without installing (always exits 0)
- One-line install script (`install.sh`): `curl -fsSL .../install.sh | sh` with OS/arch detection, checksum verification, and PATH setup
- SHA256SUMS and B3SUMS checksums generated in the release workflow
- Dogfood step in release CI: tardigrade archives its own release artifacts as `tardigrade-dist.tg`
- `cargo-binstall` metadata in Cargo.toml for binary installs via `cargo binstall tardigrade`
- Claude Code skill updated with `tdg update` reference, install workflow, and `--decrypt` flag

### Changed
- `--encrypt` flag on `tdg extract` renamed to `--decrypt` (old name kept as alias for backward compatibility)

## [0.3.0] - 2026-04-08

### Added
- ECC pipeline wired end-to-end: `--ecc low|medium|high` now produces real parity blocks during archive creation
- ECC-aware extraction: corrupted blocks are automatically reconstructed from parity data during `tdg extract`
- `tdg repair` command: scan for corruption, reconstruct using Reed-Solomon parity, write back in place
- `tdg verify` reports ECC group count, parity blocks, and whether corrupted blocks are recoverable
- `tdg info` shows ECC details (RS parameters, group count, parity block count) for erasure-coded archives
- Claude Code skill (`tardigrade-skill/SKILL.md`): teaches Claude the full tdg CLI, format, and common workflows
- 4 new integration tests: full ECC create/extract/verify flow, corruption repair, medium/high levels, repair on non-ECC archives

### Changed
- `CreateOptions` now accepts `ecc_level` field
- `BlockHeader` gains `new_parity()` constructor and `is_parity()` method
- `VerifyReport` includes ECC recoverability information
- Removed `#[allow(dead_code)]` from erasure module (all functions now used)

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
