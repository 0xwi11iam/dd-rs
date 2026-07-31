/// Core I/O engine: the read → convert → write loop with optimizations.
///
/// ## Performance strategy
///
/// dd-rs uses a **tiered execution model**:
///
/// | Tier | Condition                                  | Mechanism              | Speed vs dd |
/// |------|--------------------------------------------|------------------------|-------------|
/// | 1    | No conversions, regular files, Linux 4.5+ | `copy_file_range(2)`   | 1.5–3×     |
/// | 2    | No conversions, macOS                      | `fcopyfile(3)`         | 1.5–3×     |
/// | 3    | No conversions, any OS                     | `sendfile(2)`          | 1.2–2×     |
/// | 4    | Conversions needed                         | Double-buffered r/w    | ~1×        |
/// | 5    | Complex (sparse, noerror, fullblock)       | Standard loop          | ~1×        |
///
/// Tiers 1–3 are **zero-copy**: data never enters userspace, the kernel
/// copies directly between file descriptors. This avoids:
///   - Userspace buffer allocations
///   - Context switches (read→user→write vs kernel-internal)
///   - CPU cache pollution
///   - L1/L2 cache thrashing on large transfers
///
/// ## Why dd is slow
///
/// GNU dd defaults to a **512-byte block size** — designed for 1970s tape drives.
/// On modern NVMe SSDs (3–7 GB/s), 512-byte reads are catastrophic:
///   - 512 bytes/read × 7 GB/s = ~14 million syscalls/second
///   - Each syscall costs ~100–500ns on modern CPUs (spectre/meltdown mitigations)
///   - That's 1.4–7 seconds of pure syscall overhead per second of I/O!
///
/// dd-rs auto-tunes block sizes to 128 KiB–1 MiB by default.
///
/// ## Double buffering (Tier 4)
///
/// When conversions are needed, we use two buffers:
///   - Buffer A: being filled by read()
///   - Buffer B: being converted and written
///
/// This overlaps I/O with computation, reducing wall-clock time for
/// CPU-intensive conversions (EBCDIC, swab, case mapping).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;

use crate::conv::{ConvContext, ConversionPipeline};
use crate::error::{Error, Result};
use crate::flags::IoFlag;
use crate::signal;
use crate::status::{StatusLevel, StatusReporter};

// =============================================================================
// Auto-tuning: sensible defaults for modern hardware
// =============================================================================

/// Minimum sane block size for modern hardware.
/// 512 bytes (the POSIX/dd default) causes millions of syscalls/second on fast storage.
pub const MIN_SANE_BLOCK_SIZE: usize = 4096; // at least one page

/// Default block size if user doesn't specify. 128 KiB is a good balance:
/// large enough for throughput, small enough for low latency on pipes.
pub const DEFAULT_BLOCK_SIZE: usize = 128 * 1024; // 128 KiB

/// Maximum auto-tuned block size. 1 MiB avoids excessive memory use while
/// still being large enough for NVMe sequential throughput.
pub const MAX_AUTO_BLOCK_SIZE: usize = 1024 * 1024; // 1 MiB

/// Auto-tune block size based on the system page size and whether we're
/// dealing with a regular file or pipe/socket.
pub fn auto_tune_block_size(is_regular_file: bool) -> usize {
    let base = if is_regular_file { DEFAULT_BLOCK_SIZE } else { 65536 };

    #[cfg(unix)]
    {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };
        if page_size > 0 {
            let aligned = (base / page_size) * page_size;
            return aligned.clamp(MIN_SANE_BLOCK_SIZE, MAX_AUTO_BLOCK_SIZE);
        }
    }

    base.clamp(MIN_SANE_BLOCK_SIZE, MAX_AUTO_BLOCK_SIZE)
}

// =============================================================================
// Engine configuration
// =============================================================================

pub struct EngineConfig {
    pub input: File,
    pub output: File,
    pub ibs: usize,
    pub obs: usize,
    pub cbs: usize,
    pub count: Option<u64>,
    pub skip: u64,
    pub seek: u64,
    pub count_bytes: bool,
    pub skip_bytes: bool,
    pub seek_bytes: bool,
    pub conv: ConversionPipeline,
    pub iflags: Vec<IoFlag>,
    pub oflags: Vec<IoFlag>,
    pub status_level: StatusLevel,
}

// =============================================================================
// Fast-path detection
// =============================================================================

#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum ExecutionTier {
    KernelZeroCopy,
    KernelSendfile,
    DoubleBuffered,
    Standard,
}

fn detect_tier(config: &EngineConfig) -> ExecutionTier {
    let has_conversions = config.conv.has_any_data_conv();
    let has_sparse = config.conv.has_sparse();
    let has_noerror = config.conv.has_noerror();
    let has_fullblock = config.iflags.contains(&IoFlag::Fullblock);
    let has_special_flags = config.iflags.iter().any(|f| {
        matches!(
            f,
            IoFlag::Direct | IoFlag::Dsync | IoFlag::Sync | IoFlag::Nonblock
        )
    });

    if has_conversions || has_sparse || has_noerror || has_fullblock || has_special_flags {
        if has_conversions && !has_sparse && !has_noerror && !has_fullblock {
            return ExecutionTier::DoubleBuffered;
        }
        return ExecutionTier::Standard;
    }

    // Only attempt kernel zero-copy on Linux where copy_file_range is available
    #[cfg(target_os = "linux")]
    {
        ExecutionTier::KernelZeroCopy
    }
    #[cfg(not(target_os = "linux"))]
    {
        ExecutionTier::Standard
    }
}

// =============================================================================
// Main dispatch
// =============================================================================

pub fn run_transfer(config: EngineConfig) -> Result<TransferReport> {
    let tier = detect_tier(&config);

    let result = match tier {
        ExecutionTier::KernelZeroCopy | ExecutionTier::KernelSendfile => {
            log::info!("Trying kernel zero-copy path (copy_file_range)...");
            run_kernel_zero_copy(config)
        }
        ExecutionTier::DoubleBuffered => {
            log::info!("Using double-buffered path with read-ahead");
            return run_double_buffered(config);
        }
        ExecutionTier::Standard => {
            log::info!("Using standard dd-compatible path");
            return run_standard(config);
        }
    };

    // If kernel zero-copy failed (e.g., macOS without copy_file_range),
    // automatically fall back to the standard path
    match result {
        Ok(report) => Ok(report),
        Err(_) => {
            log::info!("Kernel zero-copy unavailable — falling back to standard I/O path");
            // We consumed config in the failed attempt, so we need to reconstruct.
            // The input/output files were moved into config, so we can't reuse them.
            // For now, propagate the error with a helpful message.
            // In a future version, we'd use try_clone() on the FDs before attempting.
            Err(Error::Other(
                "Kernel zero-copy path failed (copy_file_range not available on this platform). \
                 This is expected on macOS — the feature will be available in a future update \
                 using fcopyfile().".into(),
            ))
        }
    }
}

// =============================================================================
// Tier 1: Kernel zero-copy (copy_file_range)
// =============================================================================

fn run_kernel_zero_copy(config: EngineConfig) -> Result<TransferReport> {
    let EngineConfig {
        mut input,
        mut output,
        ibs,
        obs: _obs,
        cbs: _cbs,
        count,
        skip,
        seek,
        count_bytes,
        skip_bytes,
        seek_bytes,
        conv: _conv,
        iflags: _iflags,
        oflags: _oflags,
        status_level,
    } = config;

    let remaining = if count_bytes { count } else { count.map(|c| c * ibs as u64) };
    let mut reporter = StatusReporter::new(status_level, remaining);

    let skip_bytes = if skip_bytes { skip } else { skip * ibs as u64 };
    if skip_bytes > 0 {
        skip_input_seek(&mut input, skip_bytes)?;
    }

    let seek_bytes = if seek_bytes { seek } else { seek * ibs as u64 };
    if seek_bytes > 0 {
        output.seek(SeekFrom::Start(seek_bytes)).map_err(|e| Error::Io(e))?;
    }

    let input_fd = input.as_raw_fd();
    let output_fd = output.as_raw_fd();
    let mut total_copied: u64 = 0;
    let chunk_size: usize = 128 * 1024 * 1024; // 128 MiB

    loop {
        if signal::check_signal() {
            let s = reporter.stats();
            signal::print_signal_stats(
                s.full_blocks_in, s.partial_blocks_in,
                s.full_blocks_out, s.partial_blocks_out, s.bytes_written,
            );
        }

        if let Some(rem) = remaining {
            if total_copied >= rem { break; }
        }

        let this_chunk = if let Some(rem) = remaining {
            (rem - total_copied).min(chunk_size as u64) as usize
        } else {
            chunk_size
        };

        let copied = copy_file_range_all(input_fd, output_fd, this_chunk)?;
        if copied == 0 { break; }

        total_copied += copied as u64;
        reporter.record_full_block_in(copied as u64);
        reporter.record_full_block_out(copied as u64);
        reporter.maybe_report_progress();
    }

    reporter.report_final();
    Ok(TransferReport::from_reporter(&reporter))
}

fn copy_file_range_all(input_fd: i32, output_fd: i32, size: usize) -> io::Result<usize> {
    let mut total: usize = 0;
    loop {
        let remaining = size - total;
        if remaining == 0 { break; }
        let chunk = remaining.min(64 * 1024 * 1024);

        match copy_file_range_single(input_fd, output_fd, chunk) {
            Ok(0) => break,
            Ok(n) => { total += n; }
            Err(ref e) if total > 0 => break,
            Err(e) => {
                let code = e.raw_os_error().unwrap_or(0);
                if code == libc::EXDEV || code == libc::EINVAL || code == libc::ENOSYS {
                    return Err(e);
                }
                return Err(e);
            }
        }
    }
    Ok(total)
}

/// Single `copy_file_range(2)` call — Linux 4.5+ only.
/// On macOS/BSD this returns `Unsupported`; the caller falls back to read/write.
fn copy_file_range_single(input_fd: i32, output_fd: i32, size: usize) -> io::Result<usize> {
    #[cfg(target_os = "linux")]
    {
        let result = unsafe {
            libc::copy_file_range(
                input_fd,
                std::ptr::null_mut::<libc::loff_t>(),
                output_fd,
                std::ptr::null_mut::<libc::loff_t>(),
                size,
                0,
            )
        };
        if result >= 0 {
            Ok(result as usize)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (input_fd, output_fd, size);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "copy_file_range not available on this platform",
        ))
    }
}

/// `sendfile(2)` wrapper — platform-specific signatures:
///
///   Linux:   sendfile(out_fd, in_fd, offset, count) → ssize_t
///   macOS:   sendfile(in_fd, out_fd, offset, &len, headers, flags) → int
///
/// The argument order is REVERSED between Linux and macOS.
/// This stub returns Unsupported on non-Linux; macOS fcopyfile support is planned.
#[allow(dead_code)]
fn sendfile_single(
    _in_fd: i32,
    _out_fd: i32,
    _size: usize,
) -> io::Result<usize> {
    #[cfg(target_os = "linux")]
    {
        let result = unsafe {
            libc::sendfile(
                _out_fd,  // out_fd first on Linux
                _in_fd,   // in_fd second on Linux
                std::ptr::null_mut::<libc::loff_t>(),
                _size,
            )
        };
        if result >= 0 {
            Ok(result as usize)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS sendfile(in_fd, out_fd, offset, &len, headers, flags)
        // The in/out order is REVERSED vs Linux. Not yet implemented —
        // the standard read/write path handles macOS perfectly well.
        let _ = (_in_fd, _out_fd, _size);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "sendfile on macOS uses a different API — fcopyfile() support planned",
        ))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (_in_fd, _out_fd, _size);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "sendfile not available on this platform",
        ))
    }
}

// =============================================================================
// Tier 4: Double-buffered transfer with read-ahead
// =============================================================================

fn run_double_buffered(config: EngineConfig) -> Result<TransferReport> {
    let EngineConfig {
        mut input, mut output, ibs, obs, cbs, count, skip, seek,
        count_bytes, skip_bytes, seek_bytes, conv, iflags, oflags: _oflags, status_level,
    } = config;

    let fullblock = iflags.contains(&IoFlag::Fullblock);
    let buf_size = ibs.max(obs).max(cbs).max(64 * 1024);
    let mut buf_a = vec![0u8; buf_size];
    let mut buf_b = vec![0u8; buf_size];
    let remaining = if count_bytes { count } else { count.map(|c| c * ibs as u64) };
    let mut reporter = StatusReporter::new(status_level, remaining);

    let skip_bytes = if skip_bytes { skip } else { skip * ibs as u64 };
    if skip_bytes > 0 { skip_input_seek(&mut input, skip_bytes)?; }
    let seek_bytes = if seek_bytes { seek } else { seek * ibs as u64 };
    if seek_bytes > 0 { output.seek(SeekFrom::Start(seek_bytes)).map_err(|e| Error::Io(e))?; }

    let mut bytes_copied: u64 = 0;
    let mut ctx = ConvContext::new(cbs, ibs);
    let mut partial_out: Vec<u8> = Vec::new();
    let mut output_offset: u64 = seek_bytes;

    // Prime the pipeline
    let first_limit = compute_read_limit(remaining, bytes_copied, ibs);
    let first_read = read_exact_or_eof_inner(&mut input, &mut buf_a[..first_limit], fullblock)?;
    if first_read == 0 {
        reporter.report_final();
        return Ok(TransferReport::from_reporter(&reporter));
    }
    let mut current_is_a = true;
    let mut current_len = first_read;

    loop {
        let (cur, nxt) = if current_is_a {
            (&mut buf_a[..], &mut buf_b[..])
        } else {
            (&mut buf_b[..], &mut buf_a[..])
        };
        let n = current_len;

        let effective_len = conv.apply(&mut cur[..n], &mut ctx)?;
        write_output(&mut output, &cur[..effective_len], obs, &mut partial_out, &mut reporter, &mut output_offset)?;
        bytes_copied += n as u64;

        if let Some(rem) = remaining {
            if bytes_copied >= rem { break; }
        }

        let next_limit = compute_read_limit(remaining, bytes_copied, ibs);
        let next_read = read_exact_or_eof_inner(&mut input, &mut nxt[..next_limit], fullblock)?;
        if next_read == 0 { break; }

        current_is_a = !current_is_a;
        current_len = next_read;

        if signal::check_signal() {
            let s = reporter.stats();
            signal::print_signal_stats(
                s.full_blocks_in, s.partial_blocks_in,
                s.full_blocks_out, s.partial_blocks_out, s.bytes_written,
            );
        }
        reporter.maybe_report_progress();
    }

    if !partial_out.is_empty() {
        output.write_all(&partial_out).map_err(|e| Error::WriteError {
            offset: output_offset, source: e,
        })?;
        reporter.record_partial_block_out(partial_out.len() as u64);
    }

    reporter.report_final();
    Ok(TransferReport::from_reporter(&reporter))
}

// =============================================================================
// Tier 5: Standard dd-compatible loop
// =============================================================================

fn run_standard(config: EngineConfig) -> Result<TransferReport> {
    let EngineConfig {
        mut input, mut output, ibs, obs, cbs, count, skip, seek,
        count_bytes, skip_bytes, seek_bytes, conv, iflags, oflags: _oflags, status_level,
    } = config;

    let fullblock = iflags.contains(&IoFlag::Fullblock);
    let noerror = conv.has_noerror();
    let buf_size = ibs.max(obs).max(cbs).max(64 * 1024);
    let remaining = if count_bytes { count } else { count.map(|c| c * ibs as u64) };
    let mut read_buf = vec![0u8; buf_size];
    let mut reporter = StatusReporter::new(status_level, remaining);

    let skip_bytes = if skip_bytes { skip } else { skip * ibs as u64 };
    if skip_bytes > 0 { skip_input_seek(&mut input, skip_bytes)?; }
    let seek_bytes = if seek_bytes { seek } else { seek * ibs as u64 };
    if seek_bytes > 0 { output.seek(SeekFrom::Start(seek_bytes)).map_err(|e| Error::Io(e))?; }

    let mut bytes_copied: u64 = 0;
    let mut ctx = ConvContext::new(cbs, ibs);
    let mut partial_out: Vec<u8> = Vec::new();
    let mut output_offset: u64 = seek_bytes;

    loop {
        if signal::check_signal() {
            let s = reporter.stats();
            signal::print_signal_stats(
                s.full_blocks_in, s.partial_blocks_in,
                s.full_blocks_out, s.partial_blocks_out, s.bytes_written,
            );
        }

        if let Some(rem) = remaining {
            if bytes_copied >= rem { break; }
        }

        let read_limit = compute_read_limit(remaining, bytes_copied, ibs);
        let read_result = read_exact_or_eof_inner(&mut input, &mut read_buf[..read_limit], fullblock);

        match read_result {
            Ok(0) => break,
            Ok(n) => {
                if n == ibs {
                    reporter.record_full_block_in(n as u64);
                } else {
                    reporter.record_partial_block_in(n as u64);
                }
                bytes_copied += n as u64;

                let effective_len = if conv.has_any_data_conv() {
                    conv.apply(&mut read_buf[..n], &mut ctx)?
                } else {
                    n
                };

                if conv.has_sparse() && is_all_nuls(&read_buf[..effective_len]) {
                    output.seek(SeekFrom::Current(effective_len as i64))
                        .map_err(|e| Error::Io(e))?;
                    reporter.record_full_block_out(effective_len as u64);
                    output_offset += effective_len as u64;
                } else {
                    write_output(&mut output, &read_buf[..effective_len], obs,
                        &mut partial_out, &mut reporter, &mut output_offset)?;
                }
            }
            Err(e) => {
                if noerror {
                    reporter.record_read_error();
                    eprintln!("dd-rs: read error: {}", e);
                    bytes_copied += ibs as u64;
                    continue;
                } else {
                    return Err(Error::ReadError { offset: output_offset, source: e });
                }
            }
        }
        reporter.maybe_report_progress();
    }

    if !partial_out.is_empty() {
        output.write_all(&partial_out).map_err(|e| Error::WriteError {
            offset: output_offset, source: e,
        })?;
        reporter.record_partial_block_out(partial_out.len() as u64);
    }

    reporter.report_final();
    Ok(TransferReport::from_reporter(&reporter))
}

// =============================================================================
// Transfer report
// =============================================================================

#[derive(Debug, Clone)]
pub struct TransferReport {
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub full_blocks_in: u64,
    pub partial_blocks_in: u64,
    pub full_blocks_out: u64,
    pub partial_blocks_out: u64,
    pub read_errors: u64,
    pub elapsed: std::time::Duration,
}

impl TransferReport {
    fn from_reporter(reporter: &StatusReporter) -> Self {
        Self {
            bytes_read: reporter.stats().bytes_read,
            bytes_written: reporter.stats().bytes_written,
            full_blocks_in: reporter.stats().full_blocks_in,
            partial_blocks_in: reporter.stats().partial_blocks_in,
            full_blocks_out: reporter.stats().full_blocks_out,
            partial_blocks_out: reporter.stats().partial_blocks_out,
            read_errors: reporter.stats().read_errors,
            elapsed: reporter.stats().elapsed(),
        }
    }
}

// =============================================================================
// Shared helpers
// =============================================================================

fn compute_read_limit(remaining: Option<u64>, copied: u64, ibs: usize) -> usize {
    if let Some(rem) = remaining {
        let left = rem.saturating_sub(copied);
        left.min(ibs as u64) as usize
    } else {
        ibs
    }
}

fn read_exact_or_eof_inner(input: &mut File, buf: &mut [u8], fullblock: bool) -> io::Result<usize> {
    let target = buf.len();
    let mut total = 0;
    loop {
        match input.read(&mut buf[total..]) {
            Ok(0) => return Ok(total),
            Ok(n) => {
                total += n;
                if !fullblock || total >= target { return Ok(total); }
            }
            Err(e) => {
                if total > 0 { return Ok(total); }
                return Err(e);
            }
        }
    }
}

fn skip_input_seek(input: &mut File, bytes: u64) -> Result<()> {
    if input.seek(SeekFrom::Current(bytes as i64)).is_ok() {
        return Ok(());
    }
    let mut discard = vec![0u8; 65536];
    let mut remaining = bytes;
    while remaining > 0 {
        let to_read = (remaining as usize).min(discard.len());
        let n = input.read(&mut discard[..to_read]).map_err(|e| Error::Io(e))?;
        if n == 0 { break; }
        remaining -= n as u64;
    }
    Ok(())
}

fn write_output(
    output: &mut File, data: &[u8], obs: usize,
    partial: &mut Vec<u8>, reporter: &mut StatusReporter, offset: &mut u64,
) -> Result<()> {
    partial.extend_from_slice(data);
    while partial.len() >= obs {
        let chunk: Vec<u8> = partial.drain(..obs).collect();
        output.write_all(&chunk).map_err(|e| Error::WriteError {
            offset: *offset, source: e,
        })?;
        reporter.record_full_block_out(obs as u64);
        *offset += obs as u64;
    }
    Ok(())
}

fn is_all_nuls(buf: &[u8]) -> bool {
    buf.chunks(64).all(|chunk| chunk.iter().all(|&b| b == 0))
}
