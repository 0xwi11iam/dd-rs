/// Status and progress reporting.
///
/// Supports three levels (like GNU dd's `status=`):
///   - `none`:   suppress all output except errors
///   - `noxfer`: suppress transfer statistics (just show errors)
///   - `progress`: show periodic transfer statistics (default)
///
/// Plus dd-rs extras:
///   - `json`:   print final stats as JSON to stderr
///   - `bar`:    show a progress bar (using indicatif)

use std::io::{self, Write};
use std::time::{Duration, Instant};

/// Status verbosity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    /// Suppress all informational output.
    None,
    /// Show errors only; suppress final transfer stats.
    Noxfer,
    /// Show periodic progress and final transfer stats (default).
    Progress,
    /// Like progress, but output final stats as JSON.
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

/// Statistics for a dd-style transfer.
#[derive(Debug, Clone)]
pub struct TransferStats {
    /// Full input blocks read.
    pub full_blocks_in: u64,
    /// Partial input blocks read.
    pub partial_blocks_in: u64,
    /// Full output blocks written.
    pub full_blocks_out: u64,
    /// Partial output blocks written.
    pub partial_blocks_out: u64,
    /// Total bytes read.
    pub bytes_read: u64,
    /// Total bytes written.
    pub bytes_written: u64,
    /// Number of read errors recovered from (noerror mode).
    pub read_errors: u64,
    /// Wall-clock start time.
    pub start_time: Instant,
    /// Wall-clock end time.
    pub end_time: Option<Instant>,
}

impl TransferStats {
    pub fn new() -> Self {
        Self {
            full_blocks_in: 0,
            partial_blocks_in: 0,
            full_blocks_out: 0,
            partial_blocks_out: 0,
            bytes_read: 0,
            bytes_written: 0,
            read_errors: 0,
            start_time: Instant::now(),
            end_time: None,
        }
    }

    /// Mark transfer complete.
    pub fn finish(&mut self) {
        self.end_time = Some(Instant::now());
    }

    /// Get elapsed duration.
    pub fn elapsed(&self) -> Duration {
        self.end_time
            .unwrap_or_else(|| Instant::now())
            .duration_since(self.start_time)
    }

    /// Get throughput in bytes/sec.
    pub fn throughput_bytes_per_sec(&self) -> f64 {
        let elapsed_secs = self.elapsed().as_secs_f64();
        if elapsed_secs > 0.0 {
            self.bytes_written as f64 / elapsed_secs
        } else {
            0.0
        }
    }

    /// Format throughput as a human-readable string.
    pub fn format_throughput(&self) -> String {
        let bps = self.throughput_bytes_per_sec();
        if bps >= 1_000_000_000.0 {
            format!("{:.1} GB/s", bps / 1_000_000_000.0)
        } else if bps >= 1_000_000.0 {
            format!("{:.1} MB/s", bps / 1_000_000.0)
        } else if bps >= 1_000.0 {
            format!("{:.1} kB/s", bps / 1_000.0)
        } else {
            format!("{:.0} B/s", bps)
        }
    }
}

// =============================================================================
// Status reporter
// =============================================================================

/// Renders transfer status to stderr.
pub struct StatusReporter {
    level: StatusLevel,
    stats: TransferStats,
    last_report: Instant,
    report_interval: Duration,
}

impl StatusReporter {
    pub fn new(level: StatusLevel) -> Self {
        Self {
            level,
            stats: TransferStats::new(),
            last_report: Instant::now(),
            report_interval: Duration::from_secs(1),
        }
    }

    pub fn stats(&self) -> &TransferStats {
        &self.stats
    }

    pub fn stats_mut(&mut self) -> &mut TransferStats {
        &mut self.stats
    }

    /// Record a full block read.
    pub fn record_full_block_in(&mut self, bytes: u64) {
        self.stats.full_blocks_in += 1;
        self.stats.bytes_read += bytes;
    }

    /// Record a partial block read.
    pub fn record_partial_block_in(&mut self, bytes: u64) {
        self.stats.partial_blocks_in += 1;
        self.stats.bytes_read += bytes;
    }

    /// Record a full block written.
    pub fn record_full_block_out(&mut self, bytes: u64) {
        self.stats.full_blocks_out += 1;
        self.stats.bytes_written += bytes;
    }

    /// Record a partial block written.
    pub fn record_partial_block_out(&mut self, bytes: u64) {
        self.stats.partial_blocks_out += 1;
        self.stats.bytes_written += bytes;
    }

    /// Record a recovered read error (noerror mode).
    pub fn record_read_error(&mut self) {
        self.stats.read_errors += 1;
    }

    /// Print progress if enough time has elapsed.
    pub fn maybe_report_progress(&mut self) {
        if self.level != StatusLevel::Progress {
            return;
        }
        let now = Instant::now();
        if now.duration_since(self.last_report) < self.report_interval {
            return;
        }
        self.last_report = now;

        let stats = &self.stats;
        let elapsed = stats.elapsed();
        let _bps = stats.throughput_bytes_per_sec();

        let bw = stats.bytes_written;
        eprint!(
            "\r{} bytes ({:.1} {}) copied, {:.1}s, {}",
            bw,
            if bw >= 1_000_000_000 {
                bw as f64 / 1_000_000_000.0
            } else if bw >= 1_000_000 {
                bw as f64 / 1_000_000.0
            } else if bw >= 1_000 {
                bw as f64 / 1_000.0
            } else {
                bw as f64
            },
            if bw >= 1_000_000_000 {
                "GB"
            } else if bw >= 1_000_000 {
                "MB"
            } else if bw >= 1_000 {
                "kB"
            } else {
                "B"
            },
            elapsed.as_secs_f64(),
            stats.format_throughput(),
        );
        let _ = io::stderr().flush();
    }

    /// Print final summary to stderr.
    pub fn report_final(&mut self) {
        self.stats.finish();

        match self.level {
            StatusLevel::None => return,
            StatusLevel::Json => {
                let stats = &self.stats;
                let elapsed = stats.elapsed().as_secs_f64();
                let bps = stats.throughput_bytes_per_sec();
                eprintln!(
                    "{{\n  \"bytes_read\": {},\n  \"bytes_written\": {},\n  \
                     \"full_blocks_in\": {},\n  \"partial_blocks_in\": {},\n  \
                     \"full_blocks_out\": {},\n  \"partial_blocks_out\": {},\n  \
                     \"read_errors\": {},\n  \"elapsed_seconds\": {:.6},\n  \
                     \"throughput_bytes_per_sec\": {:.1}\n}}",
                    stats.bytes_read,
                    stats.bytes_written,
                    stats.full_blocks_in,
                    stats.partial_blocks_in,
                    stats.full_blocks_out,
                    stats.partial_blocks_out,
                    stats.read_errors,
                    elapsed,
                    bps,
                );
            }
            _ => {
                let stats = &self.stats;
                eprintln!(
                    "{}+{} records in\n{}+{} records out\n{} bytes transferred in {:.6} secs ({})",
                    stats.full_blocks_in,
                    stats.partial_blocks_in,
                    stats.full_blocks_out,
                    stats.partial_blocks_out,
                    stats.bytes_written,
                    stats.elapsed().as_secs_f64(),
                    stats.format_throughput(),
                );
            }
        }
    }
}


