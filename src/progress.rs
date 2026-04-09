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

const PULSE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Progress display for archive creation
pub struct CreateProgress {
    _multi: MultiProgress,
    main_bar: ProgressBar,
    status_bar: ProgressBar,
    pub stats: Arc<ProgressStats>,
    start: Instant,
    total_bytes: u64,
    finishing: Arc<AtomicBool>,
    pulse_idx: AtomicU64,
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

        // Status line below
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
            finishing: Arc::new(AtomicBool::new(false)),
            pulse_idx: AtomicU64::new(0),
        }
    }

    fn pulse_char(&self) -> &'static str {
        let idx = self.pulse_idx.fetch_add(1, Ordering::Relaxed) as usize % PULSE_FRAMES.len();
        PULSE_FRAMES[idx]
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

        // Before 10%: show pulsing spinner instead of unreliable ETA
        if fraction < 0.10 {
            let pulse = self.pulse_char();
            self.main_bar
                .set_message(format!("{}", style(pulse).cyan()));
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

    /// Update the status line with pulsing animation.
    pub fn inc_compressed(&self, _bytes: u64) {
        let pulse = self.pulse_char();
        self.status_bar.set_message(format!(
            "{}  {}",
            style(pulse).cyan(),
            style("compressing…").dim(),
        ));
    }

    /// Transition to "finishing" state — replace bar + ETA with a pulsing animation
    pub fn start_finishing(&self) {
        self.finishing.store(true, Ordering::Relaxed);

        // Switch main bar to a simple message style
        let bar_style = ProgressStyle::with_template("  {msg}").unwrap();
        self.main_bar.set_style(bar_style);
        self.main_bar.set_message(format!(
            "{}  {}",
            style("⠋").cyan(),
            style("finishing up…").dim()
        ));
        self.status_bar.set_message(String::new());

        // Animate the finishing spinner in a background thread
        let bar = self.main_bar.clone();
        let finishing = self.finishing.clone();
        std::thread::spawn(move || {
            let mut idx = 0usize;
            while finishing.load(Ordering::Relaxed) {
                let frame = PULSE_FRAMES[idx % PULSE_FRAMES.len()];
                bar.set_message(format!(
                    "{}  {}",
                    style(frame).cyan(),
                    style("finishing up…").dim()
                ));
                std::thread::sleep(Duration::from_millis(80));
                idx += 1;
            }
        });
    }

    pub fn finish(&self) {
        self.finishing.store(false, Ordering::Relaxed);
        // Small sleep to let the spinner thread notice and exit
        std::thread::sleep(Duration::from_millis(100));
        self.main_bar.finish_and_clear();
        self.status_bar.finish_and_clear();
    }
}
