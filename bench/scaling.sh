#!/bin/bash
# Benchmark tardigrade scaling across thread counts.
# Generates ~10 GB of test data (cached in bench/.data/).
# Designed for high-core-count machines (Threadripper, EPYC, etc.)
set -e

TDG="${TDG:-tdg}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DATADIR="$SCRIPT_DIR/.data"

MAX_CORES=$(python3 -c "import os; print(os.cpu_count())")
echo "Detected $MAX_CORES logical cores" >&2

# ── Generate 10 GB dataset (cached) ──────────────────────────────────────────
STAMP="$DATADIR/.generated"
if [ -f "$STAMP" ]; then
    echo "Reusing cached dataset in $DATADIR" >&2
else
    echo "Generating ~10 GB benchmark dataset..." >&2
    rm -rf "$DATADIR"
    mkdir -p "$DATADIR"

    # 5 GB of random binary data — 500 × 10 MB files
    echo "  random binaries (5 GB)..." >&2
    for i in $(seq 1 500); do
        dd if=/dev/urandom bs=1M count=10 of="$DATADIR/rand_${i}.bin" 2>/dev/null
        if [ $((i % 50)) -eq 0 ]; then
            echo "    $i/500 binary files" >&2
        fi
    done

    # 3 GB of compressible text — 300 × ~10 MB files
    echo "  compressible text (3 GB)..." >&2
    for i in $(seq 1 300); do
        python3 -c "
import random, string
lines = []
for _ in range(100000):
    lines.append(''.join(random.choices(string.ascii_lowercase + '    \n{}();', k=random.randint(40, 160))))
print('\n'.join(lines))
" > "$DATADIR/text_${i}.txt"
        if [ $((i % 30)) -eq 0 ]; then
            echo "    $i/300 text files" >&2
        fi
    done

    # 2 GB of duplicate data — copy 200 random binaries to simulate dedup
    echo "  duplicate binaries (2 GB)..." >&2
    for i in $(seq 1 200); do
        src=$((RANDOM % 500 + 1))
        cp "$DATADIR/rand_${src}.bin" "$DATADIR/dup_${i}.bin"
    done

    touch "$STAMP"
    echo "  dataset ready." >&2
fi

INPUT_KB=$(du -sk "$DATADIR" | awk '{print $1}')
INPUT_MB=$(python3 -c "print(f'{$INPUT_KB/1024:.0f}')")
INPUT_GB=$(python3 -c "print(f'{$INPUT_KB/1024/1024:.1f}')")
FILE_COUNT=$(find "$DATADIR" -type f -not -name '.generated' | wc -l | tr -d ' ')
echo "Dataset: ${INPUT_GB} GB, ${FILE_COUNT} files" >&2

# Warm filesystem cache
echo "Warming filesystem cache..." >&2
find "$DATADIR" -type f -not -name '.generated' -exec cat {} + > /dev/null 2>&1

WORKDIR=$(mktemp -d)
trap "rm -rf $WORKDIR" EXIT

echo "threads,time_ms,throughput_mbs"

# Build thread count list: 1, 2, 4, 8, 12, 16, 20, 24, 28, 32, ...
# Step by 4 from 4 upward to cover Threadripper well
THREAD_COUNTS="1 2 4"
t=8
while [ $t -le $MAX_CORES ]; do
    THREAD_COUNTS="$THREAD_COUNTS $t"
    t=$((t + 4))
done
# Always include max
echo "$THREAD_COUNTS" | grep -qw "$MAX_CORES" || THREAD_COUNTS="$THREAD_COUNTS $MAX_CORES"

echo "" >&2
for threads in $THREAD_COUNTS; do
    total_ms=0
    runs=3
    for r in $(seq 1 $runs); do
        rm -f "$WORKDIR/bench.tg"
        start=$(python3 -c 'import time; print(int(time.time()*1000))')
        $TDG create "$WORKDIR/bench.tg" "$DATADIR" --quiet -j $threads
        end=$(python3 -c 'import time; print(int(time.time()*1000))')
        ms=$((end - start))
        total_ms=$((total_ms + ms))
    done
    avg_ms=$((total_ms / runs))
    throughput=$(python3 -c "print(f'{$INPUT_KB/1024/max($avg_ms/1000,0.001):.1f}')")
    echo "$threads,$avg_ms,$throughput"
    echo "  ${threads} threads: ${avg_ms}ms (${throughput} MB/s)" >&2
done

echo "" >&2
echo "Done." >&2
