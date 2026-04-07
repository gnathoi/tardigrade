#!/usr/bin/env python3
"""Generate benchmark plots from CSV data."""
import csv
import sys
import os

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

DATASET_LABELS = {
    'source_project': 'Source Project\n(270 files, 5 MB)',
    'dedup_heavy': 'Heavy Dedup\n(shared deps, 13 MB)',
    'large_mixed': 'Large Mixed\n(logs+bins, 102 MB)',
}

TDG_COLOR = '#22c55e'   # green
TAR_COLOR = '#94a3b8'   # slate gray
ACCENT = '#f59e0b'      # amber for highlights

def load_csv(path):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            row['time_ms'] = int(row['time_ms'])
            rows.append(row)
    return rows

def setup_ax(ax):
    """Transparent background, clean look for GitHub light+dark."""
    ax.set_facecolor('none')
    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.spines['bottom'].set_color('#666')
    ax.spines['left'].set_color('#666')
    ax.tick_params(colors='#666')

def plot_speed(rows, output_dir):
    """Grouped bar chart: create and extract times side by side."""
    datasets = [d for d in DATASET_LABELS if any(r['dataset'] == d for r in rows)]

    fig, axes = plt.subplots(1, 2, figsize=(11, 4.5))
    fig.patch.set_alpha(0)

    for ax, op, title in [(axes[0], 'create', 'Create'), (axes[1], 'extract', 'Extract')]:
        setup_ax(ax)
        x = np.arange(len(datasets))
        w = 0.32

        tdg_t = [next((r['time_ms'] for r in rows if r['tool']=='tdg' and r['dataset']==d and r['operation']==op), 0) for d in datasets]
        tar_t = [next((r['time_ms'] for r in rows if r['tool']=='tar+zstd' and r['dataset']==d and r['operation']==op), 0) for d in datasets]

        bars1 = ax.bar(x - w/2, tdg_t, w, label='tdg', color=TDG_COLOR, edgecolor='none', zorder=3)
        bars2 = ax.bar(x + w/2, tar_t, w, label='tar+zstd', color=TAR_COLOR, edgecolor='none', zorder=3)

        # Value labels
        for bars, bold in [(bars1, True), (bars2, False)]:
            for bar in bars:
                h = bar.get_height()
                ax.text(bar.get_x() + bar.get_width()/2, h + max(tar_t)*0.02,
                        f'{int(h)}ms', ha='center', va='bottom', fontsize=9,
                        color='#333', fontweight='bold' if bold else 'normal')

        # Speedup annotations
        for i in range(len(datasets)):
            if tar_t[i] > 0 and tdg_t[i] > 0:
                speedup = tar_t[i] / tdg_t[i]
                if speedup > 1.3:
                    ax.annotate(f'{speedup:.1f}x faster',
                                xy=(x[i], 0), xytext=(x[i], max(tar_t)*0.85),
                                ha='center', fontsize=8, color=TDG_COLOR, fontweight='bold',
                                arrowprops=dict(arrowstyle='-', color=TDG_COLOR, alpha=0.3))

        ax.set_xticks(x)
        ax.set_xticklabels([DATASET_LABELS[d] for d in datasets], fontsize=8, color='#555')
        ax.set_ylabel('Time (ms)', color='#555')
        ax.set_title(title, fontsize=13, fontweight='bold', color='#333')
        ax.legend(fontsize=9, framealpha=0.5)
        ax.set_ylim(0, max(max(tdg_t), max(tar_t)) * 1.25)
        ax.grid(axis='y', alpha=0.2, zorder=0)

    fig.suptitle('tardigrade vs tar+zstd — Speed (lower is better)',
                 fontsize=14, fontweight='bold', color='#333')
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-speed.svg'), format='svg',
                bbox_inches='tight', transparent=True)
    plt.close(fig)

def plot_size(rows, output_dir):
    """Horizontal bar chart: archive sizes with dedup savings highlighted."""
    datasets = [d for d in DATASET_LABELS if any(r['dataset'] == d for r in rows)]

    fig, ax = plt.subplots(figsize=(9, 4))
    fig.patch.set_alpha(0)
    setup_ax(ax)

    y = np.arange(len(datasets))
    h = 0.3

    tdg_sizes = []
    tar_sizes = []
    input_sizes = []
    for d in datasets:
        tdg = next((r for r in rows if r['tool']=='tdg' and r['dataset']==d and r['operation']=='create'), None)
        tar = next((r for r in rows if r['tool']=='tar+zstd' and r['dataset']==d and r['operation']=='create'), None)
        tdg_sizes.append(float(tdg['output_mb']) if tdg and tdg['output_mb'] else 0)
        tar_sizes.append(float(tar['output_mb']) if tar and tar['output_mb'] else 0)
        input_sizes.append(float(tdg['input_mb']) if tdg else 0)

    # Input size as faint background
    ax.barh(y, input_sizes, h*2.5, color='#e2e8f0', edgecolor='none', zorder=1, label='Input')
    ax.barh(y + h/2, tar_sizes, h, color=TAR_COLOR, edgecolor='none', zorder=2, label='tar+zstd')
    ax.barh(y - h/2, tdg_sizes, h, color=TDG_COLOR, edgecolor='none', zorder=2, label='tdg')

    # Labels
    for i in range(len(datasets)):
        ax.text(tdg_sizes[i] + max(input_sizes)*0.01, y[i] - h/2,
                f' {tdg_sizes[i]:.1f} MB', va='center', fontsize=9, color='#333', fontweight='bold')
        ax.text(tar_sizes[i] + max(input_sizes)*0.01, y[i] + h/2,
                f' {tar_sizes[i]:.1f} MB', va='center', fontsize=9, color='#666')

        # Savings callout
        if tar_sizes[i] > 0:
            pct = (1 - tdg_sizes[i]/tar_sizes[i]) * 100
            if pct > 5:
                ax.text(max(input_sizes)*0.95, y[i],
                        f'{pct:.0f}% smaller', ha='right', va='center',
                        fontsize=9, color=TDG_COLOR, fontweight='bold',
                        bbox=dict(boxstyle='round,pad=0.3', facecolor='white', edgecolor=TDG_COLOR, alpha=0.8))

    ax.set_yticks(y)
    ax.set_yticklabels([DATASET_LABELS[d] for d in datasets], fontsize=9, color='#555')
    ax.set_xlabel('Size (MB)', color='#555')
    ax.set_title('Archive Size (smaller is better)', fontsize=13, fontweight='bold', color='#333')
    ax.legend(fontsize=9, loc='lower right', framealpha=0.5)
    ax.grid(axis='x', alpha=0.2, zorder=0)

    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-size.svg'), format='svg',
                bbox_inches='tight', transparent=True)
    plt.close(fig)

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/results.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    os.makedirs(output_dir, exist_ok=True)

    rows = load_csv(csv_path)
    plot_speed(rows, output_dir)
    plot_size(rows, output_dir)
    print(f"Plots saved to {output_dir}/")
