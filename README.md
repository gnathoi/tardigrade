```
                           :+xXXXXXxXXX+
              :+Xxxxx+xxxXXxxx++xx++xXx+xxxx+
            xxxx++++++++++++xxX;;;++++xx+;++;+XX.
          Xxxxxxx++x+;;;;;;;+++xx;;;;;;++X::;;;xXXx
        +xxxx+++++++++;:;;;;;;;+++x:;;;;;;x;:;;;xxx+x
      +Xxxx+++++;++;;;;;.:;;;;;;;;;+::;;;;;+;;;:;xxx;++
     xX++++++;;;;;;;;;;;.:;;;;:;;;;;;.:;;;;;+;;;:+x+;;x
    :X++;;;;;:;;;;;;;;;;: :;;;;;;;;;;.::;;;;;+:::;;x::;X
    xx+;;;;..;;;;;:::;;;: .:;;;;;;;;;.:::;;;;;+:::;x::;x.
   .x+;;;: :;;;;::::;;::: .;;;;;;;;;; .:::;;;;+.::;+..:;+
    x;:;::.;;;::::::::::. :;;;;;::;;; .:::;;;;+:::+x .:x+
    ++x;;::::;:::::::::. .::;;:::::;. .:::;:;;+:.::+;.:;;
     ;: .:..:;::;:::::.  ::;;::::::: ...::::;;+:.:;++..:+
     x;++:;::;::;:::::...;::.::;::+......::::;;..:::+.:;+
    x;;:+;;:::;::;xx++++;;::;;;;;;;.:...::::::;...::+:::;
     ;::::.:;.;:..:;     :;;;;::::+;:;::::::;;;..::;  :
     ;;::::;   ::.        +;;:::::     .::::+    ..
      ..:.                 :;;::         :..
                            . ..
```

# tardigrade

Archive tool. Fast, checksummed, deduplicated.

`7x faster` than tar+zstd on source code | `78% smaller` with dedup | `1.5 GB/s` throughput

[![CI](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml/badge.svg)](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml)
[![Release](https://github.com/gnathoi/tardigrade/actions/workflows/release.yml/badge.svg)](https://github.com/gnathoi/tardigrade/actions/workflows/release.yml)
[![crates.io](https://img.shields.io/crates/v/tardigrade.svg)](https://crates.io/crates/tardigrade)

---

## Why

tar is 45 years old. No checksums, no dedup, no seekability, single-threaded compression, and a mess of incompatible extensions. tardigrade is fast, safe and efficient using the modern tools at our disposal. 

## Features

**Speed**
- **Up to 9x faster** — parallel zstd/lz4 via rayon, uses all cores
- **.gitignore-aware** — skips `target/`, `node_modules/`, `.git/` automatically

**Dedup & compression**
- **Content-addressed dedup** — identical blocks stored once. 3 copies of `node_modules`, pay for 1
- **Content-defined chunking** — FastCDC splits at content boundaries, dedup works across modified files

**Integrity & recovery**
- **BLAKE3 checksums** — every block verified on read
- **Reed-Solomon ECC** — `--ecc low|medium|high` erasure coding for data recovery
- **`tdg verify`** — full integrity check with damage mapping
- **`tdg repair`** — reconstruct corrupted blocks from ECC parity

**Encryption**
- **ChaCha20-Poly1305 AEAD** with passphrase key wrapping

**Archive operations**
- **Temporal archives** — `--append` for point-in-time snapshots, `tdg log` to browse
- **Incremental** — `--incremental base.tg` stores only changed blocks
- **Merge** — `tdg merge a.tg b.tg` with cross-archive dedup
- **Split/join** — `tdg split --size 4G` for transport limits
- **tar compatibility** — `tdg extract` reads tar/tar.gz/tar.zst, `tdg convert` migrates to .tg
- **Self-update** — `tdg update` with checksum verification

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/gnathoi/tardigrade/main/install.sh | sh
```

Or via Cargo:

```bash
cargo install tardigrade
```

Or grab a binary from [Releases](https://github.com/gnathoi/tardigrade/releases).

## Usage

```bash
# Create an archive
tdg create backup.tg ./my-project

# Extract
tdg extract backup.tg -o ./restored

# List contents
tdg list backup.tg
tdg list -l backup.tg    # detailed view

# Archive info
tdg info backup.tg

# Verify integrity
tdg verify backup.tg

# Encrypted archive
tdg create --encrypt secret.tg ./private-data
tdg extract --decrypt secret.tg -o ./decrypted

# Fast mode (lz4, lower compression, maximum speed)
tdg create --compress lz4 fast.tg ./data

# Maximum compression
tdg create --level 19 small.tg ./data

# Faster compression (default is 9)
tdg create --level 1 quick.tg ./data

# Disable .gitignore filtering
tdg create --no-ignore everything.tg ./repo

# Temporal archives (append new generations)
tdg create backup.tg ./project
tdg create --append backup.tg ./project       # append generation 1
tdg create --append backup.tg ./project       # append generation 2
tdg log backup.tg                             # list all generations
tdg extract --generation 0 backup.tg -o v0    # extract specific generation

# Incremental archives (only store new/changed blocks)
tdg create base.tg ./project
tdg create --incremental base.tg diff.tg ./project
tdg extract --base base.tg diff.tg -o ./restored

# Merge archives
tdg merge a.tg b.tg -o merged.tg

# Split and join volumes
tdg split archive.tg --size 4G
tdg join archive.001.tg archive.002.tg -o archive.tg

# Extract legacy tar archives (auto-detected)
tdg extract legacy.tar.zst -o ./restored
tdg extract legacy.tar.gz -o ./restored

# Convert tar to .tg (with dedup)
tdg convert legacy.tar.zst output.tg

# Reed-Solomon erasure coding
tdg create --ecc low archive.tg ./data        # RS(10,2) ~20% overhead
tdg create --ecc medium archive.tg ./data     # RS(10,4) ~40% overhead
tdg create --ecc high archive.tg ./data       # RS(10,6) ~60% overhead

# Self-update
tdg update                                    # update to latest release
tdg update --check                            # check without installing
```

## Example Output

```
$ tdg create backup.tg ./my-project

  created backup.tg

  21.71 MiB -> 7.66 MiB  2.8x  zstd
  125 files, 11 dirs  127 blocks (123 unique)
  1.95 MiB saved by dedup (4 duplicate blocks eliminated)
  0.03s  806 MB/s
```

```
$ tdg verify backup.tg

  verified backup.tg

  header ok  footer ok  index ok
  blocks 123/123 ok, 0 corrupted
  0.02s
```

```
$ tdg extract backup.tg -o ./restored

  extracted backup.tg -> ./restored

  21.71 MiB  125 files, 11 dirs
  0.02s
```

## Benchmarks

Apple Silicon (M-series, 10 cores). Run locally: `bash bench/run-all.sh`

### Speed

![Benchmark Speed](bench/bench-speed.svg)

### Size & Compression

![Benchmark Size](bench/bench-size.svg)

### Key Results

Best of 5 runs, process time only:

| Dataset | tdg create | tar+zstd | Speedup | tdg extract | tar+zstd | Speedup | Size savings |
|---------|-----------|----------|---------|-------------|----------|---------|-------------|
| Source project (5 MB, 270 files) | 14ms | 93ms | **6.6x** | 34ms | 90ms | **2.6x** | ~equal |
| Heavy dedup (13 MB, shared deps) | 10ms | 87ms | **8.7x** | 27ms | 89ms | **3.3x** | **78% smaller** |
| Large mixed (91 MB, logs+bins) | 41ms | 33ms | 0.8x | 39ms | 73ms | **1.9x** | **28% smaller** |

### Core Scaling

![Core Scaling](bench/bench-scaling.svg)

10 logical cores: ~1.5 GB/s. 32 logical cores (projected): ~1.8 GB/s. 64 logical cores (projected): ~1.9 GB/s. The serial fraction (~34%) is the dedup lookup + sequential write pass.

## When tardigrade wins

tardigrade isn't always the fastest or smallest. Here's where it genuinely blows other methods out of the water, and where it doesn't.

**Duplicate-heavy data (the sweet spot)**
Monorepos, node_modules, Docker layers, CI artifacts, any dataset with repeated files or shared content. tar+zstd compresses each file independently and can't deduplicate across files. tardigrade's content-addressed blocks mean identical content is stored once regardless of where it appears. On the benchmark dedup dataset: **78% smaller** and **9x faster** than tar+zstd.

**Source code and project backups**
Lots of small text files. tardigrade skips FastCDC overhead for small files and parallelizes compression across all cores. On the benchmark source project (270 files, 5 MB): **7x faster** to create, **2.6x faster** to extract.

**Long-term archival (data you can't lose)**
Reed-Solomon ECC means your archive can detect and repair bit rot, flash degradation, and transmission errors. tar and zip have zero protection. One flipped bit and your data is gone. tardigrade archives heal themselves. No other single-file archiver does this by default.

**Temporal backups with dedup**
`tdg create --append` adds a new point-in-time snapshot to an existing archive. Shared blocks across snapshots are stored once. A week of daily snapshots costs barely more than a single full backup. borg and restic do this too, but they need a repository and daemon. tardigrade gives you a single portable file.

**Where tar+zstd wins**
Large datasets with no duplicate content (random binary data, unique media files). Both tools are I/O bound and tar+zstd's simpler pipeline has less overhead. On the benchmark large mixed dataset (91 MB, mostly random binaries): tar+zstd creates slightly faster. tardigrade still wins on extraction and compression ratio due to dedup on the duplicate portions.

## Archive Format (.tg)

```
[ArchiveHeader 16B] [KeyEncap?] [Block0] [Block1] ... [BlockN] [Index] [RedundantIndex] [Footer 76B]
```

- **ArchiveHeader**: magic `TRDG`, version, flags (encrypted, erasure-coded, append-only)
- **Blocks**: 48-byte header (BLAKE3 hash, sizes, codec, CRC32) + compressed payload
- **Index**: msgpack-encoded file tree, zstd compressed, stored twice for redundancy
- **Footer**: index offsets, block count, Merkle root hash, prev-footer pointer

Files are split at content boundaries (FastCDC, 64KB-1MB, target 256KB). Blocks are content-addressed by BLAKE3 hash. Identical blocks stored once.

### Encryption

- Archive key: random 256-bit symmetric key
- Block encryption: ChaCha20-Poly1305 AEAD, nonce derived from content hash
- Key wrapping: passphrase -> BLAKE3 KDF -> wrapping key -> encrypted archive key
- Dedup disabled when encrypted (prevents hash-based content inference)

## Architecture

```
CLI (clap)
  |
  +-- archive.rs      walk -> chunk (FastCDC) -> dedup -> compress -> write
  +-- extract.rs      read footer -> parse index -> decompress -> verify -> write
  +-- verify.rs       full integrity check with damage mapping
  |
  +-- chunk.rs        FastCDC content-defined chunking
  +-- dedup.rs        content-addressed block store
  +-- compress.rs     zstd / lz4 / none
  +-- encrypt.rs      ChaCha20-Poly1305 + key encapsulation
  +-- erasure.rs      Reed-Solomon erasure coding (RS 10,2/4/6)
  +-- format.rs       wire format types (the foundation)
  +-- hash.rs         BLAKE3 + Merkle tree
  +-- index.rs        msgpack + zstd index serialization
  +-- metadata.rs     POSIX metadata + path traversal protection
  +-- progress.rs     indicatif progress bars
  |
  +-- temporal.rs     append-only archives + generation management
  +-- incremental.rs  differential archives against a base
  +-- merge.rs        content-addressed archive merging
  +-- split.rs        volume splitting + reassembly
  +-- compat.rs       tar/tar.gz/tar.zst read + conversion
  +-- update.rs       self-update via GitHub releases
```

## Claude Code Skill

tardigrade includes a [Claude Code](https://claude.ai/code) skill at `tardigrade-skill/SKILL.md`. Add it to your Claude Code settings and Claude will use `tdg` commands when archiving, backing up, or working with `.tg` files.

## License

Apache-2.0
