#!/usr/bin/env python3
"""Generate core scaling plot — CERN/scientific style on dark background."""
import csv
import sys
import os

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

BG = '#0e1117'
PANEL = '#161b22'
GRID = '#21262d'
BORDER = '#30363d'
TEXT = '#e6edf3'
DIM = '#8b949e'
TDG = '#3fb950'
EXTRAP = '#238636'
LINEAR = '#30363d'
CALLOUT = '#d29922'

plt.rcParams.update({
    'font.family': 'monospace',
    'font.size': 9,
    'text.color': TEXT,
    'axes.labelcolor': DIM,
    'xtick.color': DIM,
    'ytick.color': DIM,
})

def load_csv(path):
    threads, times, throughputs = [], [], []
    with open(path) as f:
        for row in csv.DictReader(f):
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
    return max(0.01, min(0.99, s)), t1

def setup_ax(ax):
    ax.set_facecolor(PANEL)
    for spine in ax.spines.values():
        spine.set_color(BORDER)
    ax.tick_params(colors=DIM, length=3, width=0.5)
    ax.grid(color=GRID, linewidth=0.5, zorder=0)

def plot(csv_path, output_dir):
    threads, times, throughputs = load_csv(csv_path)
    s, t1 = amdahl_fit(threads, throughputs)
    max_measured = max(threads)

    extrap_threads = list(range(1, 65))
    extrap_tp = [t1 / (s + (1-s)/n) for n in extrap_threads]

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 4.2))
    fig.patch.set_facecolor(BG)

    # --- Throughput ---
    setup_ax(ax1)
    ax1.fill_between(extrap_threads, 0, extrap_tp, color=EXTRAP, alpha=0.08, zorder=1)
    ax1.plot(extrap_threads, extrap_tp, '-', color=EXTRAP, linewidth=1.5, alpha=0.6,
             label=f"amdahl (serial={s:.0%})")
    ax1.plot(extrap_threads[:32], [t1*n for n in extrap_threads[:32]], '--',
             color=LINEAR, linewidth=1, label='linear')
    ax1.plot(threads, throughputs, 'o-', color=TDG, linewidth=2,
             markersize=6, markeredgecolor=PANEL, markeredgewidth=1.5,
             label='measured', zorder=5)

    ax1.axvline(x=max_measured, color=CALLOUT, linestyle=':', alpha=0.4, linewidth=0.8)

    ax1.annotate(f'{throughputs[0]:.0f}', (threads[0], throughputs[0]),
                 textcoords="offset points", xytext=(10, -3), fontsize=9,
                 color=TDG, fontweight='bold')
    ax1.annotate(f'{throughputs[-1]:.0f}', (threads[-1], throughputs[-1]),
                 textcoords="offset points", xytext=(8, 6), fontsize=9,
                 color=TDG, fontweight='bold')

    for cores in [32, 64]:
        tp = t1 / (s + (1-s)/cores)
        ax1.plot(cores, tp, 's', color=CALLOUT, markersize=4, zorder=4)
        ax1.annotate(f'{tp:.0f} MB/s @ {cores}c', (cores, tp),
                     textcoords="offset points", xytext=(6, -12),
                     fontsize=8, color=CALLOUT)

    ax1.set_xlabel('threads', fontsize=9)
    ax1.set_ylabel('MB/s', fontsize=9)
    ax1.set_title('THROUGHPUT', fontsize=11, fontweight='bold', color=TEXT, loc='left', pad=8)
    ax1.legend(fontsize=8, facecolor=PANEL, edgecolor=BORDER, labelcolor=DIM)
    ax1.set_xlim(0, 66)
    ax1.set_ylim(0, max(extrap_tp[:64]) * 1.15)

    # --- Speedup ---
    setup_ax(ax2)
    measured_speedup = [t / throughputs[0] for t in throughputs]
    extrap_speedup = [tp / t1 for tp in extrap_tp]

    ax2.fill_between(extrap_threads, 0, extrap_speedup, color=EXTRAP, alpha=0.08, zorder=1)
    ax2.plot(extrap_threads, extrap_speedup, '-', color=EXTRAP, linewidth=1.5, alpha=0.6, label="amdahl")
    ax2.plot(extrap_threads, extrap_threads, '--', color=LINEAR, linewidth=1, label='linear')
    ax2.plot(threads, measured_speedup, 'o-', color=TDG, linewidth=2,
             markersize=6, markeredgecolor=PANEL, markeredgewidth=1.5,
             label='measured', zorder=5)
    ax2.axvline(x=max_measured, color=CALLOUT, linestyle=':', alpha=0.4, linewidth=0.8)

    ax2.set_xlabel('threads', fontsize=9)
    ax2.set_ylabel('speedup', fontsize=9)
    ax2.set_title('SPEEDUP', fontsize=11, fontweight='bold', color=TEXT, loc='left', pad=8)
    ax2.legend(fontsize=8, facecolor=PANEL, edgecolor=BORDER, labelcolor=DIM)
    ax2.set_xlim(0, 66)
    ax2.set_ylim(0, max(extrap_speedup[:64]) * 1.15)

    fig.suptitle(f'tdg  /  core scaling  /  measured + extrapolated',
                 fontsize=10, color=DIM, fontfamily='monospace', y=0.98)
    fig.tight_layout(rect=[0, 0, 1, 0.95])
    fig.savefig(os.path.join(output_dir, 'bench-scaling.svg'), format='svg',
                bbox_inches='tight', facecolor=BG)
    plt.close(fig)

    print(f"Scaling plot saved to {output_dir}/bench-scaling.svg")
    print(f"Serial fraction: {s:.1%}, predicted {t1/(s+(1-s)/32):.0f} MB/s @ 32 cores")

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/scaling.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    plot(csv_path, output_dir)
