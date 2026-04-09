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

`self-repairing` archives | `62% smaller` with dedup | checksummed, encrypted, portable

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

**Dedup & compression**
- **Content-addressed dedup** — identical blocks stored once. 3 copies of `node_modules`, pay for 1
- **Content-defined chunking** — FastCDC splits at content boundaries, dedup works across modified files
- **.gitignore-aware** — skips `target/`, `node_modules/`, `.git/` automatically
- **Parallel compression** — zstd/lz4 via rayon, uses all cores

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

AMD Threadripper PRO 5975WX (32 cores / 64 logical). Run locally: `bash bench/run-all.sh`

### Speed

![Benchmark Speed](bench/bench-speed.svg)

### Size & Compression

![Benchmark Size](bench/bench-size.svg)

### Key Results

v0.5.7 with ECC on by default. Best of 5 runs (best of 3 for 10 GB datasets):

| Dataset | tdg create | tar+zstd | tdg extract | tar+zstd | tdg size | tar+zstd size |
|---------|-----------|----------|-------------|----------|----------|---------------|
| Source project (5 MB, 270 files) | 22ms | 30ms | 22ms | 15ms | 3.6 MB | 2.5 MB |
| Heavy dedup (13 MB, shared deps) | 20ms | 26ms | 18ms | 21ms | **3.3 MB** | 10.9 MB |
| Large mixed (94 MB, logs+bins) | 129ms | 63ms | 71ms | 93ms | 14.7 MB | 15.0 MB |
| 10 GB mixed (10 GB, 1000 files) | 14.3s | 15.5s | 10.8s | 8.7s | 9.4 GB | 8.8 GB |
| 10 GB dedup (10 GB, backup snapshots) | 7.7s | 7.2s | **5.4s** | 8.0s | **3.8 GB** | 10.0 GB |

ECC adds ~20% size overhead, so tardigrade archives are larger on data with no duplicates. The tradeoff: your archive can repair itself. On duplicate-heavy data, dedup more than compensates — 10 GB of backup snapshots compresses to **3.8 GB vs 10 GB** (62% smaller), and extract is 1.5x faster.

### Core Scaling (9.7 GB dataset, 64 logical cores)

![Core Scaling](bench/bench-scaling.svg)

Peak throughput: **731 MB/s at 32 threads**. ECC computation is the bottleneck — the serial fraction is 36.1% (Reed-Solomon encoding + dedup lookup + sequential write). Performance plateaus around 28-36 threads.

## When tardigrade wins

tardigrade isn't always the fastest or smallest. Here's where it genuinely helps, and where it doesn't.

**Data you can't afford to lose**
Reed-Solomon ECC means your archive can detect and repair bit rot, flash degradation, and transmission errors. tar and zip have zero protection. One flipped bit and your data is gone. tardigrade archives heal themselves. No other single-file archiver does this by default.

**Duplicate-heavy data**
Monorepos, node_modules, CI artifacts, backup snapshots, any dataset with repeated files or shared content. tar+zstd compresses each file independently and can't deduplicate across files. tardigrade's content-addressed blocks store identical content once. 10 GB of backup snapshots: **3.8 GB vs 10 GB** (62% smaller). Extract is 1.5x faster.

**Temporal backups**
`tdg create --append` adds a new point-in-time snapshot to an existing archive. Shared blocks across snapshots are stored once. A week of daily snapshots costs barely more than a single full backup. borg and restic do this too, but they need a repository and daemon. tardigrade gives you a single portable file.

**Comparable speed, more features**
tardigrade is on par with tar+zstd for speed — sometimes a little faster, sometimes a little slower — but you also get dedup, self-healing ECC, checksums, and encryption. The ECC adds ~20% size overhead on unique data, so tar+zstd produces smaller archives when there's nothing to deduplicate.

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
