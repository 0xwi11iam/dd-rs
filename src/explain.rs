/// Command explanation engine — teaches you what a dd-rs command will do
/// before it runs, including a full risk assessment.
///
/// Triggered by `--explain` (or `-E`):
///
/// ```bash
/// $ dd-rs if=/dev/zero of=test.bin bs=1M count=10 --explain
/// $ dd-rs --explain if=/dev/sda of=/dev/sdb bs=4M
/// ```
///
/// The explanation covers:
/// 1. **Command breakdown** — each operand explained in plain English
/// 2. **Data flow diagram** — ASCII art showing what happens
/// 3. **Risk assessment** — scored danger level with specific warnings
/// 4. **Estimated outcome** — how much data, how long it'll take, what the result looks like
/// 5. **Safer alternatives** — suggestions if the command is dangerous

use std::path::Path;

use crate::conv::ConvOp;
use crate::flags::IoFlag;
use crate::safety::{self, RiskAssessment, RiskLevel};

// =============================================================================
// Explanation output
// =============================================================================

/// Full explanation of a command.
pub struct CommandExplanation {
    pub input: String,
    pub output: String,
    pub block_size: u64,
    pub count: Option<u64>,
    pub skip: u64,
    pub seek: u64,
    pub conversions: Vec<ConvOp>,
    pub iflags: Vec<IoFlag>,
    pub oflags: Vec<IoFlag>,
    pub risk_assessment: RiskAssessment,
}

/// Generate and print a full explanation of what a command will do.
pub fn explain(
    input_path: Option<&str>,
    output_path: Option<&str>,
    ibs: u64,
    obs: u64,
    count: Option<u64>,
    skip: u64,
    seek: u64,
    count_bytes: bool,
    skip_bytes: bool,
    seek_bytes: bool,
    conversions: &[ConvOp],
    iflags: &[IoFlag],
    oflags: &[IoFlag],
) {
    let input = input_path.unwrap_or("stdin");
    let output = output_path.unwrap_or("stdout");

    let total_bytes = count.map(|c| if count_bytes { c } else { c * ibs });
    let skip_actual = if skip_bytes { skip } else { skip * ibs };
    let seek_actual = if seek_bytes { seek } else { seek * ibs };

    // ---- HEADER ----
    print_header(input, output);

    // ---- SECTION 1: Operand breakdown ----
    print_operand_breakdown(
        input, output, ibs, obs, count, total_bytes,
        skip, seek, skip_bytes, seek_bytes, count_bytes,
        skip_actual, seek_actual,
    );

    // ---- SECTION 2: Conversions ----
    if !conversions.is_empty() {
        print_conversions(conversions);
    }

    // ---- SECTION 3: I/O flags ----
    if !iflags.is_empty() || !oflags.is_empty() {
        print_io_flags(iflags, oflags);
    }

    // ---- SECTION 4: Data flow diagram ----
    print_data_flow(input, output, ibs, obs, total_bytes, skip_actual, seek_actual, conversions);

    // ---- SECTION 5: Risk Assessment ----
    let risk = safety::assess_risk(
        output_path.map(Path::new),
        input_path.map(Path::new),
        count,
        ibs,
        conversions,
    );
    print_risk_assessment(&risk);

    // ---- SECTION 6: Estimated outcome ----
    print_estimated_outcome(total_bytes, ibs, obs, count);

    // ---- SECTION 7: Safety suggestions ----
    print_safety_suggestions(&risk, output, total_bytes, count);
}

// =============================================================================
// Section renderers
// =============================================================================

fn print_header(input: &str, output: &str) {
    println!(
        "\n╔══════════════════════════════════════════════════════════════╗\n\
           ║               DD-RS COMMAND EXPLANATION                      ║\n\
           ╠══════════════════════════════════════════════════════════════╣\n\
           ║  {} → {}\n\
           ╚══════════════════════════════════════════════════════════════╝\n",
        if input.len() > 35 { format!("...{}", &input[input.len().saturating_sub(35)..]) } else { format!("{:>35}", input) },
        if output.len() > 35 { format!("...{}", &output[output.len().saturating_sub(35)..]) } else { format!("{:>35}", output) },
    );
}

fn print_operand_breakdown(
    input: &str, output: &str,
    ibs: u64, obs: u64,
    count: Option<u64>, total_bytes: Option<u64>,
    skip: u64, seek: u64,
    skip_bytes: bool, seek_bytes: bool, count_bytes: bool,
    skip_actual: u64, seek_actual: u64,
) {
    println!("┌─ OPERANDS ─────────────────────────────────────────────────┐");

    println!("│ if={:<52} │", trunc(input, 52));
    println!("│   → Read input from this file (or stdin if omitted)        │");

    println!("│ of={:<52} │", trunc(output, 52));
    println!("│   → Write output to this file (or stdout if omitted)       │");

    println!("│ ibs={:<51} │", format_size(ibs));
    println!("│   → Read in chunks of this size each syscall               │");
    if ibs < 4096 {
        println!("│   ⚠ VERY SMALL — causes millions of syscalls/second        │");
        println!("│     Try --auto-tune or set bs=128K or larger               │");
    }

    if obs != ibs {
        println!("│ obs={:<51} │", format_size(obs));
        println!("│   → Write in chunks of this size (differs from read size)  │");
    }

    match count {
        Some(c) => {
            let label = if count_bytes { "bytes" } else { "blocks" };
            println!("│ count={} {}                                             │", c, label);
            if let Some(tb) = total_bytes {
                println!("│   → Copy exactly {} of data                     │", format_size(tb));
            }
            if count_bytes {
                println!("│   → Count is in BYTES (GNU 'B' suffix mode)               │");
            }
        }
        None => {
            println!("│ count=(unlimited)                                          │");
            println!("│   → Copy until EOF on input                                │");
            if output != "stdout" && !output.starts_with("/dev/null") {
                println!("│   ⚠ No limit set — will copy ENTIRE input                 │");
            }
        }
    }

    if skip > 0 {
        let label = if skip_bytes { "bytes" } else { "blocks" };
        println!("│ skip={} {} (={})                               │", skip, label, format_size(skip_actual));
        println!("│   → Skip this much data from the START of input            │");
    }

    if seek > 0 {
        let label = if seek_bytes { "bytes" } else { "blocks" };
        println!("│ seek={} {} (={})                               │", seek, label, format_size(seek_actual));
        println!("│   → Start writing this far into the output file            │");
        if output != "stdout" {
            println!("│   ⚠ Will OVERWRITE data starting at this offset            │");
        }
    }

    println!("└────────────────────────────────────────────────────────────┘\n");
}

fn print_conversions(conversions: &[ConvOp]) {
    println!("┌─ CONVERSIONS (conv=) ──────────────────────────────────────┐");

    for conv in conversions {
        let (name, desc) = describe_conversion(conv);
        println!("│ {:<12} → {:<45} │", name, desc);
    }

    println!("│                                                            │");
    println!("│ Conversion pipeline order (GNU dd canonical):              │");
    println!("│   ebcdic/ascii/ibm → block/unblock → lcase/ucase → swab → sync │");
    println!("└────────────────────────────────────────────────────────────┘\n");
}

fn print_io_flags(iflags: &[IoFlag], oflags: &[IoFlag]) {
    println!("┌─ I/O FLAGS ────────────────────────────────────────────────┐");

    for flag in iflags {
        let (name, desc) = describe_io_flag(flag);
        println!("│ iflag={:<8} → {:<41} │", name, desc);
    }
    for flag in oflags {
        let (name, desc) = describe_io_flag(flag);
        println!("│ oflag={:<8} → {:<41} │", name, desc);
    }

    println!("└────────────────────────────────────────────────────────────┘\n");
}

fn print_data_flow(
    _input: &str, _output: &str,
    ibs: u64, _obs: u64,
    total_bytes: Option<u64>,
    _skip_actual: u64, _seek_actual: u64,
    conversions: &[ConvOp],
) {
    println!("┌─ DATA FLOW ────────────────────────────────────────────────┐");
    println!("│                                                            │");
    println!("│   INPUT                    OUTPUT                          │");
    println!("│   ┌──────┐    ┌──────┐    ┌──────┐                        │");
    println!("│   │ Disk │───▶│ Read │───▶│Write │───▶ Target             │");
    println!("│   │  or  │    │ ibs  │    │ obs  │                        │");
    println!("│   │ stdin│    └──────┘    └──────┘                        │");
    println!("│   └──────┘         │                                      │");
    println!("│                    ▼                                      │");
    if conversions.is_empty() {
        println!("│              ┌──────────┐                                  │");
        println!("│              │ NO CONV  │  (zero-copy fast path!)          │");
        println!("│              │ PASS-THRU│  kernel copies FD→FD directly    │");
        println!("│              └──────────┘                                  │");
    } else {
        println!("│          ┌───────────────┐                                 │");
        println!("│          │ CONV PIPELINE │                                 │");
        for conv in conversions {
            if conv.is_data_conversion() {
                println!("│          │  ├─ {:<10} │                                 │", format!("{:?}", conv));
            }
        }
        println!("│          └───────────────┘                                 │");
    }

    if let Some(tb) = total_bytes {
        let blocks = (tb + ibs - 1) / ibs;
        println!("│                                                            │");
        println!("│   Estimated: {} blocks × {} = {} total           │",
            blocks, format_size(ibs), format_size(tb));
    }

    println!("└────────────────────────────────────────────────────────────┘\n");
}

fn print_risk_assessment(risk: &RiskAssessment) {
    let symbol = match risk.level {
        RiskLevel::Safe => "✅",
        RiskLevel::Caution => "⚠️ ",
        RiskLevel::Dangerous => "🔶",
        RiskLevel::Catastrophic => "☠️ ",
    };

    println!("┌─ RISK ASSESSMENT ──────────────────────────────────────────┐");
    println!("│  {} Score: {}/100  Level: {:<32} │",
        symbol, risk.score, format!("{:?}", risk.level));

    if !risk.warnings.is_empty() {
        println!("│                                                            │");
        println!("│  WARNINGS:                                                 │");
        for warning in &risk.warnings {
            // Word-wrap long warnings
            for line in wrap_text(warning, 58) {
                println!("│    {}", line);
            }
        }
    }

    if !risk.mitigations.is_empty() {
        println!("│                                                            │");
        println!("│  HOW TO MAKE THIS SAFER:                                   │");
        for mitigation in &risk.mitigations {
            for line in wrap_text(mitigation, 58) {
                println!("│    → {}", line);
            }
        }
    }

    println!("└────────────────────────────────────────────────────────────┘\n");
}

fn print_estimated_outcome(total_bytes: Option<u64>, ibs: u64, _obs: u64, _count: Option<u64>) {
    println!("┌─ ESTIMATED OUTCOME ────────────────────────────────────────┐");

    if let Some(tb) = total_bytes {
        println!("│  Data to transfer:  {}", format_size(tb));

        // Rough throughput estimate
        let hdd_speed: f64 = 150_000_000.0; // 150 MB/s typical HDD
        let ssd_speed: f64 = 500_000_000.0; // 500 MB/s typical SATA SSD
        let nvme_speed: f64 = 3_000_000_000.0; // 3 GB/s typical NVMe

        let hdd_secs = tb as f64 / hdd_speed;
        let ssd_secs = tb as f64 / ssd_speed;
        let nvme_secs = tb as f64 / nvme_speed;

        println!("│  Estimated time:");
        println!("│    HDD (150 MB/s):   {:>6.1}s  ({})", hdd_secs, format_duration(hdd_secs));
        println!("│    SSD (500 MB/s):   {:>6.1}s  ({})", ssd_secs, format_duration(ssd_secs));
        println!("│    NVMe (3 GB/s):    {:>6.1}s  ({})", nvme_secs, format_duration(nvme_secs));

        // Blocks estimate
        let blocks = (tb + ibs - 1) / ibs;
        println!("│  I/O operations:    {} reads + {} writes", blocks, blocks);
        if ibs < 4096 {
            println!("│  ⚠ {} syscalls is excessive for {} data", blocks, format_size(tb));
        }
    } else {
        println!("│  Data to transfer:  UNKNOWN (no count specified)");
        println!("│  → Will copy until input is exhausted");
        println!("│  → Could be anything from 0 bytes to petabytes");
    }

    println!("└────────────────────────────────────────────────────────────┘\n");
}

fn print_safety_suggestions(risk: &RiskAssessment, output: &str, total_bytes: Option<u64>, count: Option<u64>) {
    if risk.level == RiskLevel::Safe {
        return;
    }

    println!("┌─ SAFETY SUGGESTIONS ───────────────────────────────────────┐");

    let mut suggestions: Vec<String> = Vec::new();

    if risk.level >= RiskLevel::Dangerous {
        suggestions.push("Run with --dry-run first to validate all arguments".into());
        suggestions.push("Double-check of= — is this really the right target?".into());
    }

    if count.is_none() && output != "stdout" && !output.starts_with("/dev/null") {
        suggestions.push("Add count=N to limit how much data is written".into());
        suggestions.push(format!(
            "  Example: add count=100 to write only {} (100 blocks)",
            format_size(100 * 512)
        ));
    }

    if total_bytes.is_none() && risk.level >= RiskLevel::Caution {
        suggestions.push("The input size is unknown — the transfer could be unbounded".into());
    }

    if output.starts_with("/dev/") && !output.starts_with("/dev/null") && !output.starts_with("/dev/zero") {
        suggestions.push(format!(
            "Writing to '{}' — is this definitely correct?",
            output
        ));
        suggestions.push("Use --force to bypass all safety checks (not recommended)".into());
    }

    for s in &suggestions {
        for line in wrap_text(s, 58) {
            println!("│  {}", line);
        }
    }

    println!("└────────────────────────────────────────────────────────────┘\n");
}

// =============================================================================
// Helpers
// =============================================================================

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB ({} bytes)", bytes as f64 / 1_000_000_000.0, bytes)
    } else if bytes >= 1_000_000 {
        format!("{:.2} MB ({} bytes)", bytes as f64 / 1_000_000.0, bytes)
    } else if bytes >= 1_000 {
        format!("{:.2} kB ({} bytes)", bytes as f64 / 1_000.0, bytes)
    } else {
        format!("{} bytes", bytes)
    }
}

fn format_duration(secs: f64) -> String {
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else if secs < 60.0 {
        format!("{:.1}s", secs)
    } else if secs < 3600.0 {
        format!("{}m{:.0}s", secs as u64 / 60, secs % 60.0)
    } else {
        format!("{}h{}m", secs as u64 / 3600, (secs as u64 % 3600) / 60)
    }
}

fn trunc(s: &str, max: usize) -> String {
    if s.len() <= max {
        format!("{:<max$}", s, max = max)
    } else {
        format!("...{}", &s[s.len() - (max - 3)..])
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut current = String::new();

    for word in words {
        if current.len() + word.len() + 1 > width {
            if !current.is_empty() {
                lines.push(current.clone());
                current.clear();
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(text.to_string());
    }
    lines
}

fn describe_conversion(conv: &ConvOp) -> (&'static str, &'static str) {
    match conv {
        ConvOp::Ascii     => ("ascii",    "Convert EBCDIC → ASCII (implies unblock)"),
        ConvOp::Ebcdic    => ("ebcdic",   "Convert ASCII → EBCDIC (implies block)"),
        ConvOp::Ibm       => ("ibm",      "Convert ASCII → IBM1047 alternate EBCDIC"),
        ConvOp::Block     => ("block",    "Pad newline-terminated records with spaces to cbs"),
        ConvOp::Unblock   => ("unblock",  "Replace trailing spaces in cbs blocks with newlines"),
        ConvOp::Lcase     => ("lcase",    "Map uppercase A-Z to lowercase a-z"),
        ConvOp::Ucase     => ("ucase",    "Map lowercase a-z to uppercase A-Z"),
        ConvOp::Swab      => ("swab",     "Swap every pair of bytes (endianness)"),
        ConvOp::Sync      => ("sync",     "Pad short input blocks with NULs to ibs size"),
        ConvOp::Sparse    => ("sparse",   "Seek instead of writing NUL blocks (sparse files)"),
        ConvOp::Noerror   => ("noerror",  "Continue after read errors instead of aborting"),
        ConvOp::Notrunc   => ("notrunc",  "Do NOT truncate output file before writing"),
        ConvOp::Excl      => ("excl",     "Fail if output file already exists"),
        ConvOp::Nocreat   => ("nocreat",  "Fail if output file does NOT already exist"),
        ConvOp::Fdatasync => ("fdatasync","Force physical write of data before exit"),
        ConvOp::Fsync     => ("fsync",    "Force physical write of data+metadata before exit"),
    }
}

fn describe_io_flag(flag: &IoFlag) -> (&'static str, &'static str) {
    match flag {
        IoFlag::Append    => ("append",   "Open output in append mode (don't overwrite)"),
        IoFlag::Direct    => ("direct",   "Bypass kernel buffer cache (direct I/O)"),
        IoFlag::Directory => ("directory","Fail if target is not a directory"),
        IoFlag::Dsync     => ("dsync",    "Synchronized I/O for data on every write"),
        IoFlag::Sync      => ("sync",     "Synchronized I/O for data+metadata every write"),
        IoFlag::Nonblock  => ("nonblock", "Use non-blocking I/O"),
        IoFlag::Noatime   => ("noatime",  "Do not update file access time"),
        IoFlag::Nocache   => ("nocache",  "Request kernel to drop cache after I/O"),
        IoFlag::Noctty    => ("noctty",   "Do not assign controlling terminal"),
        IoFlag::Nofollow  => ("nofollow", "Do not follow symbolic links"),
        IoFlag::Fullblock => ("fullblock","Accumulate full input blocks (critical for pipes)"),
    }
}
