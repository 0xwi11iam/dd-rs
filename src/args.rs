/// Command-line argument parsing.
///
/// Uses `clap` derive to define all dd-compatible flags and operands.
/// dd-rs supports both the traditional dd syntax (if=/dev/zero of=out ...)
/// and a more modern flag-based syntax (--input /dev/zero --output out).

use clap::Parser;

use crate::conv::ConvOp;
use crate::flags::{self, IoFlag};
use crate::io_engine;
use crate::safety::SafetyLevel;
use crate::sizes;
use crate::status::StatusLevel;

/// dd-rs — a safe, modern Rust+C alternative to dd.
///
/// Data duplication, conversion, and transformation tool.
/// Copies input to output with optional conversions, block size control,
/// and progress reporting.
#[derive(Parser, Debug)]
#[command(
    name = "dd-rs",
    version,
    about = "Safe, modern alternative to dd — copy and convert data",
    long_about = "dd-rs copies data from an input file to an output file, \
                  applying optional conversions. It supports all standard dd \
                  operands plus additional safety and usability features.",
    after_help = "SIZE SUFFIXES:\n  \
                  c=1, w=2, b=512, K=1024, M=1024^2, G=1024^3, ...\n  \
                  kB=1000, MB=1000^2, GB=1000^3, ...\n  \
                  KiB=1024, MiB=1024^2, GiB=1024^3, ...\n  \
                  xM = times M (e.g., 4xM = 4*1048576)\n\n\
                  EXAMPLES:\n  \
                  dd-rs if=/dev/zero of=out bs=1M count=100\n  \
                  dd-rs if=input.dat of=output.dat conv=swab,noerror\n  \
                  dd-rs if=/dev/urandom of=key.bin bs=32 count=1 status=none\n  \
                  dd-rs --input data.txt --output /dev/null status=progress"
)]
pub struct CliArgs {
    // =========================================================================
    // Core operands
    // =========================================================================

    /// Read from FILE instead of stdin
    #[arg(short = 'i', long = "if", value_name = "FILE", help = "Read from FILE instead of stdin")]
    pub ifile: Option<String>,

    /// Write to FILE instead of stdout
    #[arg(short = 'o', long = "of", value_name = "FILE", help = "Write to FILE instead of stdout")]
    pub ofile: Option<String>,

    /// Input block size in bytes (default: 512)
    #[arg(long = "ibs", value_name = "BYTES", default_value = "512", help = "Input block size")]
    pub ibs: String,

    /// Output block size in bytes (default: 512)
    #[arg(long = "obs", value_name = "BYTES", default_value = "512", help = "Output block size")]
    pub obs: String,

    /// Set both input and output block size to BYTES
    #[arg(long = "bs", value_name = "BYTES", help = "Set both input and output block size")]
    pub bs: Option<String>,

    /// Conversion buffer size (used by block/unblock/ascii/ebcdic conversions)
    #[arg(long = "cbs", value_name = "BYTES", default_value = "0", help = "Conversion buffer size")]
    pub cbs: String,

    /// Copy only N input blocks (not bytes; use NB for byte count)
    #[arg(long = "count", value_name = "N", help = "Copy only N input blocks")]
    pub count: Option<String>,

    /// Skip N input blocks before copying
    #[arg(long = "skip", value_name = "N", default_value = "0", help = "Skip N input blocks")]
    pub skip: String,

    /// Skip N output blocks before writing (alias for seek)
    #[arg(long = "iseek", value_name = "N", help = "Alias for skip")]
    pub iseek: Option<String>,

    /// Skip N output blocks before writing
    #[arg(long = "seek", value_name = "N", default_value = "0", help = "Skip N output blocks")]
    pub seek: String,

    /// Alias for seek
    #[arg(long = "oseek", value_name = "N", help = "Alias for seek")]
    pub oseek: Option<String>,

    /// Control output verbosity: none, noxfer, progress, json
    #[arg(long = "status", value_name = "LEVEL", default_value = "progress", help = "Status verbosity level")]
    pub status: String,

    // =========================================================================
    // Conversion options
    // =========================================================================

    /// Comma-separated list of conversions
    #[arg(long = "conv", value_name = "CONVS", help = "Conversion options")]
    pub conv: Option<String>,

    // =========================================================================
    // I/O flags
    // =========================================================================

    /// Comma-separated list of input flags
    #[arg(long = "iflag", value_name = "FLAGS", help = "Input file flags")]
    pub iflag: Option<String>,

    /// Comma-separated list of output flags
    #[arg(long = "oflag", value_name = "FLAGS", help = "Output file flags")]
    pub oflag: Option<String>,

    // =========================================================================
    // dd-rs extras (beyond GNU dd)
    // =========================================================================

    /// Show progress bar (requires status=progress)
    #[arg(long = "progress-bar", help = "Show a progress bar")]
    pub progress_bar: bool,

    /// Dry run: parse arguments and check file existence, but don't transfer
    #[arg(long = "dry-run", help = "Validate arguments without transferring data")]
    pub dry_run: bool,

    /// Skip interactive confirmation prompts (warnings are still shown)
    #[arg(long = "yes", short = 'y', help = "Skip confirmation prompts")]
    pub yes: bool,

    /// Skip ALL safety checks — DANGEROUS, equivalent to GNU dd behaviour
    #[arg(long = "force", help = "Skip all safety checks (DANGEROUS)")]
    pub force: bool,

    /// Auto-tune block sizes for optimal performance on this system
    #[arg(long = "auto-tune", help = "Automatically pick optimal block sizes")]
    pub auto_tune: bool,

    /// Explain what this command will do, with risk assessment (does not execute)
    #[arg(short = 'E', long = "explain", help = "Explain command and assess risk without executing")]
    pub explain: bool,
}

// =============================================================================
// Resolved configuration
// =============================================================================

/// Fully-parsed, validated configuration ready for the I/O engine.
#[derive(Debug)]
pub struct ResolvedConfig {
    pub input_path: Option<String>,
    pub output_path: Option<String>,
    pub ibs: u64,
    pub obs: u64,
    pub cbs: u64,
    pub count: Option<u64>,
    pub skip: u64,
    pub seek: u64,
    pub count_bytes: bool,
    pub skip_bytes: bool,
    pub seek_bytes: bool,
    pub conv: Vec<ConvOp>,
    pub iflags: Vec<IoFlag>,
    pub oflags: Vec<IoFlag>,
    pub status_level: StatusLevel,
    pub safety_level: SafetyLevel,
    pub progress_bar: bool,
    pub dry_run: bool,
    pub auto_tune: bool,
    pub explain: bool,
}

/// Parse and validate all CLI arguments into a ResolvedConfig.
pub fn resolve_config(args: CliArgs) -> crate::error::Result<ResolvedConfig> {
    // --- Block sizes ---
    let ibs_size = sizes::parse_positive_size(&args.ibs)?;
    let obs_size = sizes::parse_positive_size(&args.obs)?;
    let cbs_size = sizes::parse_size(&args.cbs)?;

    // bs= overrides both ibs and obs
    let (ibs_bytes, obs_bytes) = if let Some(ref bs) = args.bs {
        let bs_size = sizes::parse_positive_size(bs)?;
        (bs_size.bytes, bs_size.bytes)
    } else {
        (ibs_size.bytes, obs_size.bytes)
    };

    // --- Count ---
    let (count, count_bytes) = if let Some(ref c) = args.count {
        let sz = sizes::parse_size(c)?;
        (Some(sz.bytes), sz.explicit_bytes)
    } else {
        (None, false)
    };

    // --- Skip ---
    let (skip, skip_bytes) = {
        let skip_str = args.iseek.as_deref().unwrap_or(&args.skip);
        let sz = sizes::parse_size(skip_str)?;
        (sz.bytes, sz.explicit_bytes)
    };

    // --- Seek ---
    let (seek, seek_bytes) = {
        let seek_str = args.oseek.as_deref().unwrap_or(&args.seek);
        let sz = sizes::parse_size(seek_str)?;
        (sz.bytes, sz.explicit_bytes)
    };

    // --- Conversions ---
    let conv_ops = if let Some(ref conv_str) = args.conv {
        ConvOp::parse_list(conv_str)?
    } else {
        vec![]
    };

    // --- I/O flags ---
    let iflags = if let Some(ref f) = args.iflag {
        flags::parse_flags(f)?
    } else {
        vec![]
    };
    let oflags = if let Some(ref f) = args.oflag {
        flags::parse_flags(f)?
    } else {
        vec![]
    };

    // --- Status level ---
    let status_level = StatusLevel::parse(&args.status).unwrap_or_else(|| {
        eprintln!(
            "dd-rs: warning: unknown status level '{}', using 'progress'",
            args.status
        );
        StatusLevel::Progress
    });

    // --- Safety level ---
    let safety_level = SafetyLevel::from_args(args.yes, args.force);

    // --- Auto-tune block sizes ---
    let (ibs_bytes, obs_bytes) = if args.auto_tune {
        // Auto-tune overrides explicit block size settings
        let tuned = io_engine::auto_tune_block_size(true) as u64;
        if args.bs.is_some() {
            // bs= was set explicitly — still use it but warn
            eprintln!("dd-rs: note: --auto-tune overrides bs=; using {} bytes", tuned);
            (tuned, tuned)
        } else {
            (tuned, tuned)
        }
    } else {
        (ibs_bytes, obs_bytes)
    };

    // Warn about small block sizes (the #1 performance killer)
    if ibs_bytes < 4096 && !args.auto_tune {
        eprintln!(
            "dd-rs: note: block size is {} bytes. For better performance, try --auto-tune\n\
             or set bs=128K or larger. Small block sizes cause millions of syscalls/second.",
            ibs_bytes
        );
    }

    Ok(ResolvedConfig {
        input_path: args.ifile,
        output_path: args.ofile,
        ibs: ibs_bytes,
        obs: obs_bytes,
        cbs: cbs_size.bytes,
        count,
        skip,
        seek,
        count_bytes,
        skip_bytes,
        seek_bytes,
        conv: conv_ops,
        iflags,
        oflags,
        status_level,
        safety_level,
        progress_bar: args.progress_bar,
        dry_run: args.dry_run,
        auto_tune: args.auto_tune,
        explain: args.explain,
    })
}
