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

**Modern archiving for modern systems.** tar, but for 2026.

Fast, multithreaded, content-addressed, encrypted, and beautiful.

[![CI](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml/badge.svg)](https://github.com/gnathoi/tardigrade/actions/workflows/ci.yml)

## Why

tar is 45 years old. It has no checksums, no dedup, no seekability, single-threaded compression, and a mess of incompatible extensions. tardigrade is what you'd build if you started from scratch with modern hardware, modern algorithms, and modern expectations.

## Features

- **Parallel compression** — zstd and lz4 via rayon, saturates all cores
- **Content-addressed dedup** — identical blocks stored once. Archive 3 copies of `node_modules` and pay for 1
- **BLAKE3 checksums** — every block verified on read. Corruption detected immediately
- **Encrypted archives** — ChaCha20-Poly1305 AEAD with passphrase key wrapping
- **Beautiful CLI** — progress bars, throughput, compression ratios, dedup savings
- **.gitignore-aware** — automatically skips `target/`, `node_modules/`, `.git/`
- **Content-defined chunking** — FastCDC for dedup that works across modified files, not just identical ones
- **Integrity verification** — `tdg verify` checks every block with detailed damage mapping
- **Cross-platform** — Linux, macOS, Windows

## Install

```bash
cargo install tardigrade
```

Or download a pre-built binary from [Releases](https://github.com/gnathoi/tardigrade/releases).

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
tdg extract --encrypt secret.tg -o ./decrypted

# Use lz4 for maximum speed
tdg create --compress lz4 fast.tg ./data

# High compression
tdg create --level 19 small.tg ./data

# Disable .gitignore filtering
tdg create --no-ignore everything.tg ./repo
```

## Example Output

```
$ tdg create backup.tg ./my-project

✓ Created backup.tg (4.29 MiB <- 19.31 MiB, 4.5x ratio, 0.02s, 827 MB/s)
  63 files, 2 dirs, 64 blocks (31 unique)
  ↗ 1.52 MiB saved by dedup
  Compression: 4.5x  Codec: zstd
```

```
$ tdg verify backup.tg

✓ Verify: backup.tg
  Header: OK  Footer: OK  Index: OK
  Blocks: 31/31 OK, 0 corrupted (0.0s)

  ✓ Archive integrity verified
```

## Benchmarks

Run locally with `bash bench/run-all.sh` before shipping. Results below from Apple Silicon (M-series, 10 cores, 3 runs averaged).

### Speed

![Benchmark Speed](bench/bench-speed.svg)

### Size & Compression

![Benchmark Size](bench/bench-size.svg)

### Key Results

| Dataset | tdg create | tar+zstd | Create | tdg extract | tar extract | Extract | tdg size | tar size | Size |
|---------|-----------|----------|--------|-------------|-------------|---------|----------|----------|------|
| Source project (5 MB, 270 files) | 150ms | 108ms | 0.7x | 49ms | 107ms | **2.2x** | 2.54 MB | 2.52 MB | ~equal |
| Heavy dedup (13 MB, shared deps) | 22ms | 103ms | **4.7x** | 44ms | 107ms | **2.4x** | 2.70 MB | 12.47 MB | **78% smaller** |
| Large mixed (102 MB, logs+bins) | 50ms | 52ms | ~equal | 88ms | 98ms | 1.1x | 10.04 MB | 14.41 MB | **30% smaller** |

**Where tardigrade wins big:** Any workload with duplicate content. Monorepos, node_modules, docker layers, backup directories. The content-addressed dedup produces dramatically smaller archives and faster operations because less data is written.

**Where it's equal:** Large unique data. Both tools hit I/O limits at the same point.

**Where tar wins:** Archive creation of small unique datasets (<10MB). tar's streaming model has less overhead per file.

### Core Scaling

tardigrade uses rayon for parallel compression and hashing. More cores = more throughput, up to the I/O and serial bottleneck. Measured on Apple Silicon, extrapolated using Amdahl's law:

![Core Scaling](bench/bench-scaling.svg)

At 10 cores: ~2 GB/s. Predicted at 32 cores: ~2.2 GB/s. The serial fraction (~34%) is the single-threaded dedup lookup + sequential write pass.

## Archive Format (.tg)

The `.tg` format is designed from scratch for modern use:

```
[ArchiveHeader 16B] [KeyEncap?] [Block0] [Block1] ... [BlockN] [Index] [RedundantIndex] [Footer 76B]
```

- **ArchiveHeader**: magic `TRDG`, version, flags (encrypted, erasure-coded, append-only)
- **Blocks**: 48-byte header (BLAKE3 hash, sizes, codec, CRC32) + compressed payload
- **Index**: msgpack-encoded file tree, zstd compressed, stored twice for redundancy
- **Footer**: index offsets, block count, Merkle root hash, prev-footer pointer

Content-defined chunking (FastCDC, 64KB-1MB target 256KB) splits files at content boundaries. Blocks are content-addressed by BLAKE3 hash. Identical blocks across files are stored once.

### Encryption

When `--encrypt` is used:
- Archive key: random 256-bit symmetric key
- Block encryption: ChaCha20-Poly1305 AEAD, nonce derived from content hash
- Key wrapping: passphrase -> BLAKE3 KDF -> wrapping key -> encrypted archive key
- Dedup automatically disabled (prevents hash-based content inference)

## What's Next

The wire format supports these features (flag bits reserved, fields in place). Implementation is in progress:

- [ ] Reed-Solomon erasure coding (`--ecc low|medium|high`)
- [ ] Temporal/append-only archives (`tdg log`, `tdg mount archive.tg@snapshot`)
- [ ] Incremental archives (`--incremental base.tg`)
- [ ] Archive merging (`tdg merge a.tg b.tg`)
- [ ] FUSE mounting (`tdg mount archive.tg /mnt`)
- [ ] Volume splitting (`tdg split --size 4G`)
- [ ] tar.zst read compatibility

## Architecture

```
CLI (clap)
  |
  +-- archive.rs    walk -> chunk (FastCDC) -> dedup -> compress -> write
  +-- extract.rs    read footer -> parse index -> decompress -> verify -> write
  +-- verify.rs     full integrity check with damage mapping
  |
  +-- chunk.rs      FastCDC content-defined chunking
  +-- dedup.rs      content-addressed block store
  +-- compress.rs   zstd / lz4 / none
  +-- encrypt.rs    ChaCha20-Poly1305 + key encapsulation
  +-- format.rs     wire format types (the foundation)
  +-- hash.rs       BLAKE3 + Merkle tree
  +-- index.rs      msgpack + zstd index serialization
  +-- metadata.rs   POSIX metadata + path traversal protection
  +-- progress.rs   indicatif progress bars
```

## License

Apache-2.0
