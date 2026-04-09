#!/usr/bin/env python3
"""Generate core scaling plot — CERN/scientific style on dark background."""
import csv
import sys
import os

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np
from scipy.optimize import curve_fit

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
    threads, times, throughputs = [], [], []
    with open(path) as f:
        for row in csv.DictReader(f):
            threads.append(int(row['threads']))
            times.append(int(row['time_ms']))
            throughputs.append(float(row['throughput_mbs']))
    return threads, times, throughputs

def amdahl_fit(threads, throughputs):
    """Least-squares fit of Amdahl's law + overhead: tp(n) = t1 / (s + (1-s)/n + c*n).
    The c*n term models per-thread overhead (contention, cache pressure)."""
    threads_a = np.array(threads, dtype=float)
    tp_a = np.array(throughputs, dtype=float)

    def model(n, t1, s, c):
        return t1 / (s + (1 - s) / n + c * n)

    t1_init = tp_a[0]
    ratio = t1_init / tp_a[-1]
    s_init = max(0.05, (ratio - 1.0/threads_a[-1]) / (1.0 - 1.0/threads_a[-1]))

    popt, _ = curve_fit(model, threads_a, tp_a, p0=[t1_init, s_init, 1e-4],
                        bounds=([0, 0.001, 0], [np.inf, 0.999, 1.0]))
    return popt[1], popt[0], popt[2]  # s, t1, c

def setup_ax(ax):
    ax.set_facecolor(PANEL)
    for spine in ax.spines.values():
        spine.set_color(BORDER)
    ax.tick_params(colors=DIM, length=3, width=0.5)
    ax.grid(color=GRID, linewidth=0.5, zorder=0)

def plot(csv_path, output_dir):
    threads, times, throughputs = load_csv(csv_path)
    s, t1, c = amdahl_fit(threads, throughputs)
    max_measured = max(threads)

    # Fit curve over measured range only
    fit_threads = np.linspace(1, max_measured, 200)
    fit_tp = [t1 / (s + (1-s)/n + c*n) for n in fit_threads]

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 4.2))
    fig.patch.set_facecolor(BG)

    # --- Throughput ---
    setup_ax(ax1)
    linear_threads = np.linspace(1, max_measured, 200)
    ax1.plot(linear_threads, [t1*n for n in linear_threads], '--',
             color=LINEAR, linewidth=1, label='linear')
    ax1.plot(fit_threads, fit_tp, '-', color=EXTRAP, linewidth=1.5, alpha=0.6,
             label=f"amdahl+overhead (s={s:.0%})")
    ax1.scatter(threads, throughputs, color=TDG, s=30, zorder=5,
                edgecolors=PANEL, linewidths=1.5, label='measured')

    ax1.set_xlabel('threads', fontsize=9)
    ax1.set_ylabel('MB/s', fontsize=9)
    ax1.set_title('THROUGHPUT', fontsize=11, fontweight='bold', color=TEXT, loc='left', pad=8)
    ax1.set_xlim(0, max_measured + 2)
    ax1.set_ylim(0, max(throughputs) * 1.15)

    # --- Speedup ---
    setup_ax(ax2)
    measured_speedup = [t / throughputs[0] for t in throughputs]
    fit_speedup = [tp / t1 for tp in fit_tp]

    ax2.plot(linear_threads, linear_threads, '--', color=LINEAR, linewidth=1, label='linear')
    ax2.plot(fit_threads, fit_speedup, '-', color=EXTRAP, linewidth=1.5, alpha=0.6, label='amdahl+overhead')
    ax2.scatter(threads, measured_speedup, color=TDG, s=30, zorder=5,
                edgecolors=PANEL, linewidths=1.5, label='measured')

    ax2.set_xlabel('threads', fontsize=9)
    ax2.set_ylabel('speedup', fontsize=9)
    ax2.set_title('SPEEDUP', fontsize=11, fontweight='bold', color=TEXT, loc='left', pad=8)
    ax2.set_xlim(0, max_measured + 2)
    ax2.set_ylim(0, max(max(measured_speedup), max(fit_speedup)) * 1.15)

    # Single shared legend below the figure
    handles, labels = ax1.get_legend_handles_labels()
    fig.legend(handles, labels, loc='lower center', ncol=3, fontsize=8,
               facecolor=PANEL, edgecolor=BORDER, labelcolor=DIM)

    meta = load_meta(output_dir)
    subtitle = 'tdg  /  core scaling'
    if meta.get('version'):
        subtitle += f'  [{meta["version"]}]'
    fig.suptitle(subtitle, fontsize=10, color=DIM, fontfamily='monospace', y=0.98)
    fig.tight_layout(rect=[0, 0.06, 1, 0.95])
    fig.savefig(os.path.join(output_dir, 'bench-scaling.svg'), format='svg',
                bbox_inches='tight', facecolor=BG)
    plt.close(fig)

    print(f"Scaling plot saved to {output_dir}/bench-scaling.svg")
    peak_n = int(fit_threads[np.argmax(fit_tp)])
    peak_tp = max(fit_tp)
    print(f"Serial fraction: {s:.1%}, overhead: {c:.2e}/thread, peak {peak_tp:.0f} MB/s @ {peak_n} threads")

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/scaling.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    plot(csv_path, output_dir)
