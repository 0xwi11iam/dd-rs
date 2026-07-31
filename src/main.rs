/// dd-rs — a safe, modern Rust+C alternative to dd.
///
/// Entry point for the `dd-rs` binary.
///
/// ## Dual syntax support
///
/// dd-rs accepts BOTH legacy dd syntax AND modern human-friendly commands:
///
/// ```bash
/// # Legacy dd syntax (100% compatible):
/// dd-rs if=/dev/zero of=test.bin bs=1M count=10
/// dd-rs if=input of=output conv=swab status=progress
///
/// # Modern friendly subcommands:
/// dd-rs copy /dev/zero test.bin --size 1M --count 10
/// dd-rs zero disk.img --size 1G
/// dd-rs random key.bin --bytes 32
/// dd-rs info /dev/sda
/// dd-rs explain if=/dev/zero of=/dev/sda
/// ```
///
/// The pre-processor converts `key=value` pairs into clap-compatible
/// `--key value` flags before argument parsing.

use std::env;
use std::process;
use std::path::Path;

use clap::Parser;
use dd_rs::args::{self, CliArgs};
use dd_rs::conv::ConversionPipeline;
use dd_rs::flags;
use dd_rs::io_engine::{self, EngineConfig};
use dd_rs::safety;
use dd_rs::signal;

// =============================================================================
// DD-style key=value argument pre-processor
// =============================================================================

/// Convert dd-style `key=value` arguments to clap-style `--key value`.
fn preprocess_dd_args(raw_args: &[String]) -> Vec<String> {
    let mut processed = vec![raw_args[0].clone()];

    for arg in &raw_args[1..] {
        if arg.starts_with("--") {
            processed.push(arg.clone());
            continue;
        }
        if arg.starts_with('-') && !arg.contains('=') {
            processed.push(arg.clone());
            continue;
        }
        if arg.starts_with("-if=") {
            processed.push("--if".to_string());
            processed.push(arg[4..].to_string());
            continue;
        }
        if arg.starts_with("-of=") {
            processed.push("--of".to_string());
            processed.push(arg[4..].to_string());
            continue;
        }

        if let Some(eq_pos) = arg.find('=') {
            let key = &arg[..eq_pos];
            let value = &arg[eq_pos + 1..];
            let flag = match key {
                "if" => "--if", "of" => "--of",
                "ibs" => "--ibs", "obs" => "--obs", "bs" => "--bs", "cbs" => "--cbs",
                "count" => "--count", "skip" => "--skip", "seek" => "--seek",
                "iseek" => "--iseek", "oseek" => "--oseek",
                "status" => "--status", "conv" => "--conv",
                "iflag" => "--iflag", "oflag" => "--oflag",
                _ => { processed.push(arg.clone()); continue; }
            };
            processed.push(flag.to_string());
            processed.push(value.to_string());
        } else {
            processed.push(arg.clone());
        }
    }
    processed
}

// =============================================================================
// Subcommand detection
// =============================================================================

#[derive(Debug, PartialEq, Eq)]
enum Subcommand { Copy, Zero, Random, Info, Explain, Wipe, Legacy }

fn detect_subcommand(args: &[String]) -> Subcommand {
    for arg in &args[1..] {
        if arg.starts_with('-') { continue; }
        match arg.as_str() {
            "copy" => return Subcommand::Copy,
            "zero" => return Subcommand::Zero,
            "random" => return Subcommand::Random,
            "info" => return Subcommand::Info,
            "explain" => return Subcommand::Explain,
            "wipe" => return Subcommand::Wipe,
            _ => return Subcommand::Legacy,
        }
    }
    Subcommand::Legacy
}

// =============================================================================
// Shared flag parser for subcommands
// =============================================================================

fn parse_subcommand_args(raw_args: &[String], cmd: &str) -> (Option<String>, Option<String>, Vec<(String, String)>) {
    let mut pos1 = None;
    let mut pos2 = None;
    let mut flags: Vec<(String, String)> = Vec::new();
    let mut past_cmd = false;
    let mut i = 0;

    while i < raw_args.len() {
        let arg = &raw_args[i];
        if !past_cmd {
            if arg == cmd { past_cmd = true; }
            i += 1; continue;
        }
        if arg.starts_with("--") {
            let flag = arg[2..].to_string();
            if i + 1 < raw_args.len() && !raw_args[i + 1].starts_with("--") {
                flags.push((flag, raw_args[i + 1].clone()));
                i += 2;
            } else {
                flags.push((flag, String::new()));
                i += 1;
            }
        } else if arg.starts_with('-') && arg.len() == 2 {
            let flag = arg[1..].to_string();
            if i + 1 < raw_args.len() && !raw_args[i + 1].starts_with('-') {
                flags.push((flag, raw_args[i + 1].clone()));
                i += 2;
            } else {
                flags.push((flag, String::new()));
                i += 1;
            }
        } else if pos1.is_none() {
            pos1 = Some(arg.clone()); i += 1;
        } else if pos2.is_none() {
            pos2 = Some(arg.clone()); i += 1;
        } else {
            i += 1;
        }
    }
    (pos1, pos2, flags)
}

fn get_flag(flags: &[(String, String)], names: &[&str], default: &str) -> String {
    for (k, v) in flags { if names.contains(&k.as_str()) { return v.clone(); } }
    default.to_string()
}

fn get_flag_opt(flags: &[(String, String)], names: &[&str]) -> Option<String> {
    for (k, v) in flags { if names.contains(&k.as_str()) { return Some(v.clone()); } }
    None
}

// =============================================================================
// Subcommand implementations
// =============================================================================

fn cmd_copy(raw_args: &[String]) {
    let (input, output, flags) = parse_subcommand_args(raw_args, "copy");
    let input = input.unwrap_or_else(|| {
        eprintln!("dd-rs copy: missing input\nUsage: dd-rs copy <INPUT> <OUTPUT> [--size SIZE] [--count N]");
        process::exit(1);
    });
    let output = output.unwrap_or_else(|| {
        eprintln!("dd-rs copy: missing output\nUsage: dd-rs copy <INPUT> <OUTPUT> [--size SIZE] [--count N]");
        process::exit(1);
    });
    let bs = get_flag(&flags, &["bs", "block-size", "size"], "1M");
    let mut dd_args = vec!["dd-rs".to_string(), format!("if={}", input), format!("of={}", output),
        format!("bs={}", bs), "--status".to_string(), "progress".to_string()];
    if let Some(c) = get_flag_opt(&flags, &["count"]) { dd_args.push(format!("count={}", c)); }
    if get_flag_opt(&flags, &["explain", "E"]).is_some() { dd_args.push("--explain".to_string()); }
    if get_flag_opt(&flags, &["yes", "y"]).is_some() { dd_args.push("--yes".to_string()); }
    if get_flag_opt(&flags, &["force"]).is_some() { dd_args.push("--force".to_string()); }
    run_legacy_or_exit(&preprocess_dd_args(&dd_args));
}

fn cmd_zero(raw_args: &[String]) {
    let (output, _, flags) = parse_subcommand_args(raw_args, "zero");
    let output = output.unwrap_or_else(|| {
        eprintln!("dd-rs zero: missing output\nUsage: dd-rs zero <OUTPUT> --size SIZE");
        process::exit(1);
    });
    let size = get_flag(&flags, &["size", "bytes", "count"], "");
    if size.is_empty() { eprintln!("dd-rs zero: --size is required\nExample: dd-rs zero disk.img --size 1G"); process::exit(1); }
    let bs = get_flag(&flags, &["bs", "block-size"], "1M");
    run_legacy_or_exit(&preprocess_dd_args(&vec!["dd-rs".to_string(), "if=/dev/zero".to_string(),
        format!("of={}", output), format!("bs={}", bs), format!("count={}", size),
        "--status".to_string(), "progress".to_string()]));
}

fn cmd_random(raw_args: &[String]) {
    let (output, _, flags) = parse_subcommand_args(raw_args, "random");
    let output = output.unwrap_or_else(|| {
        eprintln!("dd-rs random: missing output\nUsage: dd-rs random <OUTPUT> --bytes SIZE");
        process::exit(1);
    });
    let size = get_flag(&flags, &["bytes", "size", "count"], "");
    if size.is_empty() { eprintln!("dd-rs random: --bytes is required\nExample: dd-rs random key.bin --bytes 32"); process::exit(1); }
    run_legacy_or_exit(&preprocess_dd_args(&vec!["dd-rs".to_string(), "if=/dev/urandom".to_string(),
        format!("of={}", output), format!("bs=1"), format!("count={}", size),
        "--status".to_string(), "progress".to_string()]));
}

fn cmd_info(raw_args: &[String]) {
    let (path, _, _flags) = parse_subcommand_args(raw_args, "info");
    let path = path.unwrap_or_else(|| {
        eprintln!("dd-rs info: missing path\nUsage: dd-rs info <DEVICE|FILE>");
        process::exit(1);
    });
    let path = Path::new(&path);
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    DD-RS DEVICE INFO                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
    match safety::inspect_output_target(path) {
        Ok(info) => {
            println!("  Path:        {}", info.path.display());
            println!("  Type:        {:?}", info.device_type);
            println!("  Size:        {} bytes ({:.2} GB)", info.size_bytes, info.size_bytes as f64 / 1_000_000_000.0);
            println!("  Mounted:     {}", if info.is_mounted {
                format!("YES ⚠  ({})", info.mount_points.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))
            } else { "no".to_string() });
            println!("  Raw disk:    {}", if safety::is_raw_disk_device(path) { "YES ☠️" } else { "no" });
            let risk = safety::assess_risk(Some(path), None, None, 512, &[]);
            println!("\n  Risk:        {}/100 ({:?})", risk.score, risk.level);
            for w in &risk.warnings { println!("    ⚠ {}", w); }
            for m in &risk.mitigations { println!("    → {}", m); }
        }
        Err(e) => { eprintln!("  Error: {}", e); process::exit(1); }
    }
}

fn cmd_explain(raw_args: &[String]) {
    let dd_args: Vec<String> = raw_args.iter().skip_while(|a| a.as_str() != "explain").skip(1).cloned().collect();
    if dd_args.is_empty() {
        eprintln!("dd-rs explain: missing command\nUsage: dd-rs explain if=/dev/zero of=test.bin bs=1M count=10");
        process::exit(1);
    }
    let mut full = vec!["dd-rs".to_string()];
    full.extend(dd_args);
    full.push("--explain".to_string());
    run_legacy_or_exit(&preprocess_dd_args(&full));
}

fn cmd_wipe(raw_args: &[String]) {
    let (device, _, flags) = parse_subcommand_args(raw_args, "wipe");
    let device = device.unwrap_or_else(|| {
        eprintln!("dd-rs wipe: missing device\nUsage: dd-rs wipe <DEVICE> [--passes N]");
        process::exit(1);
    });
    let bs = get_flag(&flags, &["bs", "block-size"], "4M");
    eprintln!("dd-rs wipe: This will DESTROY ALL DATA on '{}'", device);
    run_legacy_or_exit(&preprocess_dd_args(&vec!["dd-rs".to_string(), "if=/dev/zero".to_string(),
        format!("of={}", device), format!("bs={}", bs), "--status".to_string(), "progress".to_string()]));
}

// =============================================================================
// Legacy engine runner (shared by main and subcommands)
// Returns Ok(exit_code) on success or Err on failure.
// =============================================================================

fn run_legacy(processed_args: &[String]) -> Result<i32, dd_rs::Error> {
    let cli_args = CliArgs::try_parse_from(processed_args)
        .map_err(|e| dd_rs::Error::InvalidArgument(e.to_string()))?;

    // Suppress advisory warnings when in explain mode
    let is_explain = processed_args.iter().any(|a| a == "--explain" || a == "-E")
        || cli_args.explain;
    let config = args::resolve_config(cli_args, is_explain)?;

    if config.explain {
        dd_rs::explain::explain(
            config.input_path.as_deref(), config.output_path.as_deref(),
            config.ibs, config.obs, config.count, config.skip, config.seek,
            config.count_bytes, config.skip_bytes, config.seek_bytes,
            &config.conv, &config.iflags, &config.oflags,
        );
        return Ok(0);
    }

    if config.dry_run {
        println!("Dry run — configuration validated.");
        println!("  Input: {}  Output: {}  BS: {}/{}  Count: {:?}",
            config.input_path.as_deref().unwrap_or("stdin"),
            config.output_path.as_deref().unwrap_or("stdout"),
            config.ibs, config.obs, config.count);
        return Ok(0);
    }

    // Safety check
    if let Some(ref out_path) = config.output_path {
        let out_path = Path::new(out_path);
        if let Ok(device_info) = safety::inspect_output_target(out_path) {
            let input_size_hint = safety::estimate_input_size(
                config.input_path.as_deref().map(Path::new), config.count, config.ibs);
            match safety::check_output_safety(&device_info, config.safety_level, input_size_hint) {
                Ok(safety::SafetyDecision::Safe | safety::SafetyDecision::Confirmed) => {}
                Ok(safety::SafetyDecision::WarningIssued) => eprintln!("dd-rs: proceeding with warnings.\n"),
                Ok(safety::SafetyDecision::Blocked { reason }) => {
                    return Err(dd_rs::Error::Other(format!(
                        "SAFETY BLOCK: {}\nUse --yes or --force to bypass.", reason
                    )));
                }
                Err(e) => return Err(e),
            }
        }
    }

    let _ = signal::install_signal_handlers();
    let conv_pipeline = ConversionPipeline::new(config.conv.clone());
    let excl = config.conv.contains(&dd_rs::conv::ConvOp::Excl);
    let nocreat = config.conv.contains(&dd_rs::conv::ConvOp::Nocreat);
    let notrunc = config.conv.contains(&dd_rs::conv::ConvOp::Notrunc);

    let input = flags::open_input(&flags::InputOptions {
        path: config.input_path.clone(), flags: config.iflags.clone(), must_be_directory: false,
    })?;

    let output = flags::open_output(&flags::OutputOptions {
        path: config.output_path.clone(), flags: config.oflags.clone(), excl, nocreat, notrunc, must_be_directory: false,
    })?;

    let engine_config = EngineConfig {
        input, output,
        ibs: config.ibs as usize, obs: config.obs as usize, cbs: config.cbs as usize,
        count: config.count, skip: config.skip, seek: config.seek,
        count_bytes: config.count_bytes, skip_bytes: config.skip_bytes, seek_bytes: config.seek_bytes,
        conv: conv_pipeline, iflags: config.iflags, oflags: config.oflags, status_level: config.status_level,
    };

    let report = io_engine::run_transfer(engine_config)?;
    if report.read_errors > 0 {
        Ok(2)
    } else {
        Ok(0)
    }
}

/// Run legacy and handle exit codes for subcommand dispatch.
fn run_legacy_or_exit(processed_args: &[String]) {
    match run_legacy(processed_args) {
        Ok(exit_code) => {
            if exit_code != 0 {
                process::exit(exit_code);
            }
        }
        Err(e) => {
            eprintln!("dd-rs: {}", e);
            match &e {
                dd_rs::Error::ReadError{..} | dd_rs::Error::WriteError{..} => process::exit(2),
                dd_rs::Error::Conversion(_) => process::exit(3),
                _ => process::exit(1),
            }
        }
    }
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let raw_args: Vec<String> = env::args().collect();
    let sub = detect_subcommand(&raw_args);

    match sub {
        Subcommand::Copy => cmd_copy(&raw_args),
        Subcommand::Zero => cmd_zero(&raw_args),
        Subcommand::Random => cmd_random(&raw_args),
        Subcommand::Info => cmd_info(&raw_args),
        Subcommand::Explain => cmd_explain(&raw_args),
        Subcommand::Wipe => cmd_wipe(&raw_args),
        Subcommand::Legacy => run_legacy_or_exit(&preprocess_dd_args(&raw_args)),
    }
}
