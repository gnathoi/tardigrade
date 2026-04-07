#!/usr/bin/env python3
"""Generate benchmark plots from CSV data."""
import csv
import sys
import os

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker

DATASET_LABELS = {
    'small_files': 'Small Files\n(500 source files)',
    'mixed_dedup': 'Mixed + Duplicates\n(binary, 30 copies)',
    'large_text': 'Large Text\n(10 files, compressible)',
}

COLORS = {
    'tdg': '#4CAF50',
    'tar+zstd': '#78909C',
}

def load_csv(path):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            row['time_ms'] = int(row['time_ms'])
            rows.append(row)
    return rows

def plot_speed(rows, output_dir):
    """Bar chart: create and extract times."""
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 5))
    fig.patch.set_facecolor('#0d1117')

    for ax, op, title in [(ax1, 'create', 'Archive Creation'), (ax2, 'extract', 'Extraction')]:
        ax.set_facecolor('#161b22')
        datasets = list(DATASET_LABELS.keys())
        x = range(len(datasets))
        width = 0.35

        tdg_times = []
        tar_times = []
        for ds in datasets:
            tdg = [r for r in rows if r['tool'] == 'tdg' and r['dataset'] == ds and r['operation'] == op]
            tar = [r for r in rows if r['tool'] == 'tar+zstd' and r['dataset'] == ds and r['operation'] == op]
            tdg_times.append(tdg[0]['time_ms'] if tdg else 0)
            tar_times.append(tar[0]['time_ms'] if tar else 0)

        bars1 = ax.bar([i - width/2 for i in x], tdg_times, width, label='tdg', color=COLORS['tdg'], edgecolor='none')
        bars2 = ax.bar([i + width/2 for i in x], tar_times, width, label='tar+zstd', color=COLORS['tar+zstd'], edgecolor='none')

        # Add value labels
        for bar in bars1:
            ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
                    f'{int(bar.get_height())}ms', ha='center', va='bottom',
                    fontsize=9, color='#c9d1d9', fontweight='bold')
        for bar in bars2:
            ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
                    f'{int(bar.get_height())}ms', ha='center', va='bottom',
                    fontsize=9, color='#c9d1d9')

        ax.set_xlabel('')
        ax.set_ylabel('Time (ms)', color='#8b949e')
        ax.set_title(title, color='#c9d1d9', fontsize=14, fontweight='bold')
        ax.set_xticks(list(x))
        ax.set_xticklabels([DATASET_LABELS[ds] for ds in datasets], fontsize=8, color='#8b949e')
        ax.tick_params(axis='y', colors='#8b949e')
        ax.legend(facecolor='#161b22', edgecolor='#30363d', labelcolor='#c9d1d9')
        ax.spines['top'].set_visible(False)
        ax.spines['right'].set_visible(False)
        ax.spines['bottom'].set_color('#30363d')
        ax.spines['left'].set_color('#30363d')

    fig.suptitle('tardigrade vs tar+zstd — Speed', color='#c9d1d9', fontsize=16, fontweight='bold', y=1.02)
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-speed.svg'), format='svg',
                bbox_inches='tight', facecolor=fig.get_facecolor(), edgecolor='none')
    plt.close(fig)

def plot_size(rows, output_dir):
    """Bar chart: archive sizes and compression ratios."""
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 5))
    fig.patch.set_facecolor('#0d1117')

    datasets = list(DATASET_LABELS.keys())
    x = range(len(datasets))
    width = 0.35

    # Archive size
    ax1.set_facecolor('#161b22')
    tdg_sizes = []
    tar_sizes = []
    input_sizes = []
    for ds in datasets:
        tdg = [r for r in rows if r['tool'] == 'tdg' and r['dataset'] == ds and r['operation'] == 'create']
        tar = [r for r in rows if r['tool'] == 'tar+zstd' and r['dataset'] == ds and r['operation'] == 'create']
        tdg_sizes.append(float(tdg[0]['output_mb']) if tdg and tdg[0]['output_mb'] else 0)
        tar_sizes.append(float(tar[0]['output_mb']) if tar and tar[0]['output_mb'] else 0)
        input_sizes.append(float(tdg[0]['input_mb']) if tdg else 0)

    bars_input = ax1.bar([i for i in x], input_sizes, width*2.2, label='Input', color='#1f6feb', alpha=0.3, edgecolor='none')
    bars1 = ax1.bar([i - width/2 for i in x], tdg_sizes, width, label='tdg', color=COLORS['tdg'], edgecolor='none')
    bars2 = ax1.bar([i + width/2 for i in x], tar_sizes, width, label='tar+zstd', color=COLORS['tar+zstd'], edgecolor='none')

    ax1.set_ylabel('Size (MB)', color='#8b949e')
    ax1.set_title('Archive Size', color='#c9d1d9', fontsize=14, fontweight='bold')
    ax1.set_xticks(list(x))
    ax1.set_xticklabels([DATASET_LABELS[ds] for ds in datasets], fontsize=8, color='#8b949e')
    ax1.tick_params(axis='y', colors='#8b949e')
    ax1.legend(facecolor='#161b22', edgecolor='#30363d', labelcolor='#c9d1d9')
    ax1.spines['top'].set_visible(False)
    ax1.spines['right'].set_visible(False)
    ax1.spines['bottom'].set_color('#30363d')
    ax1.spines['left'].set_color('#30363d')

    # Compression ratio
    ax2.set_facecolor('#161b22')
    tdg_ratios = []
    tar_ratios = []
    for ds in datasets:
        tdg = [r for r in rows if r['tool'] == 'tdg' and r['dataset'] == ds and r['operation'] == 'create']
        tar = [r for r in rows if r['tool'] == 'tar+zstd' and r['dataset'] == ds and r['operation'] == 'create']
        tdg_ratios.append(float(tdg[0]['ratio']) if tdg and tdg[0]['ratio'] else 0)
        tar_ratios.append(float(tar[0]['ratio']) if tar and tar[0]['ratio'] else 0)

    bars1 = ax2.bar([i - width/2 for i in x], tdg_ratios, width, label='tdg', color=COLORS['tdg'], edgecolor='none')
    bars2 = ax2.bar([i + width/2 for i in x], tar_ratios, width, label='tar+zstd', color=COLORS['tar+zstd'], edgecolor='none')

    for bar in bars1:
        if bar.get_height() > 0:
            ax2.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.1,
                    f'{bar.get_height():.1f}x', ha='center', va='bottom',
                    fontsize=9, color='#c9d1d9', fontweight='bold')
    for bar in bars2:
        if bar.get_height() > 0:
            ax2.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.1,
                    f'{bar.get_height():.1f}x', ha='center', va='bottom',
                    fontsize=9, color='#c9d1d9')

    ax2.set_ylabel('Compression Ratio', color='#8b949e')
    ax2.set_title('Compression Ratio (higher is better)', color='#c9d1d9', fontsize=14, fontweight='bold')
    ax2.set_xticks(list(x))
    ax2.set_xticklabels([DATASET_LABELS[ds] for ds in datasets], fontsize=8, color='#8b949e')
    ax2.tick_params(axis='y', colors='#8b949e')
    ax2.legend(facecolor='#161b22', edgecolor='#30363d', labelcolor='#c9d1d9')
    ax2.spines['top'].set_visible(False)
    ax2.spines['right'].set_visible(False)
    ax2.spines['bottom'].set_color('#30363d')
    ax2.spines['left'].set_color('#30363d')

    fig.suptitle('tardigrade vs tar+zstd — Size & Compression', color='#c9d1d9', fontsize=16, fontweight='bold', y=1.02)
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-size.svg'), format='svg',
                bbox_inches='tight', facecolor=fig.get_facecolor(), edgecolor='none')
    plt.close(fig)

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/results.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    os.makedirs(output_dir, exist_ok=True)

    rows = load_csv(csv_path)
    plot_speed(rows, output_dir)
    plot_size(rows, output_dir)
    print(f"Plots saved to {output_dir}/bench-speed.svg and {output_dir}/bench-size.svg")
