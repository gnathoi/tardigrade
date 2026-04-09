#!/usr/bin/env python3
"""Generate benchmark plots — CERN/scientific style on dark background."""
import csv
import sys
import os

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np

DATASET_LABELS = {
    'source_project': 'Source Project\n270 files, 5 MB',
    'dedup_heavy': 'Heavy Dedup\nshared deps, 13 MB',
    'large_mixed': 'Large Mixed\nlogs+bins, 102 MB',
    '10gb_mixed': '10 GB Mixed\n1000 files, 10 GB',
    'dedup_10gb': '10 GB Dedup\nbackup snapshots, 10 GB',
}

BG = '#0e1117'
PANEL = '#161b22'
GRID = '#21262d'
BORDER = '#30363d'
TEXT = '#e6edf3'
DIM = '#8b949e'
TDG = '#3fb950'       # GitHub green
TAR = '#8b949e'
CALLOUT = '#d29922'    # amber

plt.rcParams.update({
    'font.family': 'monospace',
    'font.size': 9,
    'text.color': TEXT,
    'axes.labelcolor': DIM,
    'xtick.color': DIM,
    'ytick.color': DIM,
})

def load_meta(csv_dir):
    meta_path = os.path.join(csv_dir, 'meta.txt')
    meta = {}
    if os.path.exists(meta_path):
        with open(meta_path) as f:
            for line in f:
                if '=' in line:
                    k, v = line.strip().split('=', 1)
                    meta[k] = v
    return meta

def load_csv(path):
    rows = []
    with open(path) as f:
        for row in csv.DictReader(f):
            row['time_ms'] = int(row['time_ms'])
            rows.append(row)
    return rows

def setup_ax(ax):
    ax.set_facecolor(PANEL)
    for spine in ax.spines.values():
        spine.set_color(BORDER)
    ax.tick_params(colors=DIM, length=3, width=0.5)
    ax.grid(axis='y', color=GRID, linewidth=0.5, zorder=0)

def plot_speed(rows, output_dir):
    datasets = [d for d in DATASET_LABELS if any(r['dataset'] == d for r in rows)]
    fig, axes = plt.subplots(1, 2, figsize=(12, 4.2))
    fig.patch.set_facecolor(BG)

    for ax, op, title in [(axes[0], 'create', 'CREATE'), (axes[1], 'extract', 'EXTRACT')]:
        setup_ax(ax)
        x = np.arange(len(datasets))
        w = 0.30

        tdg_t = [next((r['time_ms'] for r in rows if r['tool']=='tdg' and r['dataset']==d and r['operation']==op), 0) for d in datasets]
        tar_t = [next((r['time_ms'] for r in rows if r['tool']=='tar+zstd' and r['dataset']==d and r['operation']==op), 0) for d in datasets]

        ax.bar(x - w/2, tdg_t, w, color=TDG, edgecolor='none', zorder=3, label='tdg')
        ax.bar(x + w/2, tar_t, w, color=TAR, edgecolor='none', zorder=3, alpha=0.6, label='tar+zstd')

        ymax = max(max(tdg_t), max(tar_t))
        for i in range(len(datasets)):
            ax.text(x[i] - w/2, tdg_t[i] + ymax*0.02, f'{tdg_t[i]}',
                    ha='center', va='bottom', fontsize=9, color=TDG, fontweight='bold', fontfamily='monospace')
            ax.text(x[i] + w/2, tar_t[i] + ymax*0.02, f'{tar_t[i]}',
                    ha='center', va='bottom', fontsize=9, color=DIM, fontfamily='monospace')

        ax.set_xticks(x)
        ax.set_xticklabels([DATASET_LABELS[d] for d in datasets], fontsize=8)
        ax.set_ylabel('ms', fontsize=9)
        ax.set_title(title, fontsize=11, fontweight='bold', color=TEXT, loc='left', pad=8)
        ax.set_ylim(0, ymax * 1.3)

    # Single shared legend below the figure
    handles, labels = axes[0].get_legend_handles_labels()
    fig.legend(handles, labels, loc='lower center', ncol=2, fontsize=8,
               facecolor=PANEL, edgecolor=BORDER, labelcolor=DIM)

    meta = load_meta(output_dir)
    subtitle = 'tdg vs tar+zstd  /  speed  /  lower is better'
    if meta.get('version'):
        subtitle += f'  [{meta["version"]}]'
    fig.suptitle(subtitle, fontsize=10, color=DIM, fontfamily='monospace', y=0.98)
    fig.tight_layout(rect=[0, 0.06, 1, 0.95])
    fig.savefig(os.path.join(output_dir, 'bench-speed.svg'), format='svg',
                bbox_inches='tight', facecolor=BG)
    plt.close(fig)

def plot_size(rows, output_dir):
    datasets = [d for d in DATASET_LABELS if any(r['dataset'] == d for r in rows)]
    fig, ax = plt.subplots(figsize=(10, 3.8))
    fig.patch.set_facecolor(BG)
    setup_ax(ax)
    ax.grid(axis='y', visible=False)
    ax.grid(axis='x', color=GRID, linewidth=0.5, zorder=0)

    y = np.arange(len(datasets))
    h = 0.25

    tdg_sizes, tar_sizes, input_sizes = [], [], []
    for d in datasets:
        tdg = next((r for r in rows if r['tool']=='tdg' and r['dataset']==d and r['operation']=='create'), None)
        tar = next((r for r in rows if r['tool']=='tar+zstd' and r['dataset']==d and r['operation']=='create'), None)
        tdg_sizes.append(float(tdg['output_mb']) if tdg and tdg['output_mb'] else 0)
        tar_sizes.append(float(tar['output_mb']) if tar and tar['output_mb'] else 0)
        input_sizes.append(float(tdg['input_mb']) if tdg else 0)

    ax.barh(y, input_sizes, h*2.8, color=GRID, edgecolor='none', zorder=1, label='input')
    ax.barh(y + h/2, tar_sizes, h, color=TAR, edgecolor='none', zorder=2, alpha=0.6, label='tar+zstd')
    ax.barh(y - h/2, tdg_sizes, h, color=TDG, edgecolor='none', zorder=2, label='tdg')

    xmax = max(input_sizes) * 1.15
    for i in range(len(datasets)):
        ax.text(tdg_sizes[i] + xmax*0.01, y[i] - h/2,
                f'{tdg_sizes[i]:.1f} MB', va='center', fontsize=9,
                color=TDG, fontweight='bold', fontfamily='monospace')
        ax.text(tar_sizes[i] + xmax*0.01, y[i] + h/2,
                f'{tar_sizes[i]:.1f} MB', va='center', fontsize=9,
                color=DIM, fontfamily='monospace')

    ax.set_yticks(y)
    ax.set_yticklabels([DATASET_LABELS[d] for d in datasets], fontsize=9)
    ax.set_xlabel('MB', fontsize=9)
    ax.set_title('SIZE  /  smaller is better', fontsize=11, fontweight='bold', color=TEXT, loc='left', pad=8)
    ax.legend(fontsize=8, loc='lower right', facecolor=PANEL, edgecolor=BORDER, labelcolor=DIM)

    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-size.svg'), format='svg',
                bbox_inches='tight', facecolor=BG)
    plt.close(fig)

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/results.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    os.makedirs(output_dir, exist_ok=True)
    rows = load_csv(csv_path)
    plot_speed(rows, output_dir)
    plot_size(rows, output_dir)
    print(f"Plots saved to {output_dir}/")
