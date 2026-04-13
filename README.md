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
- **ChaCha20-Poly1305 AEAD** with Argon2id key derivation (memory-hard, GPU-resistant)
- ECC works with encryption: self-healing even for encrypted archives (parity over ciphertext)
- Dedup disabled for encrypted archives (prevents hash-based content inference)

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

## Shell completions

`tdg completions <shell>` prints a completion script to stdout. Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

**Bash**
```bash
# One-shot (reload after editing)
echo 'source <(tdg completions bash)' >> ~/.bashrc

# Or install system-wide
tdg completions bash | sudo tee /etc/bash_completion.d/tdg >/dev/null
```

**Zsh**
```bash
# Put completions where zsh looks for them
mkdir -p ~/.zfunc
tdg completions zsh > ~/.zfunc/_tdg

# Make sure your ~/.zshrc has:
#   fpath=(~/.zfunc $fpath)
#   autoload -Uz compinit && compinit
```

**Fish**
```bash
tdg completions fish > ~/.config/fish/completions/tdg.fish
```

**PowerShell**
```powershell
# Add to your $PROFILE
tdg completions powershell | Out-String | Invoke-Expression
```

**Elvish**
```bash
tdg completions elvish > ~/.config/elvish/lib/tdg-completion.elv
# Then in rc.elv:  use tdg-completion
```

After installing, start a new shell and tab-complete subcommands, flags, and file paths:

```
$ tdg <TAB>
cat      completions  convert  create   diff     extract  info     join     list     log
merge    repair       split    update   verify

$ tdg create --<TAB>
--append              --encrypt-allow-dedup  --incremental        --no-ignore
--compress            --ecc                  --level              --quiet
--encrypt             --help                 --threads            --verbose
```

## Usage

```bash
# Create an archive
tdg create backup.tg ./my-project

# Extract
tdg extract backup.tg -o ./restored

# Print a single file without extracting
tdg cat backup.tg path/to/file.txt
tdg cat backup.tg config.yaml | head -20     # pipe-friendly

# List contents
tdg list backup.tg
tdg list -l backup.tg    # detailed view

# Archive info
tdg info backup.tg

# Verify integrity
tdg verify backup.tg

# Encrypted archive (ECC still works, dedup off for privacy)
tdg create --encrypt secret.tg ./private-data
tdg extract --decrypt secret.tg -o ./decrypted

# Encrypted with dedup (user accepts content-equality leakage)
tdg create --encrypt --encrypt-allow-dedup secret.tg ./private-data

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

## Command reference

Everything `tdg` can do, with every flag documented. Run `tdg <command> --help` for the same information from the CLI.

### Global flags

Available on every subcommand:

| Flag | Short | Description |
|------|-------|-------------|
| `--threads <N>` | `-j` | Number of worker threads. Defaults to all logical cores. Affects compression, decompression, ECC encoding, and parallel file walk. |
| `--quiet` | `-q` | Suppress all output including progress bars and summaries. Errors still go to stderr. Exit code conveys success/failure. |
| `--verbose` | `-v` | Verbose output (reserved for future use; currently no-op for most commands). |
| `--help` | `-h` | Print help for a command. |
| `--version` | `-V` | Print the `tdg` version. |

---

### `tdg create` — create an archive

Alias: `tdg c`

```
tdg create [OPTIONS] <ARCHIVE> <PATHS>...
```

**Arguments**
- `<ARCHIVE>` — path to the `.tg` file to write
- `<PATHS>...` — one or more files or directories to archive

**Flags**

| Flag | Default | Description |
|------|---------|-------------|
| `--compress <ALGO>` | `zstd` | Compression codec: `zstd`, `lz4`, `none`. |
| `--level <N>` / `-l <N>` | `9` | zstd level 1–19. Higher = smaller + slower. `--compress lz4` ignores this. |
| `--no-ignore` |  | Ignore `.gitignore`/`.ignore` files and archive everything (default: respect them — skips `target/`, `node_modules/`, `.git/`, etc.). |
| `--encrypt` / `-e` |  | Prompts for a passphrase (no echo) and encrypts every block with ChaCha20-Poly1305. Dedup is disabled unless `--encrypt-allow-dedup`. |
| `--encrypt-allow-dedup` |  | Re-enable dedup under encryption. This leaks content equality (an attacker can tell whether two blocks held the same plaintext) — explicitly off by default. |
| `--append` |  | Append a new generation to an existing archive (temporal mode). Shared blocks across generations are stored once. |
| `--incremental <BASE>` |  | Store only blocks not present in `<BASE>`. Extracting the result requires the base archive. |
| `--ecc <LEVEL>` | `low` | Reed-Solomon erasure coding: `none`, `low` (RS 10,2 ≈ 20% overhead), `medium` (RS 10,4 ≈ 40%), `high` (RS 10,6 ≈ 60%). Self-healing vs. size tradeoff. |

**Examples**
```bash
tdg create backup.tg ./project                    # defaults: zstd -9, low ECC, respect .gitignore
tdg create --compress lz4 fast.tg ./data          # prioritize speed
tdg create --level 19 tiny.tg ./data              # maximum compression
tdg create --encrypt secret.tg ./private          # prompts for passphrase
tdg create --ecc high --level 19 archive.tg ./data
tdg create --append snapshots.tg ./project        # new generation in existing archive
tdg create --incremental base.tg diff.tg ./project
```

**Notes**
- Combining `--encrypt` with `--incremental` or `--append` is not supported.
- `--no-ignore` applies to both `.gitignore` and `.ignore` files; hidden files are always included.
- Symlinks and hardlinks are preserved as-is; their targets are not followed.

---

### `tdg extract` — extract an archive

Alias: `tdg x`

```
tdg extract [OPTIONS] <ARCHIVE>
```

**Arguments**
- `<ARCHIVE>` — the archive to extract. Can be a `.tg` file or a legacy `tar` / `tar.gz` / `tar.zst` (auto-detected by magic bytes).

**Flags**

| Flag | Default | Description |
|------|---------|-------------|
| `--output <DIR>` / `-o <DIR>` | current dir | Destination directory. Created if missing. |
| `--decrypt` / `-d` |  | Decrypt the archive (prompts for passphrase). Alias: `--encrypt`. |
| `--base <BASE>` |  | Base archive for incremental extraction. Required for archives built with `--incremental`. |
| `--generation <N>` |  | Extract a specific generation from a temporal archive (0-indexed; see `tdg log`). |

**Examples**
```bash
tdg extract backup.tg -o ./restored
tdg extract --decrypt secret.tg -o ./out
tdg extract --base base.tg diff.tg -o ./restored    # incremental
tdg extract --generation 2 snapshots.tg -o ./v2     # temporal
tdg extract legacy.tar.gz -o ./from-tar             # auto-detect, streams decompression
```

**Progress output**
Extract shows a live progress bar, spinner, elapsed time, and ETA in the same style as `create`. Suppressed with `--quiet`. For streaming tar formats, progress is measured against the on-disk file size; for `.tg` archives, it's measured against total uncompressed bytes from the index.

**Security**
- Refuses to extract entries with `..` components that would escape the output directory.
- Refuses symlinks whose resolved target escapes the output directory.

---

### `tdg list` — list archive contents

Alias: `tdg ls`

```
tdg list [OPTIONS] <ARCHIVE>
```

**Arguments**
- `<ARCHIVE>` — archive to inspect.

**Flags**

| Flag | Short | Description |
|------|-------|-------------|
| `--long` | `-l` | Detailed output: permissions, owner, size, mtime, path. Similar to `ls -l`. |

**Examples**
```bash
tdg list backup.tg
tdg list -l backup.tg
tdg ls backup.tg | grep '\.rs$'
```

---

### `tdg info` — archive statistics

```
tdg info <ARCHIVE>
```

Prints format version, flags (encrypted, erasure-coded, incremental), file/dir counts, block counts (total vs unique), dedup savings, ECC configuration, and total compressed size.

```bash
tdg info backup.tg
```

---

### `tdg cat` — print one file to stdout

```
tdg cat [OPTIONS] <ARCHIVE> <PATH>
```

**Arguments**
- `<ARCHIVE>` — archive to read from.
- `<PATH>` — path of the file inside the archive (forward or back slashes accepted; leading `/` ignored).

**Flags**

| Flag | Short | Description |
|------|-------|-------------|
| `--decrypt` | `-d` | Prompts for a passphrase. Alias: `--encrypt`. |

**Examples**
```bash
tdg cat backup.tg src/main.rs
tdg cat backup.tg config.yaml | head -20
tdg cat --decrypt secret.tg notes.md
```

Reads only the blocks needed for the requested file — no full archive decompression.

---

### `tdg verify` — integrity check

```
tdg verify <ARCHIVE>
```

Walks every block, verifies BLAKE3 hashes and CRC32 block checksums, checks both the primary and redundant index, and reports corruption with ECC recovery status.

```bash
tdg verify backup.tg
# header ok  footer ok  index ok
# blocks 4/4 ok, 0 corrupted
```

Exit code: `0` if clean, `1` if any block is corrupted beyond ECC recovery.

---

### `tdg repair` — reconstruct corrupted blocks

```
tdg repair <ARCHIVE>
```

Finds corrupted blocks (failed hash/CRC check), reconstructs them using Reed-Solomon parity, and writes them back into the archive in place. Requires the archive to have been created with `--ecc` (low/medium/high).

```bash
tdg repair photos.tg
# repaired 1 block (Reed-Solomon recovery)
```

Fails with a clear error if ECC is absent or damage exceeds the parity budget.

---

### `tdg log` — list temporal generations

```
tdg log <ARCHIVE>
```

For archives built with `tdg create --append`, prints every generation's index: generation number, creation time, file count, total size, and dedup savings vs. prior generations.

```bash
tdg log snapshots.tg
# @0  2026-04-01  243 files   12.3 MB
# @1  2026-04-08  245 files   +81 KB (delta)
# @2  2026-04-13  251 files   +156 KB (delta)
```

Feeds directly into `tdg extract --generation N` and `tdg diff --from A --to B`.

---

### `tdg diff` — diff two generations

```
tdg diff --from <N> --to <M> <ARCHIVE>
```

**Flags**

| Flag | Description |
|------|-------------|
| `--from <N>` | Generation number to diff from. |
| `--to <M>` | Generation number to diff to. |

Prints added, removed, and modified paths between two generations of a temporal archive.

```bash
tdg diff --from 0 --to 2 snapshots.tg
```

---

### `tdg merge` — merge two archives

```
tdg merge [OPTIONS] <A> <B>
```

**Arguments**
- `<A>` — first archive.
- `<B>` — second archive.

**Flags**

| Flag | Short | Description |
|------|-------|-------------|
| `--output <ARCHIVE>` | `-o` | Output archive path. Required. |

Combines two archives into one with cross-archive dedup — identical blocks from either side are stored once.

```bash
tdg merge a.tg b.tg -o merged.tg
```

If both archives contain the same path, the entry from `<B>` wins.

---

### `tdg split` — split into volumes

```
tdg split --size <SIZE> <ARCHIVE>
```

**Arguments**
- `<ARCHIVE>` — archive to split.

**Flags**

| Flag | Description |
|------|-------------|
| `--size <SIZE>` | Max volume size. Accepts `K`, `M`, `G` suffixes (e.g. `500M`, `4G`). Required. |

Writes sibling files `<ARCHIVE>.001`, `<ARCHIVE>.002`, ... each at most `<SIZE>` bytes.

```bash
tdg split backup.tg --size 4G
# wrote backup.tg.001 (4.0 GB), backup.tg.002 (4.0 GB), backup.tg.003 (1.2 GB)
```

Useful for FAT32 limits, chunked uploads, or multi-disc transport.

---

### `tdg join` — reassemble split volumes

```
tdg join [OPTIONS] <VOLUMES>...
```

**Arguments**
- `<VOLUMES>...` — volume files in order (e.g. `backup.tg.001 backup.tg.002 ...`).

**Flags**

| Flag | Short | Description |
|------|-------|-------------|
| `--output <ARCHIVE>` | `-o` | Output archive path. Required. |

```bash
tdg join backup.tg.001 backup.tg.002 backup.tg.003 -o backup.tg
```

Validates that volumes concatenate into a coherent archive (header, footer, index all check out).

---

### `tdg convert` — migrate tar to .tg

```
tdg convert [OPTIONS] <INPUT> <OUTPUT>
```

**Arguments**
- `<INPUT>` — source `tar`, `tar.gz`, or `tar.zst`.
- `<OUTPUT>` — destination `.tg` file.

**Flags**

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--compress <ALGO>` |  | `zstd` | Codec for the new archive. |
| `--level <N>` | `-l` | `9` | zstd level 1–19. |

Extracts the tar to a temp directory then re-archives as `.tg` with dedup + ECC applied. Slower than `tdg create` on the original source, but it's the supported migration path when you only have the tar.

```bash
tdg convert legacy.tar.zst output.tg
tdg convert --level 19 legacy.tar.gz tight.tg
```

To simply *read* a tar archive without converting, use `tdg extract` — it auto-detects the format.

---

### `tdg update` — self-update

```
tdg update [OPTIONS]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--check` | Only check for updates; don't install. Exit code `0` if up to date, non-zero if a newer release exists. |

Downloads the latest release binary from GitHub, verifies the SHA256 checksum, and atomically replaces the current `tdg` executable.

```bash
tdg update
tdg update --check
```

---

### `tdg completions` — generate shell completion script

```
tdg completions <SHELL>
```

**Arguments**
- `<SHELL>` — one of: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

Prints the completion script to stdout. See [Shell completions](#shell-completions) above for per-shell install instructions.

```bash
tdg completions zsh > ~/.zfunc/_tdg
```

---

## Example Output

```
$ tdg create backup.tg ./my-project

  created backup.tg

  195.37 KiB -> 891 B  224.5x  zstd
  5 files, 2 dirs  5 blocks (4 unique)
  97.66 KiB saved by dedup (1 duplicate blocks eliminated)
  ecc: RS(10,2) 2 parity blocks ~20% overhead
  0.02s  12 MB/s
```

```
$ tdg verify backup.tg

  verified backup.tg

  header ok  footer ok  index ok
  blocks 4/4 ok, 0 corrupted
  0.01s
  ecc: 1 groups, 2 parity blocks
```

```
$ tdg extract backup.tg -o ./restored

  extracted backup.tg -> ./restored

  195.37 KiB  5 files, 2 dirs
  0.01s
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
- Key wrapping: passphrase -> Argon2id (64 MB, 3 iterations) -> wrapping key -> encrypted archive key
- ECC works with encryption: parity is computed over ciphertext (encrypt-then-ECC)
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
