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

`11x faster` than tar+zstd on source code | `78% smaller` with dedup | `2 GB/s` throughput

[![CI](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml/badge.svg)](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml)
[![Release](https://github.com/gnathoi/tardigrade/actions/workflows/release.yml/badge.svg)](https://github.com/gnathoi/tardigrade/actions/workflows/release.yml)

---

## Why

tar is 45 years old. No checksums, no dedup, no seekability, single-threaded compression, and a mess of incompatible extensions. tardigrade is fast, safe and efficient using the modern tools at our disposal. 

## Features

**Speed**
- **11x faster** — parallel zstd/lz4 via rayon, uses all cores
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
| Source project (5 MB, 270 files) | 8ms | 91ms | **11.4x** | 34ms | 89ms | **2.6x** | ~equal |
| Heavy dedup (13 MB, shared deps) | 9ms | 89ms | **9.9x** | 28ms | 93ms | **3.3x** | **78% smaller** |
| Large mixed (94 MB, logs+bins) | 31ms | 35ms | **1.1x** | 39ms | 72ms | **1.8x** | **29% smaller** |

tardigrade wins big on source code, projects with shared dependencies, anything with duplicate content. Parallel compression + dedup + skipping FastCDC for small files = 10x faster for typical developer workloads.

Large unique binary data is roughly equal. Both tools are I/O bound.

### Core Scaling

![Core Scaling](bench/bench-scaling.svg)

10 cores: ~2 GB/s. 32 cores (projected): ~2.2 GB/s. The serial fraction (~34%) is the dedup lookup + sequential write pass.

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
