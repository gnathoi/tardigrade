use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use console::style;
use humansize::{BINARY, format_size};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

pub struct ProgressStats {
    pub files_scanned: AtomicU64,
    pub bytes_scanned: AtomicU64,
    pub bytes_written: AtomicU64,
    pub dedup_savings: AtomicU64,
    pub blocks_deduped: AtomicU64,
}

impl ProgressStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            files_scanned: AtomicU64::new(0),
            bytes_scanned: AtomicU64::new(0),
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
}

impl CreateProgress {
    pub fn new(total_bytes: u64) -> Self {
        let multi = MultiProgress::new();
        let stats = ProgressStats::new();

        // Main progress bar — the one that fills up
        let main_style = ProgressStyle::with_template(
            "  {bar:40.green/dark_gray} {percent:>3}%  {binary_bytes_per_sec:>12}  ETA {eta_precise}",
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
        }
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

    pub fn inc_compressed(&self, bytes: u64) {
        self.main_bar.inc(bytes);

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
