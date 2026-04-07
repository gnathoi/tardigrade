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

create_datasets() {
    >&2 echo "Creating benchmark datasets..."

    # Dataset 1: Source code project (~10MB, realistic mix of sizes)
    mkdir -p "$WORKDIR/source_project/src" "$WORKDIR/source_project/tests" "$WORKDIR/source_project/docs"
    for i in $(seq 1 200); do
        # Source files: 1-20KB each (realistic for code)
        python3 -c "
import random, string
lines = random.randint(50, 500)
print('\n'.join(''.join(random.choices(string.ascii_lowercase + '    \n{}();', k=random.randint(20, 120))) for _ in range(lines)))
" > "$WORKDIR/source_project/src/module_$i.rs"
    done
    for i in $(seq 1 50); do
        python3 -c "
import random, string
lines = random.randint(20, 200)
print('\n'.join('fn test_' + str(j) + '() { assert!(true); }' for j in range(lines)))
" > "$WORKDIR/source_project/tests/test_$i.rs"
    done
    for i in $(seq 1 20); do
        yes "Documentation content for module $i with various details about usage." | head -500 > "$WORKDIR/source_project/docs/doc_$i.md"
    done

    # Dataset 2: Project with heavy duplication (simulates monorepo / node_modules)
    # Multiple packages sharing many identical or near-identical files
    mkdir -p "$WORKDIR/dedup_heavy"
    # Create a "base package" (~2MB)
    mkdir -p "$WORKDIR/dedup_heavy/base"
    for i in $(seq 1 40); do
        dd if=/dev/urandom bs=1024 count=50 of="$WORKDIR/dedup_heavy/base/lib_$i.bin" 2>/dev/null
    done
    # 5 copies with slight variations (simulates 5 packages sharing deps)
    for pkg in $(seq 1 5); do
        cp -r "$WORKDIR/dedup_heavy/base" "$WORKDIR/dedup_heavy/pkg_$pkg"
        # Each package has 2-3 unique files
        for u in $(seq 1 3); do
            dd if=/dev/urandom bs=1024 count=50 of="$WORKDIR/dedup_heavy/pkg_$pkg/unique_$u.bin" 2>/dev/null
        done
    done

    # Dataset 3: Large mixed workload (binaries + text + duplicates)
    mkdir -p "$WORKDIR/large_mixed"
    # Big compressible text files
    for i in $(seq 1 5); do
        yes "log entry $(date) level=INFO msg=\"processing request $i\" duration=42ms" | head -200000 > "$WORKDIR/large_mixed/log_$i.txt"
    done
    # Medium binary files
    for i in $(seq 1 10); do
        dd if=/dev/urandom bs=1024 count=1024 of="$WORKDIR/large_mixed/data_$i.bin" 2>/dev/null
    done
    # Some duplicates of the binary files
    for i in $(seq 1 5); do
        cp "$WORKDIR/large_mixed/data_$i.bin" "$WORKDIR/large_mixed/backup_$i.bin"
    done

    for ds in source_project dedup_heavy large_mixed; do
        local size_kb=$(du -sk "$WORKDIR/$ds" | awk '{print $1}')
        local files=$(find "$WORKDIR/$ds" -type f | wc -l | tr -d ' ')
        >&2 echo "  $ds: ${files} files, ${size_kb} KB"
    done
}

bench_one() {
    local dataset=$1 path=$2 runs=${3:-3}
    local input_kb=$(du -sk "$path" | awk '{print $1}')
    local input_mb=$(python3 -c "print(f'{$input_kb/1024:.2f}')")

    # Warm filesystem cache
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

    echo "tdg,$dataset,create,$tdg_create,$input_mb,$tdg_size_mb,$tdg_ratio"
    echo "tar+zstd,$dataset,create,$tar_create,$input_mb,$tar_size_mb,$tar_ratio"
    echo "tdg,$dataset,extract,$tdg_extract,$input_mb,,,"
    echo "tar+zstd,$dataset,extract,$tar_extract,$input_mb,,,"

    local create_speedup=$(python3 -c "print(f'{max($tar_create,1)/max($tdg_create,1):.1f}')")
    local extract_speedup=$(python3 -c "print(f'{max($tar_extract,1)/max($tdg_extract,1):.1f}')")
    local size_savings=$(python3 -c "print(f'{(1-$tdg_size_kb/max($tar_size_kb,1))*100:.0f}')")
    >&2 echo "  $dataset:"
    >&2 echo "    create:  tdg ${tdg_create}ms vs tar+zstd ${tar_create}ms — ${create_speedup}x"
    >&2 echo "    extract: tdg ${tdg_extract}ms vs tar+zstd ${tar_extract}ms — ${extract_speedup}x"
    >&2 echo "    size:    tdg ${tdg_size_mb}MB vs tar+zstd ${tar_size_mb}MB — ${size_savings}% smaller"
}

create_datasets

>&2 echo ""
>&2 echo "Running benchmarks (3 runs each, averaged)..."
>&2 echo ""

echo "tool,dataset,operation,time_ms,input_mb,output_mb,ratio"
bench_one "source_project" "$WORKDIR/source_project"
bench_one "dedup_heavy" "$WORKDIR/dedup_heavy"
bench_one "large_mixed" "$WORKDIR/large_mixed"

>&2 echo ""
>&2 echo "Done."
