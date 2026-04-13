use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use console::style;
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
    finishing: Arc<AtomicBool>,
}

impl CreateProgress {
    pub fn new(total_bytes: u64) -> Self {
        let multi = MultiProgress::new();
        let stats = ProgressStats::new();

        // Main progress bar — no throughput (shown in final summary instead)
        let main_style = ProgressStyle::with_template(
            "  {bar:40.green/dark_gray} {percent:>3}%  {elapsed_precise}  {msg}",
        )
        .unwrap()
        .progress_chars("━━╸");

        let main_bar = multi.add(ProgressBar::new(total_bytes));
        main_bar.set_style(main_style);
        main_bar.enable_steady_tick(Duration::from_millis(80));

        // Status line — uses {spinner} which animates via steady_tick automatically
        let status_style = ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]);

        let status_bar = multi.add(ProgressBar::new_spinner());
        status_bar.set_style(status_style);
        status_bar.set_message(format!("{}", style("compressing…").dim()));
        status_bar.enable_steady_tick(Duration::from_millis(80));

        Self {
            _multi: multi,
            main_bar,
            status_bar,
            stats,
            start: Instant::now(),
            total_bytes,
            finishing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Called from rayon threads as each file is read + compressed.
    pub fn inc_processed(&self, input_bytes: u64) {
        self.stats
            .bytes_processed
            .fetch_add(input_bytes, Ordering::Relaxed);
        self.main_bar.inc(input_bytes);
        self.update_eta();
    }

    /// Stable linear ETA: elapsed / fraction_done * fraction_remaining.
    fn update_eta(&self) {
        if self.finishing.load(Ordering::Relaxed) {
            return;
        }

        let processed = self.stats.bytes_processed.load(Ordering::Relaxed);
        if processed == 0 {
            return;
        }
        let elapsed = self.start.elapsed();
        let fraction = processed as f64 / self.total_bytes.max(1) as f64;

        // Before 10%: no ETA yet (too unreliable), bar + spinner are enough
        if fraction < 0.10 {
            return;
        }

        let remaining_secs = if fraction > 0.0 {
            elapsed.as_secs_f64() / fraction * (1.0 - fraction)
        } else {
            0.0
        };

        // Seconds only below 1min, nearest minute otherwise
        let eta = if remaining_secs < 60.0 {
            format!("ETA {:.0}s", remaining_secs)
        } else if remaining_secs < 3600.0 {
            let mins = (remaining_secs / 60.0).round() as u64;
            format!("ETA ~{}m", mins)
        } else {
            let h = remaining_secs as u64 / 3600;
            let m = ((remaining_secs as u64 % 3600) as f64 / 60.0).round() as u64;
            format!("ETA ~{h}h{m:02}m")
        };
        self.main_bar.set_message(eta);
    }

    pub fn finish_scan(&self) {
        // no-op: scan stats no longer shown during progress
    }

    /// Update the status line. The spinner animates automatically via steady_tick.
    pub fn inc_compressed(&self, _bytes: u64) {
        // Status bar spinner + message animate automatically, nothing to do here
    }

    /// Transition to "finishing" state — replace bar + ETA with a pulsing animation
    pub fn start_finishing(&self) {
        self.finishing.store(true, Ordering::Relaxed);

        // Switch main bar to a spinner style for the finishing phase
        let finish_style = ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]);
        self.main_bar.set_style(finish_style);
        self.main_bar
            .set_message(format!("{}", style("finishing up…").dim()));
        self.status_bar.set_message(String::new());
    }

    pub fn finish(&self) {
        self.finishing.store(false, Ordering::Relaxed);
        self.main_bar.finish_and_clear();
        self.status_bar.finish_and_clear();
    }
}

/// Progress display for archive extraction. Mirrors CreateProgress visually
/// so the two commands feel consistent: main progress bar + spinner status
/// line, steady_tick animation, linear ETA after 10% complete, and a
/// pulsing "finishing up…" phase for post-file metadata work.
pub struct ExtractProgress {
    _multi: MultiProgress,
    main_bar: ProgressBar,
    status_bar: ProgressBar,
    start: Instant,
    total_bytes: u64,
    bytes_extracted: AtomicU64,
    finishing: Arc<AtomicBool>,
}

impl ExtractProgress {
    pub fn new(total_bytes: u64) -> Self {
        let multi = MultiProgress::new();

        let main_style = ProgressStyle::with_template(
            "  {bar:40.green/dark_gray} {percent:>3}%  {elapsed_precise}  {msg}",
        )
        .unwrap()
        .progress_chars("━━╸");

        let main_bar = multi.add(ProgressBar::new(total_bytes.max(1)));
        main_bar.set_style(main_style);
        main_bar.enable_steady_tick(Duration::from_millis(80));

        let status_style = ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]);

        let status_bar = multi.add(ProgressBar::new_spinner());
        status_bar.set_style(status_style);
        status_bar.set_message(format!("{}", style("extracting…").dim()));
        status_bar.enable_steady_tick(Duration::from_millis(80));

        Self {
            _multi: multi,
            main_bar,
            status_bar,
            start: Instant::now(),
            total_bytes,
            bytes_extracted: AtomicU64::new(0),
            finishing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Called after each file is written to disk.
    pub fn inc_extracted(&self, bytes: u64) {
        self.bytes_extracted.fetch_add(bytes, Ordering::Relaxed);
        self.main_bar.inc(bytes);
        self.update_eta();
    }

    fn update_eta(&self) {
        if self.finishing.load(Ordering::Relaxed) {
            return;
        }

        let extracted = self.bytes_extracted.load(Ordering::Relaxed);
        if extracted == 0 {
            return;
        }
        let elapsed = self.start.elapsed();
        let fraction = extracted as f64 / self.total_bytes.max(1) as f64;

        if fraction < 0.10 {
            return;
        }

        let remaining_secs = if fraction > 0.0 {
            elapsed.as_secs_f64() / fraction * (1.0 - fraction)
        } else {
            0.0
        };

        let eta = if remaining_secs < 60.0 {
            format!("ETA {:.0}s", remaining_secs)
        } else if remaining_secs < 3600.0 {
            let mins = (remaining_secs / 60.0).round() as u64;
            format!("ETA ~{}m", mins)
        } else {
            let h = remaining_secs as u64 / 3600;
            let m = ((remaining_secs as u64 % 3600) as f64 / 60.0).round() as u64;
            format!("ETA ~{h}h{m:02}m")
        };
        self.main_bar.set_message(eta);
    }

    /// Transition to "finishing" state — post-file metadata restoration, etc.
    pub fn start_finishing(&self) {
        self.finishing.store(true, Ordering::Relaxed);

        let finish_style = ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]);
        self.main_bar.set_style(finish_style);
        self.main_bar
            .set_message(format!("{}", style("finishing up…").dim()));
        self.status_bar.set_message(String::new());
    }

    pub fn finish(&self) {
        self.finishing.store(false, Ordering::Relaxed);
        self.main_bar.finish_and_clear();
        self.status_bar.finish_and_clear();
    }
}
