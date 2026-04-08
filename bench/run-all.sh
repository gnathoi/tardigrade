#!/bin/bash
# Run all benchmarks and generate plots.
# Run this locally before shipping a release.
#
# Usage: bash bench/run-all.sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"

echo "Building release binary..."
cargo build --release --manifest-path "$REPO_DIR/Cargo.toml" 2>&1 | tail -1

export TDG="$REPO_DIR/target/release/tdg"

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║  tardigrade benchmark suite                   ║"
echo "╚══════════════════════════════════════════════╝"

echo ""
echo "── Comparison: tdg vs tar+zstd ──"
bash "$SCRIPT_DIR/run.sh" > "$SCRIPT_DIR/results.csv"

echo ""
echo "── Core scaling ──"
bash "$SCRIPT_DIR/scaling.sh" > "$SCRIPT_DIR/scaling.csv"

echo ""
echo "── Generating plots ──"

# Use venv if available, otherwise try system python
PYTHON=""
if [ -x "$REPO_DIR/.venv/bin/python3" ]; then
    PYTHON="$REPO_DIR/.venv/bin/python3"
elif python3 -c "import matplotlib" 2>/dev/null; then
    PYTHON="python3"
else
    echo "matplotlib not found. Install: python3 -m venv .venv && .venv/bin/pip install matplotlib"
    exit 1
fi

$PYTHON "$SCRIPT_DIR/plot.py" "$SCRIPT_DIR/results.csv" "$SCRIPT_DIR/"
$PYTHON "$SCRIPT_DIR/plot_scaling.py" "$SCRIPT_DIR/scaling.csv" "$SCRIPT_DIR/"

echo ""
echo "Done. Files updated:"
ls -la "$SCRIPT_DIR"/results.csv "$SCRIPT_DIR"/scaling.csv "$SCRIPT_DIR"/*.svg
echo ""
echo "Commit these with: git add bench/ && git commit -m 'bench: update benchmark results'"
