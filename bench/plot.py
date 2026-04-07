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

TDG_COLOR = '#059669'   # emerald-600 — saturated, readable
TAR_COLOR = '#6b7280'   # gray-500
TEXT_COLOR = '#111827'   # gray-900 — high contrast
DIM_COLOR = '#374151'    # gray-700
GRID_COLOR = '#e5e7eb'   # gray-200

def load_csv(path):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            row['time_ms'] = int(row['time_ms'])
            rows.append(row)
    return rows

def setup_ax(ax):
    ax.set_facecolor('white')
    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.spines['bottom'].set_color('#d1d5db')
    ax.spines['left'].set_color('#d1d5db')
    ax.tick_params(colors=DIM_COLOR, labelsize=9)
    ax.grid(axis='y', color=GRID_COLOR, linewidth=0.5, zorder=0)

def plot_speed(rows, output_dir):
    datasets = [d for d in DATASET_LABELS if any(r['dataset'] == d for r in rows)]

    fig, axes = plt.subplots(1, 2, figsize=(11, 4.5))
    fig.patch.set_facecolor('white')

    for ax, op, title in [(axes[0], 'create', 'Create'), (axes[1], 'extract', 'Extract')]:
        setup_ax(ax)
        x = np.arange(len(datasets))
        w = 0.32

        tdg_t = [next((r['time_ms'] for r in rows if r['tool']=='tdg' and r['dataset']==d and r['operation']==op), 0) for d in datasets]
        tar_t = [next((r['time_ms'] for r in rows if r['tool']=='tar+zstd' and r['dataset']==d and r['operation']==op), 0) for d in datasets]

        bars1 = ax.bar(x - w/2, tdg_t, w, label='tdg', color=TDG_COLOR, edgecolor='none', zorder=3)
        bars2 = ax.bar(x + w/2, tar_t, w, label='tar+zstd', color=TAR_COLOR, edgecolor='none', zorder=3)

        ymax = max(max(tdg_t), max(tar_t))
        for bars, bold in [(bars1, True), (bars2, False)]:
            for bar in bars:
                h = bar.get_height()
                ax.text(bar.get_x() + bar.get_width()/2, h + ymax*0.02,
                        f'{int(h)}ms', ha='center', va='bottom', fontsize=10,
                        color=TEXT_COLOR, fontweight='bold' if bold else 'normal')

        for i in range(len(datasets)):
            if tar_t[i] > 0 and tdg_t[i] > 0:
                speedup = tar_t[i] / tdg_t[i]
                if speedup > 1.3:
                    ax.text(x[i], ymax * 1.05, f'{speedup:.1f}x faster',
                            ha='center', fontsize=10, color=TDG_COLOR, fontweight='bold')

        ax.set_xticks(x)
        ax.set_xticklabels([DATASET_LABELS[d] for d in datasets], fontsize=9, color=DIM_COLOR)
        ax.set_ylabel('Time (ms)', color=DIM_COLOR, fontsize=10)
        ax.set_title(title, fontsize=14, fontweight='bold', color=TEXT_COLOR)
        ax.legend(fontsize=10, frameon=True, facecolor='white', edgecolor=GRID_COLOR)
        ax.set_ylim(0, ymax * 1.3)

    fig.suptitle('tardigrade vs tar+zstd  —  Speed (lower is better)',
                 fontsize=15, fontweight='bold', color=TEXT_COLOR, y=1.01)
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-speed.svg'), format='svg',
                bbox_inches='tight', facecolor='white', edgecolor='none')
    plt.close(fig)

def plot_size(rows, output_dir):
    datasets = [d for d in DATASET_LABELS if any(r['dataset'] == d for r in rows)]

    fig, ax = plt.subplots(figsize=(9, 4))
    fig.patch.set_facecolor('white')
    setup_ax(ax)
    ax.grid(axis='x', color=GRID_COLOR, linewidth=0.5, zorder=0)
    ax.grid(axis='y', visible=False)

    y = np.arange(len(datasets))
    h = 0.28

    tdg_sizes, tar_sizes, input_sizes = [], [], []
    for d in datasets:
        tdg = next((r for r in rows if r['tool']=='tdg' and r['dataset']==d and r['operation']=='create'), None)
        tar = next((r for r in rows if r['tool']=='tar+zstd' and r['dataset']==d and r['operation']=='create'), None)
        tdg_sizes.append(float(tdg['output_mb']) if tdg and tdg['output_mb'] else 0)
        tar_sizes.append(float(tar['output_mb']) if tar and tar['output_mb'] else 0)
        input_sizes.append(float(tdg['input_mb']) if tdg else 0)

    ax.barh(y, input_sizes, h*2.5, color='#f3f4f6', edgecolor='#e5e7eb', zorder=1, label='Input')
    ax.barh(y + h/2, tar_sizes, h, color=TAR_COLOR, edgecolor='none', zorder=2, label='tar+zstd')
    ax.barh(y - h/2, tdg_sizes, h, color=TDG_COLOR, edgecolor='none', zorder=2, label='tdg')

    xmax = max(input_sizes) * 1.1
    for i in range(len(datasets)):
        ax.text(tdg_sizes[i] + xmax*0.01, y[i] - h/2,
                f'{tdg_sizes[i]:.1f} MB', va='center', fontsize=10,
                color=TEXT_COLOR, fontweight='bold')
        ax.text(tar_sizes[i] + xmax*0.01, y[i] + h/2,
                f'{tar_sizes[i]:.1f} MB', va='center', fontsize=10, color=DIM_COLOR)

        if tar_sizes[i] > 0:
            pct = (1 - tdg_sizes[i]/tar_sizes[i]) * 100
            if pct > 5:
                ax.text(xmax * 0.97, y[i],
                        f'{pct:.0f}% smaller', ha='right', va='center',
                        fontsize=10, color=TDG_COLOR, fontweight='bold')

    ax.set_yticks(y)
    ax.set_yticklabels([DATASET_LABELS[d] for d in datasets], fontsize=10, color=DIM_COLOR)
    ax.set_xlabel('Size (MB)', color=DIM_COLOR, fontsize=10)
    ax.set_title('Archive Size (smaller is better)', fontsize=14, fontweight='bold', color=TEXT_COLOR)
    ax.legend(fontsize=10, loc='lower right', frameon=True, facecolor='white', edgecolor=GRID_COLOR)

    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-size.svg'), format='svg',
                bbox_inches='tight', facecolor='white', edgecolor='none')
    plt.close(fig)

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/results.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    os.makedirs(output_dir, exist_ok=True)

    rows = load_csv(csv_path)
    plot_speed(rows, output_dir)
    plot_size(rows, output_dir)
    print(f"Plots saved to {output_dir}/")
