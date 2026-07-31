/// Status and progress reporting with indicatif progress bar.
///
/// Supports four levels (like GNU dd's `status=`):
///   - `none`:   suppress all output except errors
///   - `noxfer`: suppress transfer statistics
///   - `progress`: show indicatif progress bar (default)
///   - `json`:   print final stats as JSON to stderr

use std::io::{self, Write};
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressStyle};

/// Status verbosity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    None,
    Noxfer,
    Progress,
    Json,
}

impl StatusLevel {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "none" => Some(Self::None),
            "noxfer" => Some(Self::Noxfer),
            "progress" => Some(Self::Progress),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

// =============================================================================
// Transfer statistics
// =============================================================================

#[derive(Debug, Clone)]
pub struct TransferStats {
    pub full_blocks_in: u64,
    pub partial_blocks_in: u64,
    pub full_blocks_out: u64,
    pub partial_blocks_out: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_errors: u64,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
}

impl TransferStats {
    pub fn new() -> Self {
        Self {
            full_blocks_in: 0, partial_blocks_in: 0,
            full_blocks_out: 0, partial_blocks_out: 0,
            bytes_read: 0, bytes_written: 0, read_errors: 0,
            start_time: Instant::now(), end_time: None,
        }
    }

    pub fn finish(&mut self) { self.end_time = Some(Instant::now()); }

    pub fn elapsed(&self) -> Duration {
        self.end_time.unwrap_or_else(|| Instant::now()).duration_since(self.start_time)
    }

    pub fn throughput_bytes_per_sec(&self) -> f64 {
        let s = self.elapsed().as_secs_f64();
        if s > 0.0 { self.bytes_written as f64 / s } else { 0.0 }
    }

    pub fn format_throughput(&self) -> String {
        let bps = self.throughput_bytes_per_sec();
        if bps >= 1_000_000_000.0 { format!("{:.1} GB/s", bps / 1_000_000_000.0) }
        else if bps >= 1_000_000.0 { format!("{:.1} MB/s", bps / 1_000_000.0) }
        else if bps >= 1_000.0 { format!("{:.1} kB/s", bps / 1_000.0) }
        else { format!("{:.0} B/s", bps) }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 { format!("{:.2} GB", bytes as f64 / 1_000_000_000.0) }
    else if bytes >= 1_000_000 { format!("{:.2} MB", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.2} kB", bytes as f64 / 1_000.0) }
    else { format!("{} B", bytes) }
}

// =============================================================================
// Status reporter with indicatif progress bar
// =============================================================================

pub struct StatusReporter {
    level: StatusLevel,
    stats: TransferStats,
    bar: Option<ProgressBar>,
    total: Option<u64>,
}

impl StatusReporter {
    pub fn new(level: StatusLevel, total_bytes: Option<u64>) -> Self {
        let bar = if level == StatusLevel::Progress {
            let pb = ProgressBar::new(total_bytes.unwrap_or(0));
            if total_bytes.is_some() {
                // Bounded transfer — show percentage, ETA, speed, bar
                pb.set_style(
                    ProgressStyle::with_template(
                        "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})"
                    )
                    .unwrap()
                    .progress_chars("#>-")
                );
            } else {
                // Unbounded transfer — show spinner, bytes, speed (no percentage)
                pb.set_style(
                    ProgressStyle::with_template(
                        "{spinner:.green} [{elapsed_precise}] {bytes} ({bytes_per_sec})"
                    )
                    .unwrap()
                );
            }
            pb.enable_steady_tick(Duration::from_millis(100));
            Some(pb)
        } else {
            None
        };

        Self { level, stats: TransferStats::new(), bar, total: total_bytes }
    }

    pub fn stats(&self) -> &TransferStats { &self.stats }
    pub fn stats_mut(&mut self) -> &mut TransferStats { &mut self.stats }

    pub fn record_full_block_in(&mut self, bytes: u64) {
        self.stats.full_blocks_in += 1;
        self.stats.bytes_read += bytes;
        self.tick(bytes);
    }

    pub fn record_partial_block_in(&mut self, bytes: u64) {
        self.stats.partial_blocks_in += 1;
        self.stats.bytes_read += bytes;
        self.tick(bytes);
    }

    pub fn record_full_block_out(&mut self, bytes: u64) {
        self.stats.full_blocks_out += 1;
        self.stats.bytes_written += bytes;
        self.tick(bytes);
    }

    pub fn record_partial_block_out(&mut self, bytes: u64) {
        self.stats.partial_blocks_out += 1;
        self.stats.bytes_written += bytes;
        self.tick(bytes);
    }

    pub fn record_read_error(&mut self) {
        self.stats.read_errors += 1;
    }

    /// Advance the progress bar by `bytes` if active.
    fn tick(&mut self, bytes: u64) {
        if let Some(ref bar) = self.bar {
            if let Some(total) = self.total {
                // Bounded: set absolute position
                let pos = self.stats.bytes_written.min(total);
                bar.set_position(pos);
            } else {
                // Unbounded: increment by bytes
                bar.inc(bytes);
            }
        }
    }

    /// Print progress if enough time has elapsed (legacy text fallback).
    pub fn maybe_report_progress(&mut self) {
        // With indicatif, the bar handles its own rendering — nothing to do here
        if self.bar.is_some() { return; }

        if self.level != StatusLevel::Progress { return; }
        let stats = &self.stats;
        let bw = stats.bytes_written;
        eprint!("\r{} ({}) copied, {}, {}",
            format_size(bw),
            bw,
            stats.elapsed().as_secs_f64(),
            stats.format_throughput(),
        );
        let _ = io::stderr().flush();
    }

    /// Print final summary and finish the bar.
    pub fn report_final(&mut self) {
        self.stats.finish();

        // Finish the progress bar cleanly
        if let Some(ref bar) = self.bar {
            if let Some(total) = self.total {
                bar.set_position(total);
            }
            bar.finish_and_clear();
        }

        match self.level {
            StatusLevel::None => return,
            StatusLevel::Json => {
                let s = &self.stats;
                let elapsed = s.elapsed().as_secs_f64();
                let bps = s.throughput_bytes_per_sec();
                eprintln!(
                    "{{\n  \"bytes_read\": {},\n  \"bytes_written\": {},\n  \
                     \"full_blocks_in\": {},\n  \"partial_blocks_in\": {},\n  \
                     \"full_blocks_out\": {},\n  \"partial_blocks_out\": {},\n  \
                     \"read_errors\": {},\n  \"elapsed_seconds\": {:.6},\n  \
                     \"throughput_bytes_per_sec\": {:.1}\n}}",
                    s.bytes_read, s.bytes_written,
                    s.full_blocks_in, s.partial_blocks_in,
                    s.full_blocks_out, s.partial_blocks_out,
                    s.read_errors, elapsed, bps,
                );
            }
            _ => {
                let s = &self.stats;
                eprintln!(
                    "{}+{} records in\n{}+{} records out\n{} bytes transferred in {:.6} secs ({})",
                    s.full_blocks_in, s.partial_blocks_in,
                    s.full_blocks_out, s.partial_blocks_out,
                    s.bytes_written,
                    s.elapsed().as_secs_f64(),
                    s.format_throughput(),
                );
            }
        }
    }
}
