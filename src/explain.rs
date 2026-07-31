/// Command explanation engine — explains what a dd-rs command will do
/// before it runs, with a full risk assessment.
///
/// Triggered by `--explain` (or `-E`):
///
/// ```bash
/// $ dd-rs if=/dev/zero of=test.bin bs=1M count=10 --explain
/// $ dd-rs --explain if=/dev/sda of=/dev/sdb bs=4M
/// ```

use std::path::Path;

use crate::conv::ConvOp;
use crate::flags::IoFlag;
use crate::safety::{self, RiskAssessment, RiskLevel};

// ANSI escape codes for terminal formatting
mod color {
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const CYAN: &str = "\x1b[36m";
    pub const RST: &str = "\x1b[0m";
}

use color::*;

pub fn explain(
    input_path: Option<&str>, output_path: Option<&str>,
    ibs: u64, obs: u64, count: Option<u64>,
    skip: u64, seek: u64,
    count_bytes: bool, skip_bytes: bool, seek_bytes: bool,
    conversions: &[ConvOp], iflags: &[IoFlag], oflags: &[IoFlag],
) {
    let input = input_path.unwrap_or("stdin");
    let output = output_path.unwrap_or("stdout");
    let total_bytes = count.map(|c| if count_bytes { c } else { c * ibs });
    let skip_actual = if skip_bytes { skip } else { skip * ibs };
    let seek_actual = if seek_bytes { seek } else { seek * ibs };

    let risk = safety::assess_risk(
        output_path.map(Path::new), input_path.map(Path::new),
        count, ibs, conversions,
    );

    // ── Header ──
    println!("\n{BOLD}{CYAN}  dd-rs — Command Explanation{RST}\n");

    // ── Summary line ──
    let risk_icon = match risk.level {
        RiskLevel::Safe => "🟢",
        RiskLevel::Caution => "🟡",
        RiskLevel::Dangerous => "🟠",
        RiskLevel::Catastrophic => "🔴",
    };
    let input_disp = if input.len() > 40 { format!("…{}", &input[input.len().saturating_sub(37)..]) } else { input.to_string() };
    let output_disp = if output.len() > 40 { format!("…{}", &output[output.len().saturating_sub(37)..]) } else { output.to_string() };
    println!("  {BOLD}{input_disp}{RST} {DIM}→{RST} {BOLD}{output_disp}{RST}    {risk_icon} Risk: {}/100", risk.score);
    if let Some(tb) = total_bytes {
        println!("  {DIM}{} total{RST}", format_size(tb));
    }
    println!();

    // ── Operands ──
    println!("{BOLD}▸ Operands{RST}");
    println!("  {DIM}──┬──{RST}");
    operand("Input", &format!("if={}", input));
    operand("Output", &format!("of={}", output));
    operand("Block size", &format!("{}  {}", format_size_simple(ibs), if ibs < 4096 { format!("{YELLOW}(very small — try --auto-tune){RST}") } else { String::new() }));
    if obs != ibs { operand("Output block", &format_size_simple(obs)); }
    match count {
        Some(c) => {
            let label = if count_bytes { "bytes" } else { "blocks" };
            operand("Count", &format!("{} {}  {DIM}({}){RST}", c, label, total_bytes.map(|t| format_size(t)).unwrap_or_default()));
        }
        None => { operand("Count", &format!("{YELLOW}unlimited — copies until EOF{RST}")); }
    }
    if skip > 0 { operand("Skip", &format!("{} ({})", format_size_simple(skip_actual), if skip_bytes { "bytes" } else { "blocks" })); }
    if seek > 0 { operand("Seek", &format!("{} ({})", format_size_simple(seek_actual), if seek_bytes { "bytes" } else { "blocks" })); }
    println!();

    // ── Conversions ──
    if !conversions.is_empty() {
        println!("{BOLD}▸ Conversions{RST}");
        for conv in conversions {
            if conv.is_data_conversion() {
                let (name, desc) = describe_conversion(conv);
                println!("  {CYAN}{name:<10}{RST}  {DIM}{desc}{RST}");
            }
        }
        println!("  {DIM}Pipeline order: ebcdic → block/unblock → case → swab → sync{RST}");
        println!();
    }

    // ── I/O Flags ──
    if !iflags.is_empty() || !oflags.is_empty() {
        println!("{BOLD}▸ I/O Flags{RST}");
        for f in iflags { let (n, d) = describe_io_flag(f); println!("  {CYAN}iflag={n:<10}{RST} {DIM}{d}{RST}"); }
        for f in oflags { let (n, d) = describe_io_flag(f); println!("  {CYAN}oflag={n:<10}{RST} {DIM}{d}{RST}"); }
        println!();
    }

    // ── Data Flow ──
    println!("{BOLD}▸ Data Flow{RST}");
    if conversions.iter().any(|c| c.is_data_conversion()) {
        println!("  input  →  [{} conversions]  →  output", conversions.iter().filter(|c| c.is_data_conversion()).count());
    } else {
        println!("  input  →  {GREEN}pass-through (zero-copy){RST}  →  output");
    }
    if let Some(tb) = total_bytes {
        println!("  {DIM}{}  ·  {} blocks of {}{RST}", format_size(tb), (tb + ibs - 1) / ibs, format_size_simple(ibs));
    }
    println!();

    // ── Risk Assessment ──
    println!("{BOLD}▸ Risk Assessment{RST}  {risk_icon}  {}/100  {}{:?}{RST}", risk.score, risk_color(&risk), risk.level);
    if risk.warnings.is_empty() && risk.mitigations.is_empty() {
        println!("  {GREEN}No concerns — this operation looks safe.{RST}");
    }
    for w in &risk.warnings {
        println!("  {YELLOW}⚠{RST}  {}", w);
    }
    for m in &risk.mitigations {
        println!("  {DIM}→{RST}  {}", m);
    }
    println!();

    // ── Estimated Outcome ──
    println!("{BOLD}▸ Estimated{RST}");
    if let Some(tb) = total_bytes {
        println!("  Total data  {BOLD}{}{RST}", format_size(tb));
        let blocks = (tb + ibs - 1) / ibs;
        println!("  I/O calls   {DIM}~{} read + ~{} write{RST}", blocks, blocks);
        let hdd = tb as f64 / 150_000_000.0;
        let ssd = tb as f64 / 500_000_000.0;
        let nvme = tb as f64 / 3_000_000_000.0;
        println!("  Time est.   {DIM}HDD: {:.1}s  ·  SSD: {:.1}s  ·  NVMe: {:.1}s{RST}", hdd, ssd, nvme);
    } else {
        println!("  {YELLOW}Unbounded — no count specified, will copy until EOF{RST}");
        println!("  {DIM}Add count=N to set a limit{RST}");
    }
    println!();
}

// ── Helpers ──

fn operand(label: &str, value: &str) {
    println!("  {DIM}{:<12}{RST} {}", label, value);
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 { format!("{:.2} GB", bytes as f64 / 1_000_000_000.0) }
    else if bytes >= 1_000_000 { format!("{:.2} MB", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.2} kB", bytes as f64 / 1_000.0) }
    else { format!("{} B", bytes) }
}

fn format_size_simple(bytes: u64) -> String {
    if bytes >= 1_048_576 && bytes % 1_048_576 == 0 { format!("{}M", bytes / 1_048_576) }
    else if bytes >= 1_024 && bytes % 1_024 == 0 { format!("{}K", bytes / 1_024) }
    else { format!("{}B", bytes) }
}

fn risk_color(risk: &RiskAssessment) -> &'static str {
    match risk.level {
        RiskLevel::Safe => GREEN,
        RiskLevel::Caution => YELLOW,
        RiskLevel::Dangerous => RED,
        RiskLevel::Catastrophic => RED,
    }
}

fn describe_conversion(conv: &ConvOp) -> (&'static str, &'static str) {
    match conv {
        ConvOp::Ascii     => ("ascii",     "EBCDIC → ASCII (implies unblock)"),
        ConvOp::Ebcdic    => ("ebcdic",    "ASCII → EBCDIC CP037 (implies block)"),
        ConvOp::Ibm       => ("ibm",       "ASCII → IBM1047 alternate EBCDIC"),
        ConvOp::Block     => ("block",     "pad newline records to cbs with spaces"),
        ConvOp::Unblock   => ("unblock",   "trailing spaces → newline"),
        ConvOp::Lcase     => ("lcase",     "A-Z → a-z"),
        ConvOp::Ucase     => ("ucase",     "a-z → A-Z"),
        ConvOp::Swab      => ("swab",      "swap every byte pair (endianness)"),
        ConvOp::Sync      => ("sync",      "pad short blocks with NULs to ibs"),
        ConvOp::Sparse    => ("sparse",    "seek over NUL blocks (sparse files)"),
        ConvOp::Noerror   => ("noerror",   "continue after read errors"),
        ConvOp::Notrunc   => ("notrunc",   "do not truncate output"),
        ConvOp::Excl      => ("excl",      "fail if output exists"),
        ConvOp::Nocreat   => ("nocreat",   "fail if output missing"),
        ConvOp::Fdatasync => ("fdatasync", "sync data before exit"),
        ConvOp::Fsync     => ("fsync",     "sync data+metadata before exit"),
    }
}

fn describe_io_flag(flag: &IoFlag) -> (&'static str, &'static str) {
    match flag {
        IoFlag::Append    => ("append",    "open in append mode"),
        IoFlag::Direct    => ("direct",    "bypass kernel cache (Linux)"),
        IoFlag::Directory => ("directory", "fail if not a directory"),
        IoFlag::Dsync     => ("dsync",     "sync data per write (Linux)"),
        IoFlag::Sync      => ("sync",      "sync data+metadata per write (Linux)"),
        IoFlag::Nonblock  => ("nonblock",  "non-blocking I/O"),
        IoFlag::Noatime   => ("noatime",   "don't update access time (Linux)"),
        IoFlag::Nocache   => ("nocache",   "drop cache after I/O (Linux)"),
        IoFlag::Noctty    => ("noctty",    "no controlling terminal"),
        IoFlag::Nofollow  => ("nofollow",  "don't follow symlinks"),
        IoFlag::Fullblock => ("fullblock", "accumulate full blocks (pipes)"),
    }
}
