#!/bin/bash
# Run all benchmarks and generate plots.
# Uses the installed tdg binary (from PATH or $TDG).
# Creates a temporary Python venv for plotting and cleans it up.
#
# Usage: bash bench/run-all.sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"

# Locate tdg binary — prefer $TDG env, then PATH
if [ -n "$TDG" ] && [ -x "$TDG" ]; then
    :
elif command -v tdg >/dev/null 2>&1; then
    TDG="$(command -v tdg)"
else
    echo "ERROR: tdg not found in PATH and \$TDG not set."
    echo "Install with: cargo install tardigrade"
    exit 1
fi
export TDG

TDG_VERSION=$($TDG --version 2>&1 | head -1)
echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║  tardigrade benchmark suite                  ║"
echo "╚══════════════════════════════════════════════╝"
echo ""
echo "  binary:  $TDG"
echo "  version: $TDG_VERSION"
echo "  date:    $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "  host:    $(uname -m) / $(uname -s)"
echo "  cores:   $(python3 -c 'import os; print(os.cpu_count())')"
echo ""

# Record metadata for plots
META="$SCRIPT_DIR/meta.txt"
echo "version=$TDG_VERSION" > "$META"
echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$META"
echo "host=$(uname -m)/$(uname -s)" >> "$META"
echo "cores=$(python3 -c 'import os; print(os.cpu_count())')" >> "$META"

echo "── Comparison: tdg vs tar+zstd ──"
bash "$SCRIPT_DIR/run.sh" > "$SCRIPT_DIR/results.csv"

echo ""
echo "── Core scaling ──"
bash "$SCRIPT_DIR/scaling.sh" > "$SCRIPT_DIR/scaling.csv"

echo ""
echo "── Generating plots ──"

# Create temporary venv, install matplotlib, generate plots, clean up
VENV_DIR=$(mktemp -d)
python3 -m venv "$VENV_DIR"
"$VENV_DIR/bin/pip" install -q -r "$SCRIPT_DIR/requirements.txt"
PYTHON="$VENV_DIR/bin/python3"

$PYTHON "$SCRIPT_DIR/plot.py" "$SCRIPT_DIR/results.csv" "$SCRIPT_DIR/"
$PYTHON "$SCRIPT_DIR/plot_scaling.py" "$SCRIPT_DIR/scaling.csv" "$SCRIPT_DIR/"

rm -rf "$VENV_DIR"

echo ""
echo "Done. Files updated:"
ls -la "$SCRIPT_DIR"/results.csv "$SCRIPT_DIR"/scaling.csv "$SCRIPT_DIR"/*.svg "$META"
echo ""
echo "Commit these with: git add bench/ && git commit -m 'bench: update benchmark results'"
