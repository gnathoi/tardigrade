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

Self-healing archive tool. Fast, checksummed, deduplicated.

`self-repairing` ECC by default | `1.9 GB/s` parallel throughput | `73% smaller` with dedup

[![CI](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml/badge.svg)](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml)
[![Release](https://github.com/gnathoi/tardigrade/actions/workflows/release.yml/badge.svg)](https://github.com/gnathoi/tardigrade/actions/workflows/release.yml)
[![crates.io](https://img.shields.io/crates/v/tardigrade.svg)](https://crates.io/crates/tardigrade)

---

## Why

tar is 45 years old. No checksums, no dedup, no seekability, single-threaded compression, and a mess of incompatible extensions. tardigrade is fast, safe and efficient using the modern tools at our disposal. 

## Self-healing archives

Every archive includes Reed-Solomon erasure coding by default. Corrupt the bytes, tardigrade fixes itself:

```bash
# Create an archive
tdg create photos.tg ./photos

# Corrupt 50 bytes with a hex editor, dd, or bit rot
dd if=/dev/urandom of=photos.tg bs=1 count=50 seek=200 conv=notrunc

# Detect the damage
tdg verify photos.tg
# blocks 1/42 corrupted, 1 recoverable via ECC

# Repair it
tdg repair photos.tg
# repaired 1 block (Reed-Solomon recovery)

# Extract — original files restored perfectly
tdg extract photos.tg -o ./restored
```

tar and zip have zero protection against bit rot. One flipped bit and your data is gone. tardigrade archives know how to heal themselves.

## Features

**Speed**
- **11x faster** — parallel zstd/lz4 via rayon, uses all cores
- **.gitignore-aware** — skips `target/`, `node_modules/`, `.git/` automatically

**Dedup & compression**
- **Content-addressed dedup** — identical blocks stored once. 3 copies of `node_modules`, pay for 1
- **Content-defined chunking** — FastCDC splits at content boundaries, dedup works across modified files

**Self-healing archives**
- **Reed-Solomon ECC on by default** — every archive can detect and repair its own corruption
- **BLAKE3 checksums** — every block verified on read
- **`tdg verify`** — full integrity check with damage mapping
- **`tdg repair`** — reconstruct corrupted blocks from ECC parity
- Three levels: `low` (default, ~20% overhead), `medium` (~40%), `high` (~60%). Disable with `--ecc none`

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

# Reed-Solomon erasure coding (on by default)
tdg create archive.tg ./data                  # ECC low is the default
tdg create --ecc medium archive.tg ./data     # RS(10,4) ~40% overhead
tdg create --ecc high archive.tg ./data       # RS(10,6) ~60% overhead
tdg create --ecc none archive.tg ./data        # disable ECC for smallest size

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
  ecc: RS(10,2) 13 parity blocks ~20% overhead
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

AMD Threadripper PRO 5975WX (64 cores). Run locally: `bash bench/run-all.sh`

### Speed

![Benchmark Speed](bench/bench-speed.svg)

### Size & Compression

![Benchmark Size](bench/bench-size.svg)

### Key Results

Best of 5 runs (best of 3 for 10 GB datasets), process time only:

| Dataset | tdg create | tar+zstd | Speedup | tdg extract | tar+zstd | Speedup | Size savings |
|---------|-----------|----------|---------|-------------|----------|---------|-------------|
| Source project (5 MB, 270 files) | 19ms | 29ms | **1.5x** | 21ms | 15ms | 0.7x | ~equal |
| Heavy dedup (13 MB, shared deps) | 18ms | 25ms | **1.4x** | 18ms | 22ms | **1.2x** | **75% smaller** |
| Large mixed (94 MB, logs+bins) | 95ms | 65ms | 0.7x | 69ms | 93ms | **1.3x** | **33% smaller** |
| 10 GB mixed (10 GB, 1000 files) | 5.9s | 15.3s | **2.6x** | 10.8s | 8.6s | 0.8x | **23% smaller** |
| 10 GB dedup (10 GB, backup snapshots) | 4.4s | 7.1s | **1.6x** | 5.4s | 8.0s | **1.5x** | **73% smaller** |

tardigrade's strength is dedup — backup snapshots, container layers, anything with duplicate content compresses to a fraction of what tar+zstd produces. At 10 GB with heavy dedup: **2.7 GB vs 10 GB**. Create speed scales well on large datasets (2.6x at 10 GB). On large unique binary data, both tools are I/O bound.

### Core Scaling (9.7 GB dataset)

![Core Scaling](bench/bench-scaling.svg)

Peak throughput: **1.9 GB/s at 28 threads**. Serial fraction: 22.5% (dedup lookup + sequential write pass). Performance plateaus around 28–36 threads, then declines slightly from memory bandwidth contention.

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
