#!/bin/bash
# Benchmark tardigrade scaling across thread counts
set -e

TDG="${TDG:-./target/release/tdg}"
WORKDIR=$(mktemp -d)
trap "rm -rf $WORKDIR" EXIT

MAX_CORES=$(python3 -c "import os; print(os.cpu_count())")
echo "Detected $MAX_CORES cores" >&2

# Create a dataset large enough to be CPU-bound
echo "Creating benchmark dataset..." >&2
mkdir -p "$WORKDIR/data"
for i in $(seq 1 50); do
    dd if=/dev/urandom bs=1024 count=512 of="$WORKDIR/data/file_$i.bin" 2>/dev/null
done
# Add compressible text
for i in $(seq 1 20); do
    yes "repeated text line $i for compression scaling test" | head -50000 > "$WORKDIR/data/text_$i.txt"
done
INPUT_KB=$(du -sk "$WORKDIR/data" | awk '{print $1}')
INPUT_MB=$(python3 -c "print(f'{$INPUT_KB/1024:.1f}')")
echo "Dataset: ${INPUT_MB}MB" >&2

# Warm filesystem cache
find "$WORKDIR/data" -type f -exec cat {} + > /dev/null 2>&1

echo "threads,time_ms,throughput_mbs"

# Test at 1, 2, 4, 6, 8, ... up to MAX_CORES
THREAD_COUNTS="1 2"
t=4
while [ $t -le $MAX_CORES ]; do
    THREAD_COUNTS="$THREAD_COUNTS $t"
    t=$((t + 2))
done
# Always include max
echo "$THREAD_COUNTS" | grep -q "$MAX_CORES" || THREAD_COUNTS="$THREAD_COUNTS $MAX_CORES"

for threads in $THREAD_COUNTS; do
    total_ms=0
    runs=3
    for r in $(seq 1 $runs); do
        rm -f "$WORKDIR/bench.tg"
        start=$(python3 -c 'import time; print(int(time.time()*1000))')
        $TDG create "$WORKDIR/bench.tg" "$WORKDIR/data" --quiet -j $threads
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
