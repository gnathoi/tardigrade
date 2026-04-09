use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use console::style;
use humansize::{BINARY, format_size};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

pub struct ProgressStats {
    pub files_scanned: AtomicU64,
    pub bytes_scanned: AtomicU64,
    pub bytes_processed: AtomicU64,
    pub bytes_written: AtomicU64,
    pub dedup_savings: AtomicU64,
    pub blocks_deduped: AtomicU64,
}

impl ProgressStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            files_scanned: AtomicU64::new(0),
            bytes_scanned: AtomicU64::new(0),
            bytes_processed: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            dedup_savings: AtomicU64::new(0),
            blocks_deduped: AtomicU64::new(0),
        })
    }
}

/// Progress display for archive creation
pub struct CreateProgress {
    _multi: MultiProgress,
    main_bar: ProgressBar,
    status_bar: ProgressBar,
    pub stats: Arc<ProgressStats>,
    start: Instant,
    total_bytes: u64,
}

impl CreateProgress {
    pub fn new(total_bytes: u64) -> Self {
        let multi = MultiProgress::new();
        let stats = ProgressStats::new();

        // Main progress bar — uses a custom template without indicatif's ETA
        // (we compute our own stable linear ETA instead)
        let main_style = ProgressStyle::with_template(
            "  {bar:40.green/dark_gray} {percent:>3}%  {binary_bytes_per_sec:>12}  {elapsed_precise}  {msg}",
        )
        .unwrap()
        .progress_chars("━━╸");

        let main_bar = multi.add(ProgressBar::new(total_bytes));
        main_bar.set_style(main_style);
        main_bar.enable_steady_tick(Duration::from_millis(80));

        // Status line below — shows live compression stats
        let status_style = ProgressStyle::with_template("  {msg}").unwrap();

        let status_bar = multi.add(ProgressBar::new_spinner());
        status_bar.set_style(status_style);
        status_bar.enable_steady_tick(Duration::from_millis(200));

        Self {
            _multi: multi,
            main_bar,
            status_bar,
            stats,
            start: Instant::now(),
            total_bytes,
        }
    }

    /// Called from rayon threads as each file is read + compressed.
    /// This is where most wall-clock time is spent, so this drives the progress bar.
    pub fn inc_processed(&self, input_bytes: u64) {
        self.stats
            .bytes_processed
            .fetch_add(input_bytes, Ordering::Relaxed);
        self.main_bar.inc(input_bytes);
        self.update_eta();
    }

    /// Stable linear ETA: elapsed / fraction_done * fraction_remaining.
    /// No exponential smoothing, no wild oscillation.
    fn update_eta(&self) {
        let processed = self.stats.bytes_processed.load(Ordering::Relaxed);
        if processed == 0 {
            return;
        }
        let elapsed = self.start.elapsed();
        let fraction = processed as f64 / self.total_bytes.max(1) as f64;
        let remaining_secs = if fraction > 0.0 {
            elapsed.as_secs_f64() / fraction * (1.0 - fraction)
        } else {
            0.0
        };

        let eta = if remaining_secs < 60.0 {
            format!("ETA {:.0}s", remaining_secs)
        } else if remaining_secs < 3600.0 {
            format!(
                "ETA {}m{:02}s",
                remaining_secs as u64 / 60,
                remaining_secs as u64 % 60
            )
        } else {
            let h = remaining_secs as u64 / 3600;
            let m = (remaining_secs as u64 % 3600) / 60;
            format!("ETA {h}h{m:02}m")
        };
        self.main_bar.set_message(eta);
    }

    pub fn finish_scan(&self) {
        let files = self.stats.files_scanned.load(Ordering::Relaxed);
        let bytes = self.stats.bytes_scanned.load(Ordering::Relaxed);
        self.status_bar.set_message(format!(
            "{} {} files, {}",
            style("○").dim(),
            style(files).white().bold(),
            style(format_size(bytes, BINARY)).dim(),
        ));
    }

    /// Update the status line with compression ratio and dedup stats.
    /// Called during the write phase as blocks are flushed to disk.
    pub fn inc_compressed(&self, _bytes: u64) {
        let input = self.stats.bytes_scanned.load(Ordering::Relaxed);
        let written = self.stats.bytes_written.load(Ordering::Relaxed);
        let dedup = self.stats.dedup_savings.load(Ordering::Relaxed);

        let ratio = if written > 0 {
            input as f64 / written as f64
        } else {
            0.0
        };

        let mut parts = vec![format!(
            "{} ratio: {}",
            style("○").dim(),
            style(format!("{:.1}x", ratio)).cyan().bold(),
        )];

        if dedup > 0 {
            parts.push(format!(
                "dedup: {}",
                style(format!("-{}", format_size(dedup, BINARY)))
                    .green()
                    .bold(),
            ));
        }

        self.status_bar.set_message(parts.join("  "));
    }

    pub fn finish(&self) {
        self.main_bar.finish_and_clear();
        self.status_bar.finish_and_clear();
    }
}
