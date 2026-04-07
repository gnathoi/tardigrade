#!/bin/bash
# Benchmark tardigrade vs tar+zstd
# Outputs CSV to stdout, human-readable to stderr
set -e

TDG="${TDG:-./target/release/tdg}"
WORKDIR=$(mktemp -d)
trap "rm -rf $WORKDIR" EXIT

# Timing helper — outputs milliseconds
time_ms() {
    local start end
    start=$(python3 -c 'import time; print(int(time.time()*1000))')
    "$@" >/dev/null 2>&1
    end=$(python3 -c 'import time; print(int(time.time()*1000))')
    echo $((end - start))
}

# Create datasets
create_datasets() {
    >&2 echo "Creating benchmark datasets..."

    # Dataset 1: Many small files (simulates source code)
    mkdir -p "$WORKDIR/small_files"
    for i in $(seq 1 500); do
        printf "// file %d\nfn main() { println!(\"hello %d\"); }\n" "$i" "$i" > "$WORKDIR/small_files/file_$i.rs"
    done

    # Dataset 2: Mixed binary + duplicates (simulates project with node_modules)
    mkdir -p "$WORKDIR/mixed_dedup"
    dd if=/dev/urandom bs=1024 count=512 of="$WORKDIR/mixed_dedup/base.bin" 2>/dev/null
    for i in $(seq 1 30); do
        cp "$WORKDIR/mixed_dedup/base.bin" "$WORKDIR/mixed_dedup/copy_$i.bin"
    done
    for i in $(seq 1 20); do
        dd if=/dev/urandom bs=1024 count=$((RANDOM % 256 + 64)) of="$WORKDIR/mixed_dedup/unique_$i.bin" 2>/dev/null
    done

    # Dataset 3: Large compressible text
    mkdir -p "$WORKDIR/large_text"
    for i in $(seq 1 10); do
        yes "repeated line $i for compression benchmarking purposes — tardigrade vs tar" | head -100000 > "$WORKDIR/large_text/text_$i.txt"
    done

    for ds in small_files mixed_dedup large_text; do
        local size_kb=$(du -sk "$WORKDIR/$ds" | awk '{print $1}')
        local files=$(find "$WORKDIR/$ds" -type f | wc -l | tr -d ' ')
        >&2 echo "  $ds: ${files} files, ${size_kb} KB"
    done
}

# Run a single benchmark
bench_one() {
    local dataset=$1 path=$2 runs=${3:-3}
    local input_kb=$(du -sk "$path" | awk '{print $1}')
    local input_mb=$(python3 -c "print(f'{$input_kb/1024:.2f}')")

    # Warm up filesystem cache
    find "$path" -type f -exec cat {} + > /dev/null 2>&1

    # --- tardigrade create ---
    local tdg_create_total=0
    for r in $(seq 1 $runs); do
        rm -f "$WORKDIR/bench.tg"
        local ms=$(time_ms $TDG create "$WORKDIR/bench.tg" "$path" --quiet)
        tdg_create_total=$((tdg_create_total + ms))
    done
    local tdg_create=$((tdg_create_total / runs))
    local tdg_size_kb=$(du -sk "$WORKDIR/bench.tg" | awk '{print $1}')
    local tdg_size_mb=$(python3 -c "print(f'{$tdg_size_kb/1024:.2f}')")
    local tdg_ratio=$(python3 -c "print(f'{$input_kb/max($tdg_size_kb,1):.2f}')")

    # --- tar+zstd create ---
    local tar_create_total=0
    for r in $(seq 1 $runs); do
        rm -f "$WORKDIR/bench.tar.zst"
        local ms=$(time_ms sh -c "tar cf - -C $(dirname $path) $(basename $path) | zstd -3 -q -o $WORKDIR/bench.tar.zst")
        tar_create_total=$((tar_create_total + ms))
    done
    local tar_create=$((tar_create_total / runs))
    local tar_size_kb=$(du -sk "$WORKDIR/bench.tar.zst" | awk '{print $1}')
    local tar_size_mb=$(python3 -c "print(f'{$tar_size_kb/1024:.2f}')")
    local tar_ratio=$(python3 -c "print(f'{$input_kb/max($tar_size_kb,1):.2f}')")

    # --- tardigrade extract ---
    local tdg_extract_total=0
    for r in $(seq 1 $runs); do
        rm -rf "$WORKDIR/extract-tdg"
        local ms=$(time_ms $TDG extract "$WORKDIR/bench.tg" -o "$WORKDIR/extract-tdg" --quiet)
        tdg_extract_total=$((tdg_extract_total + ms))
    done
    local tdg_extract=$((tdg_extract_total / runs))

    # --- tar+zstd extract ---
    local tar_extract_total=0
    for r in $(seq 1 $runs); do
        rm -rf "$WORKDIR/extract-tar"
        mkdir -p "$WORKDIR/extract-tar"
        local ms=$(time_ms sh -c "zstd -d -q $WORKDIR/bench.tar.zst -o $WORKDIR/bench.tar && tar xf $WORKDIR/bench.tar -C $WORKDIR/extract-tar")
        tar_extract_total=$((tar_extract_total + ms))
        rm -f "$WORKDIR/bench.tar"
    done
    local tar_extract=$((tar_extract_total / runs))

    # Output CSV rows
    echo "tdg,$dataset,create,$tdg_create,$input_mb,$tdg_size_mb,$tdg_ratio"
    echo "tar+zstd,$dataset,create,$tar_create,$input_mb,$tar_size_mb,$tar_ratio"
    echo "tdg,$dataset,extract,$tdg_extract,$input_mb,,,"
    echo "tar+zstd,$dataset,extract,$tar_extract,$input_mb,,,"

    # Human-readable
    local create_speedup=$(python3 -c "print(f'{max($tar_create,1)/max($tdg_create,1):.1f}')")
    local extract_speedup=$(python3 -c "print(f'{max($tar_extract,1)/max($tdg_extract,1):.1f}')")
    >&2 echo "  $dataset:"
    >&2 echo "    create:  tdg ${tdg_create}ms (${tdg_size_mb}MB, ${tdg_ratio}x) vs tar+zstd ${tar_create}ms (${tar_size_mb}MB, ${tar_ratio}x) — ${create_speedup}x"
    >&2 echo "    extract: tdg ${tdg_extract}ms vs tar+zstd ${tar_extract}ms — ${extract_speedup}x"
}

create_datasets

>&2 echo ""
>&2 echo "Running benchmarks (3 runs each, averaged)..."
>&2 echo ""

echo "tool,dataset,operation,time_ms,input_mb,output_mb,ratio"
bench_one "small_files" "$WORKDIR/small_files"
bench_one "mixed_dedup" "$WORKDIR/mixed_dedup"
bench_one "large_text" "$WORKDIR/large_text"

>&2 echo ""
>&2 echo "Done."
