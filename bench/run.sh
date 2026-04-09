#!/bin/bash
# Benchmark tardigrade vs tar+zstd
# Outputs CSV to stdout, human-readable to stderr
# Timing: measures only the process, not shell overhead
set -e

TDG="${TDG:-tdg}"
WORKDIR=$(mktemp -d)
trap "rm -rf $WORKDIR" EXIT

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DATADIR="$SCRIPT_DIR/.data"

# Time a command precisely using date +%s%N (nanoseconds) or gdate
# Falls back to python only if needed
if date +%s%N >/dev/null 2>&1 && [ "$(date +%s%N)" != "%s%N" ]; then
    now_ms() { echo $(( $(date +%s%N) / 1000000 )); }
elif command -v gdate >/dev/null 2>&1; then
    now_ms() { echo $(( $(gdate +%s%N) / 1000000 )); }
elif command -v python3 >/dev/null 2>&1; then
    now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }
else
    >&2 echo "ERROR: need date with nanoseconds, gdate, or python3 for timing"
    exit 1
fi

# Time a single command, output ms. Runs the command directly (no subshell).
time_cmd() {
    local start=$(now_ms)
    "$@" >/dev/null 2>&1
    local end=$(now_ms)
    echo $((end - start))
}

create_datasets() {
    >&2 echo "Creating benchmark datasets..."

    # Dataset 1: Source code project (~5 MB)
    mkdir -p "$WORKDIR/source_project/src" "$WORKDIR/source_project/tests" "$WORKDIR/source_project/docs"
    for i in $(seq 1 200); do
        python3 -c "
import random, string
lines = random.randint(50, 500)
print('\n'.join(''.join(random.choices(string.ascii_lowercase + '    \n{}();', k=random.randint(20, 120))) for _ in range(lines)))
" > "$WORKDIR/source_project/src/module_$i.rs"
    done
    for i in $(seq 1 50); do
        python3 -c "
import random
lines = random.randint(20, 200)
print('\n'.join('fn test_' + str(j) + '() { assert!(true); }' for j in range(lines)))
" > "$WORKDIR/source_project/tests/test_$i.rs"
    done
    for i in $(seq 1 20); do
        yes "Documentation content for module $i with various details." | head -500 > "$WORKDIR/source_project/docs/doc_$i.md"
    done

    # Dataset 2: Heavy duplication (monorepo / node_modules sim, ~13 MB)
    mkdir -p "$WORKDIR/dedup_heavy/base"
    for i in $(seq 1 40); do
        dd if=/dev/urandom bs=1024 count=50 of="$WORKDIR/dedup_heavy/base/lib_$i.bin" 2>/dev/null
    done
    for pkg in $(seq 1 5); do
        cp -r "$WORKDIR/dedup_heavy/base" "$WORKDIR/dedup_heavy/pkg_$pkg"
        for u in $(seq 1 3); do
            dd if=/dev/urandom bs=1024 count=50 of="$WORKDIR/dedup_heavy/pkg_$pkg/unique_$u.bin" 2>/dev/null
        done
    done

    # Dataset 3: Large mixed (logs + binaries + copies, ~100 MB)
    mkdir -p "$WORKDIR/large_mixed"
    for i in $(seq 1 5); do
        yes "log entry $(date) level=INFO msg=\"request $i\" duration=42ms" | head -200000 > "$WORKDIR/large_mixed/log_$i.txt"
    done
    for i in $(seq 1 10); do
        dd if=/dev/urandom bs=1024 count=1024 of="$WORKDIR/large_mixed/data_$i.bin" 2>/dev/null
    done
    for i in $(seq 1 5); do
        cp "$WORKDIR/large_mixed/data_$i.bin" "$WORKDIR/large_mixed/backup_$i.bin"
    done

    for ds in source_project dedup_heavy large_mixed; do
        local size_kb=$(du -sk "$WORKDIR/$ds" | awk '{print $1}')
        local files=$(find "$WORKDIR/$ds" -type f | wc -l | tr -d ' ')
        >&2 echo "  $ds: ${files} files, ${size_kb} KB"
    done

    # Dataset 4: 10 GB scaling dataset (reuse cached from scaling.sh if available)
    if [ -f "$DATADIR/.generated" ]; then
        local size_kb=$(du -sk "$DATADIR" | awk '{print $1}')
        local files=$(find "$DATADIR" -type f -not -name '.generated' | wc -l | tr -d ' ')
        >&2 echo "  10gb_mixed: ${files} files, ${size_kb} KB (cached)"
    else
        >&2 echo "  10gb_mixed: not available (run scaling.sh first to generate)"
    fi

    # Dataset 5: 10 GB heavy dedup (simulates backup snapshots / container layers)
    DEDUP10G_DIR="$SCRIPT_DIR/.data-dedup10g"
    DEDUP10G_STAMP="$DEDUP10G_DIR/.generated"
    if [ -f "$DEDUP10G_STAMP" ]; then
        local size_kb=$(du -sk "$DEDUP10G_DIR" | awk '{print $1}')
        local files=$(find "$DEDUP10G_DIR" -type f -not -name '.generated' | wc -l | tr -d ' ')
        >&2 echo "  dedup_10gb: ${files} files, ${size_kb} KB (cached)"
    else
        >&2 echo "  Generating ~10 GB heavy-dedup dataset (backup snapshots)..."
        rm -rf "$DEDUP10G_DIR"
        mkdir -p "$DEDUP10G_DIR/base"

        # 2 GB of unique base data — 200 × 10 MB files
        for i in $(seq 1 200); do
            dd if=/dev/urandom bs=1M count=10 of="$DEDUP10G_DIR/base/file_$i.bin" 2>/dev/null
            if [ $((i % 50)) -eq 0 ]; then
                >&2 echo "    $i/200 base files"
            fi
        done

        # 4 snapshots that copy the base and tweak ~10% of files each
        for snap in $(seq 1 4); do
            snap_dir="$DEDUP10G_DIR/snapshot_$snap"
            cp -r "$DEDUP10G_DIR/base" "$snap_dir"
            # Overwrite ~20 random files with new data per snapshot
            for i in $(seq 1 20); do
                target=$((RANDOM % 200 + 1))
                dd if=/dev/urandom bs=1M count=10 of="$snap_dir/file_$target.bin" 2>/dev/null
            done
            >&2 echo "    snapshot $snap created"
        done

        touch "$DEDUP10G_STAMP"
        local size_kb=$(du -sk "$DEDUP10G_DIR" | awk '{print $1}')
        local files=$(find "$DEDUP10G_DIR" -type f -not -name '.generated' | wc -l | tr -d ' ')
        >&2 echo "  dedup_10gb: ${files} files, ${size_kb} KB"
    fi
}

bench_one() {
    local dataset=$1 path=$2 runs=${3:-5}
    local input_kb=$(du -sk "$path" | awk '{print $1}')
    local input_mb
    input_mb=$(python3 -c "print(f'{$input_kb/1024:.2f}')")

    # Warm filesystem cache
    find "$path" -type f -exec cat {} + > /dev/null 2>&1

    # --- tardigrade create ---
    local tdg_create_best=999999
    for r in $(seq 1 $runs); do
        rm -f "$WORKDIR/bench.tg"
        local ms=$(time_cmd $TDG create "$WORKDIR/bench.tg" "$path" --quiet)
        [ $ms -lt $tdg_create_best ] && tdg_create_best=$ms
    done
    local tdg_size_kb=$(du -sk "$WORKDIR/bench.tg" | awk '{print $1}')
    local tdg_size_mb tdg_ratio
    tdg_size_mb=$(python3 -c "print(f'{$tdg_size_kb/1024:.2f}')")
    tdg_ratio=$(python3 -c "print(f'{$input_kb/max($tdg_size_kb,1):.2f}')")

    # --- tar+zstd create ---
    local tar_create_best=999999
    for r in $(seq 1 $runs); do
        rm -f "$WORKDIR/bench.tar.zst"
        local ms=$(time_cmd sh -c "tar cf - -C '$(dirname "$path")' '$(basename "$path")' | zstd -3 -q -o '$WORKDIR/bench.tar.zst'")
        [ $ms -lt $tar_create_best ] && tar_create_best=$ms
    done
    local tar_size_kb=$(du -sk "$WORKDIR/bench.tar.zst" | awk '{print $1}')
    local tar_size_mb tar_ratio
    tar_size_mb=$(python3 -c "print(f'{$tar_size_kb/1024:.2f}')")
    tar_ratio=$(python3 -c "print(f'{$input_kb/max($tar_size_kb,1):.2f}')")

    # --- tardigrade extract ---
    local tdg_extract_best=999999
    for r in $(seq 1 $runs); do
        rm -rf "$WORKDIR/extract-tdg"
        local ms=$(time_cmd $TDG extract "$WORKDIR/bench.tg" -o "$WORKDIR/extract-tdg" --quiet)
        [ $ms -lt $tdg_extract_best ] && tdg_extract_best=$ms
    done

    # --- tar+zstd extract ---
    local tar_extract_best=999999
    for r in $(seq 1 $runs); do
        rm -rf "$WORKDIR/extract-tar"
        mkdir -p "$WORKDIR/extract-tar"
        local ms=$(time_cmd sh -c "zstd -d -q '$WORKDIR/bench.tar.zst' -o '$WORKDIR/bench.tar' && tar xf '$WORKDIR/bench.tar' -C '$WORKDIR/extract-tar'")
        [ $ms -lt $tar_extract_best ] && tar_extract_best=$ms
        rm -f "$WORKDIR/bench.tar"
    done

    echo "tdg,$dataset,create,$tdg_create_best,$input_mb,$tdg_size_mb,$tdg_ratio"
    echo "tar+zstd,$dataset,create,$tar_create_best,$input_mb,$tar_size_mb,$tar_ratio"
    echo "tdg,$dataset,extract,$tdg_extract_best,$input_mb,,,"
    echo "tar+zstd,$dataset,extract,$tar_extract_best,$input_mb,,,"

    local create_speedup extract_speedup size_savings
    create_speedup=$(python3 -c "print(f'{max($tar_create_best,1)/max($tdg_create_best,1):.1f}')")
    extract_speedup=$(python3 -c "print(f'{max($tar_extract_best,1)/max($tdg_extract_best,1):.1f}')")
    size_savings=$(python3 -c "print(f'{(1-$tdg_size_kb/max($tar_size_kb,1))*100:.0f}')")
    >&2 echo "  $dataset (best of $runs):"
    >&2 echo "    create:  tdg ${tdg_create_best}ms vs tar+zstd ${tar_create_best}ms — ${create_speedup}x"
    >&2 echo "    extract: tdg ${tdg_extract_best}ms vs tar+zstd ${tar_extract_best}ms — ${extract_speedup}x"
    >&2 echo "    size:    tdg ${tdg_size_mb}MB vs tar+zstd ${tar_size_mb}MB — ${size_savings}% smaller"
}

create_datasets

>&2 echo ""
>&2 echo "Running benchmarks (best of 5 runs)..."
>&2 echo ""

echo "tool,dataset,operation,time_ms,input_mb,output_mb,ratio"
bench_one "source_project" "$WORKDIR/source_project"
bench_one "dedup_heavy" "$WORKDIR/dedup_heavy"
bench_one "large_mixed" "$WORKDIR/large_mixed"

# 10 GB dataset — best of 3 (too large for 5 runs)
if [ -f "$DATADIR/.generated" ]; then
    bench_one "10gb_mixed" "$DATADIR" 3
fi

# 10 GB heavy dedup — best of 3
DEDUP10G_DIR="$SCRIPT_DIR/.data-dedup10g"
if [ -f "$DEDUP10G_DIR/.generated" ]; then
    bench_one "dedup_10gb" "$DEDUP10G_DIR" 3
fi

>&2 echo ""
>&2 echo "Done."
