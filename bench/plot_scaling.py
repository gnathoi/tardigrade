#!/usr/bin/env python3
"""Generate core scaling plot with extrapolation."""
import csv
import sys
import os
import math

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
    """Fit Amdahl's law: T(n) = T1 * (s + (1-s)/n)
    where s is the serial fraction.
    Throughput(n) = T1_throughput / (s + (1-s)/n)
    """
    t1 = throughputs[0]  # single-thread throughput
    # Simple fit: use the last data point to estimate s
    n_max = threads[-1]
    tp_max = throughputs[-1]
    # t1/tp_max = s + (1-s)/n_max
    # t1/tp_max - 1/n_max = s * (1 - 1/n_max)
    ratio = t1 / tp_max
    s = (ratio - 1.0/n_max) / (1.0 - 1.0/n_max)
    s = max(0.01, min(0.99, s))  # clamp
    return s, t1

def plot(csv_path, output_dir):
    threads, times, throughputs = load_csv(csv_path)
    s, t1 = amdahl_fit(threads, throughputs)
    max_measured = max(threads)

    # Extrapolate to 64 cores
    extrap_threads = list(range(1, 65))
    extrap_throughput = [t1 / (s + (1-s)/n) for n in extrap_threads]

    # Perfect linear scaling reference
    linear_throughput = [t1 * n for n in extrap_threads]

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(14, 5.5))
    fig.patch.set_facecolor('#0d1117')

    # --- Left: Throughput vs cores ---
    ax1.set_facecolor('#161b22')

    # Extrapolated (Amdahl's law)
    ax1.plot(extrap_threads, extrap_throughput, '-', color='#388E3C', linewidth=1.5,
             alpha=0.5, label=f"Amdahl's law (serial={s:.0%})")
    # Perfect linear
    ax1.plot(extrap_threads[:32], [t1*n for n in extrap_threads[:32]], '--',
             color='#1f6feb', linewidth=1, alpha=0.3, label='Perfect linear scaling')
    # Measured points
    ax1.plot(threads, throughputs, 'o-', color='#4CAF50', linewidth=2.5,
             markersize=8, markeredgecolor='white', markeredgewidth=1.5,
             label='Measured', zorder=5)
    # Mark extrapolation zone
    ax1.axvline(x=max_measured, color='#f0883e', linestyle=':', alpha=0.5, linewidth=1)
    ax1.text(max_measured + 1, max(throughputs) * 0.5, 'extrapolated  →',
             color='#f0883e', fontsize=9, alpha=0.7)

    # Annotate key points
    for i in [0, -1]:
        ax1.annotate(f'{throughputs[i]:.0f} MB/s',
                     (threads[i], throughputs[i]),
                     textcoords="offset points", xytext=(10, 10),
                     fontsize=9, color='#c9d1d9', fontweight='bold')

    # Extrapolated points of interest
    for cores in [16, 32, 64]:
        if cores > max_measured:
            tp = t1 / (s + (1-s)/cores)
            ax1.plot(cores, tp, 's', color='#f0883e', markersize=6, alpha=0.7, zorder=4)
            ax1.annotate(f'{tp:.0f} MB/s\n({cores} cores)',
                        (cores, tp), textcoords="offset points",
                        xytext=(8, -15 if cores == 32 else 8),
                        fontsize=8, color='#f0883e', alpha=0.8)

    ax1.set_xlabel('Threads', color='#8b949e', fontsize=11)
    ax1.set_ylabel('Throughput (MB/s)', color='#8b949e', fontsize=11)
    ax1.set_title('Throughput Scaling', color='#c9d1d9', fontsize=14, fontweight='bold')
    ax1.tick_params(colors='#8b949e')
    ax1.legend(facecolor='#161b22', edgecolor='#30363d', labelcolor='#c9d1d9', fontsize=9)
    ax1.spines['top'].set_visible(False)
    ax1.spines['right'].set_visible(False)
    ax1.spines['bottom'].set_color('#30363d')
    ax1.spines['left'].set_color('#30363d')
    ax1.set_xlim(0, 66)
    ax1.set_ylim(0, max(extrap_throughput[:64]) * 1.15)

    # --- Right: Speedup vs cores ---
    ax2.set_facecolor('#161b22')

    measured_speedup = [throughputs[i] / throughputs[0] for i in range(len(threads))]
    extrap_speedup = [tp / t1 for tp in extrap_throughput]
    linear_speedup = extrap_threads

    ax2.plot(extrap_threads, extrap_speedup, '-', color='#388E3C', linewidth=1.5,
             alpha=0.5, label=f"Amdahl's law")
    ax2.plot(extrap_threads, linear_speedup, '--', color='#1f6feb', linewidth=1,
             alpha=0.3, label='Perfect linear')
    ax2.plot(threads, measured_speedup, 'o-', color='#4CAF50', linewidth=2.5,
             markersize=8, markeredgecolor='white', markeredgewidth=1.5,
             label='Measured', zorder=5)
    ax2.axvline(x=max_measured, color='#f0883e', linestyle=':', alpha=0.5, linewidth=1)

    ax2.set_xlabel('Threads', color='#8b949e', fontsize=11)
    ax2.set_ylabel('Speedup vs 1 thread', color='#8b949e', fontsize=11)
    ax2.set_title('Speedup Scaling', color='#c9d1d9', fontsize=14, fontweight='bold')
    ax2.tick_params(colors='#8b949e')
    ax2.legend(facecolor='#161b22', edgecolor='#30363d', labelcolor='#c9d1d9', fontsize=9)
    ax2.spines['top'].set_visible(False)
    ax2.spines['right'].set_visible(False)
    ax2.spines['bottom'].set_color('#30363d')
    ax2.spines['left'].set_color('#30363d')
    ax2.set_xlim(0, 66)
    ax2.set_ylim(0, max(extrap_speedup[:64]) * 1.15)

    fig.suptitle('tardigrade — Core Scaling (measured + extrapolated)',
                 color='#c9d1d9', fontsize=16, fontweight='bold', y=1.02)
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, 'bench-scaling.svg'), format='svg',
                bbox_inches='tight', facecolor=fig.get_facecolor(), edgecolor='none')
    plt.close(fig)
    print(f"Scaling plot saved to {output_dir}/bench-scaling.svg")
    print(f"Amdahl serial fraction: {s:.1%}")
    print(f"Predicted throughput at 32 cores: {t1/(s+(1-s)/32):.0f} MB/s")
    print(f"Predicted throughput at 64 cores: {t1/(s+(1-s)/64):.0f} MB/s")

if __name__ == '__main__':
    csv_path = sys.argv[1] if len(sys.argv) > 1 else 'bench/scaling.csv'
    output_dir = sys.argv[2] if len(sys.argv) > 2 else 'bench'
    plot(csv_path, output_dir)
