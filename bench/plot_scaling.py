#!/usr/bin/env python3
"""Generate core scaling plot with Amdahl's law extrapolation."""
import csv
import sys
import os

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

def load_csv(path):
    threads, times, throughputs = [], [], []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            threads.append(int(row['threads']))
            times.append(int(row['time_ms']))
            throughputs.append(float(row['throughput_mbs']))
    return threads, times, throughputs

def amdahl_fit(threads, throughputs):
    t1 = throughputs[0]
    n_max = threads[-1]
    tp_max = throughputs[-1]
    ratio = t1 / tp_max
    s = (ratio - 1.0/n_max) / (1.0 - 1.0/n_max)
    s = max(0.01, min(0.99, s))
    return s, t1

TDG_COLOR = '#22c55e'
EXTRAP_COLOR = '#86efac'
LINEAR_COLOR = '#94a3b8'
CALLOUT_COLOR = '#f59e0b'

def setup_ax(ax):
    ax.set_facecolor('none')
    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.spines['bottom'].set_color('#666')
    ax.spines['left'].set_color('#666')
    ax.tick_params(colors='#666')

def plot(csv_path, output_dir):
    threads, times, throughputs = load_csv(csv_path)
    s, t1 = amdahl_fit(threads, throughputs)
    max_measured = max(threads)

    extrap_threads = list(range(1, 65))
    extrap_tp = [t1 / (s + (1-s)/n) for n in extrap_threads]

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(11, 4.5))
    fig.patch.set_alpha(0)

    # --- Throughput ---
    setup_ax(ax1)
    ax1.plot(extrap_threads, extrap_tp, '-', color=EXTRAP_COLOR, linewidth=1.5,
             alpha=0.6, label=f"Amdahl's law (serial={s:.0%})")
    ax1.plot(extrap_threads[:32], [t1*n for n in extrap_threads[:32]], '--',
             color=LINEAR_COLOR, linewidth=1, alpha=0.3, label='Perfect linear')
    ax1.plot(threads, throughputs, 'o-', color=TDG_COLOR, linewidth=2.5,
             markersize=7, markeredgecolor='white', markeredgewidth=1.5,
             label='Measured', zorder=5)

    ax1.axvline(x=max_measured, color=CALLOUT_COLOR, linestyle=':', alpha=0.4)
    ax1.text(max_measured + 1, max(throughputs) * 0.4, 'extrapolated  \u2192',
             color=CALLOUT_COLOR, fontsize=8, alpha=0.7)

    # Annotate endpoints
    ax1.annotate(f'{throughputs[0]:.0f} MB/s', (threads[0], throughputs[0]),
                 textcoords="offset points", xytext=(12, -5), fontsize=9,
                 color='#333', fontweight='bold')
    ax1.annotate(f'{throughputs[-1]:.0f} MB/s', (threads[-1], throughputs[-1]),
                 textcoords="offset points", xytext=(8, 8), fontsize=9,
                 color='#333', fontweight='bold')

    for cores in [32, 64]:
        tp = t1 / (s + (1-s)/cores)
        ax1.plot(cores, tp, 's', color=CALLOUT_COLOR, markersize=5, alpha=0.7, zorder=4)
        ax1.annotate(f'{tp:.0f} MB/s\n({cores} cores)', (cores, tp),
                     textcoords="offset points", xytext=(8, -12),
                     fontsize=8, color=CALLOUT_COLOR, alpha=0.8)

    ax1.set_xlabel('Threads', color='#555')
    ax1.set_ylabel('Throughput (MB/s)', color='#555')
    ax1.set_title('Throughput Scaling', fontsize=13, fontweight='bold', color='#333')
    ax1.legend(fontsize=8, framealpha=0.5)
    ax1.set_xlim(0, 66)
    ax1.set_ylim(0, max(extrap_tp[:64]) * 1.15)
    ax1.grid(alpha=0.15, zorder=0)

    # --- Speedup ---
    setup_ax(ax2)
    measured_speedup = [t / throughputs[0] for t in throughputs]
    extrap_speedup = [tp / t1 for tp in extrap_tp]

    ax2.plot(extrap_threads, extrap_speedup, '-', color=EXTRAP_COLOR, linewidth=1.5, alpha=0.6, label="Amdahl's law")
    ax2.plot(extrap_threads, extrap_threads, '--', color=LINEAR_COLOR, linewidth=1, alpha=0.3, label='Perfect linear')
    ax2.plot(threads, measured_speedup, 'o-', color=TDG_COLOR, linewidth=2.5,
             markersize=7, markeredgecolor='white', markeredgewidth=1.5,
             label='Measured', zorder=5)
    ax2.axvline(x=max_measured, color=CALLOUT_COLOR, linestyle=':', alpha=0.4)

    ax2.set_xlabel('Threads', color='#555')
    ax2.set_ylabel('Speedup vs 1 thread', color='#555')
    ax2.set_title('Parallel Speedup', fontsize=13, fontweight='bold', color='#333')
    ax2.legend(fontsize=8, framealpha=0.5)
    ax2.set_xlim(0, 66)
    ax2.set_ylim(0, max(extrap_speedup[:64]) * 1.15)
    ax2.grid(alpha=0.15, zorder=0)

    fig.suptitle('tardigrade — Core Scaling (measured + extrapolated)',
                 fontsize=14, fontweight='bold', color='#333')
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-scaling.svg'), format='svg',
                bbox_inches='tight', transparent=True)
    plt.close(fig)

    print(f"Scaling plot saved to {output_dir}/bench-scaling.svg")
    print(f"Serial fraction: {s:.1%}, predicted {t1/(s+(1-s)/32):.0f} MB/s @ 32 cores")

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/scaling.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    plot(csv_path, output_dir)
