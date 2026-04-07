use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use console::style;
use humansize::{BINARY, format_size};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Progress tracking for archive creation
pub struct CreateProgress {
    multi: MultiProgress,
    scan_bar: ProgressBar,
    compress_bar: ProgressBar,
    pub stats: Arc<ProgressStats>,
}

pub struct ProgressStats {
    pub files_scanned: AtomicU64,
    pub bytes_scanned: AtomicU64,
    pub bytes_compressed: AtomicU64,
    pub bytes_written: AtomicU64,
    pub dedup_savings: AtomicU64,
    pub blocks_total: AtomicU64,
    pub blocks_deduped: AtomicU64,
}

impl ProgressStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            files_scanned: AtomicU64::new(0),
            bytes_scanned: AtomicU64::new(0),
            bytes_compressed: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            dedup_savings: AtomicU64::new(0),
            blocks_total: AtomicU64::new(0),
            blocks_deduped: AtomicU64::new(0),
        })
    }
}

impl CreateProgress {
    pub fn new(total_bytes: u64) -> Self {
        let multi = MultiProgress::new();

        let scan_style = ProgressStyle::with_template("  {spinner:.cyan} Scanning...  {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");

        let scan_bar = multi.add(ProgressBar::new_spinner());
        scan_bar.set_style(scan_style);
        scan_bar.enable_steady_tick(std::time::Duration::from_millis(80));

        let compress_style = ProgressStyle::with_template(
            "  {bar:30.cyan/dim} {percent:>3}%  {binary_bytes_per_sec}  ETA {eta}  {msg}",
        )
        .unwrap()
        .progress_chars("━╸─");

        let compress_bar = multi.add(ProgressBar::new(total_bytes));
        compress_bar.set_style(compress_style);

        let stats = ProgressStats::new();

        Self {
            multi,
            scan_bar,
            compress_bar,
            stats,
        }
    }

    pub fn set_scan_msg(&self, files: u64, bytes: u64) {
        self.scan_bar.set_message(format!(
            "{} files, {}",
            style(files).bold(),
            style(format_size(bytes, BINARY)).cyan()
        ));
    }

    pub fn finish_scan(&self) {
        let files = self.stats.files_scanned.load(Ordering::Relaxed);
        let bytes = self.stats.bytes_scanned.load(Ordering::Relaxed);
        self.scan_bar.finish_with_message(format!(
            "{} files, {}",
            style(files).bold(),
            style(format_size(bytes, BINARY)).cyan()
        ));
    }

    pub fn inc_compressed(&self, bytes: u64) {
        self.compress_bar.inc(bytes);

        let input = self.stats.bytes_scanned.load(Ordering::Relaxed);
        let written = self.stats.bytes_written.load(Ordering::Relaxed);
        let dedup = self.stats.dedup_savings.load(Ordering::Relaxed);

        let ratio = if written > 0 {
            input as f64 / written as f64
        } else {
            0.0
        };

        let mut msg = format!("Ratio: {:.1}x", ratio,);

        if dedup > 0 {
            msg.push_str(&format!("  Dedup: {} saved", format_size(dedup, BINARY)));
        }

        self.compress_bar.set_message(msg);
    }

    pub fn finish(&self) {
        self.compress_bar.finish_and_clear();
        self.scan_bar.finish_and_clear();
    }
}

/// Progress tracking for extraction
pub struct ExtractProgress {
    bar: ProgressBar,
}

impl ExtractProgress {
    pub fn new(total_files: u64) -> Self {
        let style = ProgressStyle::with_template(
            "  {bar:30.green/dim} {percent:>3}%  {pos}/{len} files  {msg}",
        )
        .unwrap()
        .progress_chars("━╸─");

        let bar = ProgressBar::new(total_files);
        bar.set_style(style);

        Self { bar }
    }

    pub fn inc(&self) {
        self.bar.inc(1);
    }

    pub fn set_message(&self, msg: impl Into<std::borrow::Cow<'static, str>>) {
        self.bar.set_message(msg);
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
    }
}
