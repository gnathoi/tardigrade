// Adaptive thread scaling for archive creation.
//
// Three layers:
//   1. Startup memory cap — clamp initial thread count to fit available memory,
//      so `-j auto` doesn't OOM on CI runners, containers, or small VPS.
//   2. Runtime backpressure — during the archive walk+compress pipeline, watch
//      process RSS per batch. When it climbs past the pressure threshold,
//      shrink the batch size (reducing in-flight work). When it drops back,
//      grow it again.
//   3. Throughput-based scaling — track bytes/sec per batch. When adding work
//      per batch stops improving throughput (I/O bound or cache contention),
//      stop growing. Prevents scaling past useful parallelism on systems where
//      the serial fraction (dedup lookup, sequential writes) dominates.
//
// An explicit `-j N` from the user bypasses all three layers — user intent wins.

use std::time::Instant;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Pessimistic per-thread memory reserve used for startup capping.
/// zstd context at level 9 is ~8 MB; in-flight block buffers (input + compressed
/// output) run ~4-8 MB per worker; plus a safety margin. 32 MB per thread is a
/// defensible overestimate that keeps the cap from OOM'ing real workloads.
const PER_THREAD_RESERVE_BYTES: u64 = 32 * 1024 * 1024;

/// Never reduce below this many threads, even on very small systems.
const MIN_THREADS: usize = 2;

/// Fraction of available memory we're willing to use for archive work.
/// The rest is reserved for page cache, OS, and concurrent processes.
const MEMORY_BUDGET_FRACTION: f64 = 0.5;

/// RSS above this fraction of the memory budget → shrink the next batch.
const PRESSURE_HIGH: f64 = 0.75;

/// RSS below this fraction of the memory budget → allow the batch to grow.
const PRESSURE_LOW: f64 = 0.40;

/// Throughput must improve by at least this much to count as "still scaling".
const THROUGHPUT_IMPROVEMENT_EPSILON: f64 = 0.05; // 5%

/// After this many consecutive non-improving batches at the current size, we
/// freeze growth (throughput-based scaling signal).
const PLATEAU_SAMPLES: usize = 3;

/// Startup configuration for the rayon thread pool.
#[derive(Debug, Clone)]
pub struct ThreadConfig {
    pub threads: usize,
    pub user_override: bool,
    pub memory_budget: u64,
    pub available_memory: u64,
    pub logical_cpus: usize,
}

impl ThreadConfig {
    /// Configure the initial rayon pool size.
    /// `user_threads = Some(n)` → honor exactly, bypass memory cap.
    pub fn configure(user_threads: Option<usize>) -> Self {
        let logical_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let available = probe_available_memory();

        if let Some(n) = user_threads {
            return Self {
                threads: n.max(1),
                user_override: true,
                memory_budget: available,
                available_memory: available,
                logical_cpus,
            };
        }

        let budget = (available as f64 * MEMORY_BUDGET_FRACTION) as u64;
        let max_by_memory = (budget / PER_THREAD_RESERVE_BYTES).max(1) as usize;
        let threads = logical_cpus.min(max_by_memory).max(MIN_THREADS);

        Self {
            threads,
            user_override: false,
            memory_budget: budget,
            available_memory: available,
            logical_cpus,
        }
    }

    /// Returns true if the startup memory cap reduced the thread count below
    /// what logical CPUs alone would allow.
    pub fn clamped_by_memory(&self) -> bool {
        !self.user_override && self.threads < self.logical_cpus
    }

    /// Human-readable diagnostic for verbose mode / debugging.
    pub fn diagnostic(&self) -> String {
        if self.user_override {
            format!("threads: {} (user -j)", self.threads)
        } else if self.clamped_by_memory() {
            format!(
                "threads: {} (capped from {} cores, memory budget {} MiB of {} MiB available)",
                self.threads,
                self.logical_cpus,
                self.memory_budget / (1024 * 1024),
                self.available_memory / (1024 * 1024),
            )
        } else {
            format!("threads: {} ({} cores)", self.threads, self.logical_cpus)
        }
    }
}

/// Probe currently available system memory (bytes). Returns a conservative
/// non-zero value on failure so callers don't accidentally divide by zero.
fn probe_available_memory() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    let avail = sys.available_memory();
    // sysinfo returns bytes in 0.30+. Guard against zero.
    avail.max(PER_THREAD_RESERVE_BYTES * MIN_THREADS as u64)
}

/// Runtime controller for batch sizing. Fed RSS + throughput measurements at
/// each batch boundary; returns the next batch size.
pub struct BatchController {
    max_batch: usize,
    current_batch: usize,
    min_batch: usize,
    memory_budget: u64,
    sampler: RssSampler,
    batch_start: Option<Instant>,
    best_throughput: Option<f64>,
    plateau_count: usize,
    growth_frozen: bool,
    diagnostics: Vec<String>,
}

impl BatchController {
    /// Build a controller that probes system memory itself for its budget.
    pub fn new(initial_batch: usize) -> Self {
        let available = probe_available_memory();
        let budget = (available as f64 * MEMORY_BUDGET_FRACTION) as u64;
        Self::with_budget(initial_batch, budget)
    }

    /// Like `new`, but with an explicit budget (for testing).
    pub fn with_budget(initial_batch: usize, memory_budget: u64) -> Self {
        Self {
            max_batch: initial_batch,
            current_batch: initial_batch,
            min_batch: 1,
            memory_budget,
            sampler: RssSampler::new(),
            batch_start: None,
            best_throughput: None,
            plateau_count: 0,
            growth_frozen: false,
            diagnostics: Vec::new(),
        }
    }

    /// Current batch size to use when slicing walk_entries.
    pub fn batch_size(&self) -> usize {
        self.current_batch
    }

    /// Called just before processing a batch. Records the start time for
    /// throughput measurement.
    pub fn start_batch(&mut self) {
        self.batch_start = Some(Instant::now());
    }

    /// Called after a batch finishes writing. Updates the controller state
    /// based on RSS (memory pressure) and bytes/sec (throughput plateau).
    pub fn end_batch(&mut self, bytes_in_batch: u64) {
        let elapsed = self
            .batch_start
            .take()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        let throughput = if elapsed > 0.0 {
            bytes_in_batch as f64 / elapsed
        } else {
            0.0
        };

        // Memory-pressure feedback — always on, even under explicit -j
        // (user's -j sets rayon pool size, not in-flight work bound).
        if self.memory_budget > 0 {
            let rss = self.sampler.current_rss();
            let pressure = rss as f64 / self.memory_budget as f64;

            if pressure > PRESSURE_HIGH && self.current_batch > self.min_batch {
                let new_size = (self.current_batch / 2).max(self.min_batch);
                self.diagnostics.push(format!(
                    "memory pressure {:.0}% of budget → batch {} → {}",
                    pressure * 100.0,
                    self.current_batch,
                    new_size,
                ));
                self.current_batch = new_size;
                // After a shrink, reset throughput baseline — measurements at
                // the old size no longer compare fairly.
                self.best_throughput = None;
                self.plateau_count = 0;
                self.growth_frozen = false;
                return;
            }

            if pressure < PRESSURE_LOW && self.current_batch < self.max_batch && !self.growth_frozen
            {
                let new_size = (self.current_batch * 2).min(self.max_batch);
                self.diagnostics.push(format!(
                    "memory pressure {:.0}% of budget → batch {} → {}",
                    pressure * 100.0,
                    self.current_batch,
                    new_size,
                ));
                self.current_batch = new_size;
                self.best_throughput = None;
                self.plateau_count = 0;
                return;
            }
        }

        // Throughput-based scaling: if adding work per batch has stopped
        // improving throughput, freeze growth. We never force a shrink on
        // throughput alone — only memory pressure can shrink.
        if throughput > 0.0 && !self.growth_frozen {
            match self.best_throughput {
                None => {
                    self.best_throughput = Some(throughput);
                    self.plateau_count = 0;
                }
                Some(best) => {
                    let improvement = (throughput - best) / best;
                    if improvement > THROUGHPUT_IMPROVEMENT_EPSILON {
                        self.best_throughput = Some(throughput);
                        self.plateau_count = 0;
                    } else {
                        self.plateau_count += 1;
                        if self.plateau_count >= PLATEAU_SAMPLES {
                            self.growth_frozen = true;
                            self.max_batch = self.current_batch;
                            self.diagnostics.push(format!(
                                "throughput plateau at batch {} ({:.0} MiB/s) — freezing growth",
                                self.current_batch,
                                throughput / (1024.0 * 1024.0),
                            ));
                        }
                    }
                }
            }
        }
    }

    /// Diagnostic lines collected during this run (for verbose output).
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

/// Caches sysinfo state across RSS polls to avoid rebuilding it each call.
struct RssSampler {
    sys: System,
    pid: Pid,
}

impl RssSampler {
    fn new() -> Self {
        let pid = Pid::from_u32(std::process::id());
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::new().with_memory(),
        );
        Self { sys, pid }
    }

    /// Current resident-set size in bytes. Returns 0 on failure.
    fn current_rss(&mut self) -> u64 {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::new().with_memory(),
        );
        self.sys.process(self.pid).map(|p| p.memory()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_user_override_wins() {
        let cfg = ThreadConfig::configure(Some(1));
        assert_eq!(cfg.threads, 1);
        assert!(cfg.user_override);
        assert!(!cfg.clamped_by_memory());
    }

    #[test]
    fn user_override_does_not_clamp_for_high_counts() {
        // Even 256 threads should be honored exactly when explicit.
        let cfg = ThreadConfig::configure(Some(256));
        assert_eq!(cfg.threads, 256);
        assert!(cfg.user_override);
    }

    #[test]
    fn auto_threads_respects_min_floor() {
        let cfg = ThreadConfig::configure(None);
        assert!(cfg.threads >= MIN_THREADS);
    }

    #[test]
    fn auto_threads_capped_by_logical_cpus() {
        let cfg = ThreadConfig::configure(None);
        // On any real machine, threads should never exceed logical_cpus when
        // auto-configured. Memory cap only *reduces* from that baseline.
        assert!(cfg.threads <= cfg.logical_cpus.max(MIN_THREADS));
    }

    #[test]
    fn controller_honors_initial_batch() {
        let mut ctrl = BatchController::with_budget(16, 1_000_000_000);
        assert_eq!(ctrl.batch_size(), 16);
        ctrl.start_batch();
        ctrl.end_batch(1_000_000);
        assert!(ctrl.batch_size() <= 16);
        assert!(ctrl.batch_size() >= 1);
    }

    #[test]
    fn controller_throughput_plateau_freezes_growth() {
        // Huge budget so memory pressure can't interfere.
        let mut ctrl = BatchController::with_budget(16, u64::MAX);
        for _ in 0..(PLATEAU_SAMPLES + 1) {
            ctrl.batch_start = Some(Instant::now() - std::time::Duration::from_millis(100));
            ctrl.end_batch(1_000_000);
        }
        assert!(
            ctrl.growth_frozen,
            "should freeze after {PLATEAU_SAMPLES} flat batches"
        );
    }

    #[test]
    fn controller_shrinks_under_memory_pressure() {
        // Tiny budget (1 byte) guarantees pressure exceeds HIGH threshold.
        let mut ctrl = BatchController::with_budget(16, 1);
        let initial = ctrl.batch_size();
        ctrl.start_batch();
        ctrl.end_batch(1_000_000);
        assert!(
            ctrl.batch_size() < initial,
            "batch should shrink under pressure: {} < {}",
            ctrl.batch_size(),
            initial
        );
    }

    #[test]
    fn diagnostic_string_is_non_empty() {
        let cfg = ThreadConfig::configure(None);
        assert!(!cfg.diagnostic().is_empty());
    }
}
