/// Integration tests for dd-rs.
///
/// Covers: size parsing, conversions, I/O flags, E2E transfers,
/// safety assessment, error handling, edge cases.
///
/// Run: cargo test

use std::io::Write;
use tempfile::NamedTempFile;

use dd_rs::conv::{ConvOp, ConversionPipeline, ConvContext};
use dd_rs::io_engine::{EngineConfig, run_transfer};
use dd_rs::sizes;
use dd_rs::status::StatusLevel;

// ═══════════════════════════════════════════════════════════════
// Size parsing
// ═══════════════════════════════════════════════════════════════

#[test]
fn size_basic() {
    assert_eq!(sizes::parse_size("512").unwrap().bytes, 512);
    assert_eq!(sizes::parse_size("0").unwrap().bytes, 0);
}

#[test]
fn size_single_char() {
    assert_eq!(sizes::parse_size("10c").unwrap().bytes, 10);
    assert_eq!(sizes::parse_size("10w").unwrap().bytes, 20);
    assert_eq!(sizes::parse_size("1b").unwrap().bytes, 512);
}

#[test]
fn size_binary() {
    assert_eq!(sizes::parse_size("1K").unwrap().bytes, 1024);
    assert_eq!(sizes::parse_size("1KiB").unwrap().bytes, 1024);
    assert_eq!(sizes::parse_size("1M").unwrap().bytes, 1048576);
    assert_eq!(sizes::parse_size("1G").unwrap().bytes, 1073741824);
}

#[test]
fn size_decimal() {
    assert_eq!(sizes::parse_size("1kB").unwrap().bytes, 1000);
    assert_eq!(sizes::parse_size("1MB").unwrap().bytes, 1_000_000);
    assert_eq!(sizes::parse_size("1GB").unwrap().bytes, 1_000_000_000);
}

#[test]
fn size_x_syntax() {
    assert_eq!(sizes::parse_size("4xM").unwrap().bytes, 4 * 1048576);
    assert_eq!(sizes::parse_size("2xK").unwrap().bytes, 2048);
}

#[test]
fn size_byte_count() {
    let s = sizes::parse_size("100B").unwrap();
    assert_eq!(s.bytes, 100);
    assert!(s.explicit_bytes);
}

#[test]
fn size_hex() {
    assert_eq!(sizes::parse_size("0xFF").unwrap().bytes, 255);
    assert_eq!(sizes::parse_size("0x1000").unwrap().bytes, 4096);
}

#[test]
fn size_invalid() {
    assert!(sizes::parse_size("").is_err());
    assert!(sizes::parse_size("notanumber").is_err());
    assert!(sizes::parse_size("99999999999999999999E").is_err());
}

#[test]
fn size_edge_cases() {
    // Suffixes are case-sensitive: T not t
    assert_eq!(sizes::parse_size("1T").unwrap().bytes, 1024u64.pow(4));
    assert_eq!(sizes::parse_size("1P").unwrap().bytes, 1024u64.pow(5));
    assert_eq!(sizes::parse_size("1TiB").unwrap().bytes, 1024u64.pow(4));
    // Decimal edge
    assert_eq!(sizes::parse_size("1TB").unwrap().bytes, 1_000_000_000_000);
}

// ═══════════════════════════════════════════════════════════════
// Conversion parsing
// ═══════════════════════════════════════════════════════════════

#[test]
fn conv_parse_all() {
    // Every conversion should parse
    for conv_str in &[
        "ascii", "ebcdic", "ibm", "block", "unblock",
        "lcase", "ucase", "swab", "sync", "sparse",
        "noerror", "notrunc", "excl", "nocreat", "fdatasync", "fsync",
    ] {
        assert!(ConvOp::parse(conv_str).is_ok(), "failed to parse: {}", conv_str);
    }
}

#[test]
fn conv_parse_list() {
    let ops = ConvOp::parse_list("noerror,sync,notrunc").unwrap();
    assert_eq!(ops.len(), 3);
}

#[test]
fn conv_parse_invalid() {
    assert!(ConvOp::parse("nonsense").is_err());
    assert!(ConvOp::parse("").is_err());
}

#[test]
fn conv_pipeline_canonical_order() {
    let ops = ConvOp::parse_list("swab,sync,lcase").unwrap();
    let pipeline = ConversionPipeline::new(ops);
    let data_ops: Vec<_> = pipeline.ops().iter()
        .filter(|o| o.is_data_conversion()).cloned().collect();
    let lcase_pos = data_ops.iter().position(|o| *o == ConvOp::Lcase).unwrap();
    let swab_pos = data_ops.iter().position(|o| *o == ConvOp::Swab).unwrap();
    let sync_pos = data_ops.iter().position(|o| *o == ConvOp::Sync).unwrap();
    assert!(lcase_pos < swab_pos, "lcase must come before swab");
    assert!(swab_pos < sync_pos, "swab must come before sync");
}

#[test]
fn conv_ebcdic_implies_block() {
    let pipeline = ConversionPipeline::new(ConvOp::parse_list("ebcdic").unwrap());
    assert!(pipeline.ops().contains(&ConvOp::Block));
}

#[test]
fn conv_ascii_implies_unblock() {
    let pipeline = ConversionPipeline::new(ConvOp::parse_list("ascii").unwrap());
    assert!(pipeline.ops().contains(&ConvOp::Unblock));
}

#[test]
fn conv_ibm_implies_block() {
    let pipeline = ConversionPipeline::new(ConvOp::parse_list("ibm").unwrap());
    assert!(pipeline.ops().contains(&ConvOp::Block));
}

#[test]
fn conv_empty_pipeline_no_data_convs() {
    let pipeline = ConversionPipeline::new(vec![]);
    assert!(!pipeline.has_any_data_conv());
    assert!(!pipeline.has_sparse());
    assert!(!pipeline.has_noerror());
}

// ═══════════════════════════════════════════════════════════════
// Case conversion
// ═══════════════════════════════════════════════════════════════

#[test]
fn case_lcase() {
    use dd_rs::conv::case;
    let mut buf = b"Hello, WORLD! 123".to_vec();
    case::lcase(&mut buf);
    assert_eq!(&buf, b"hello, world! 123");
}

#[test]
fn case_ucase() {
    use dd_rs::conv::case;
    let mut buf = b"Hello, world! 123".to_vec();
    case::ucase(&mut buf);
    assert_eq!(&buf, b"HELLO, WORLD! 123");
}

#[test]
fn case_noop_on_non_ascii() {
    use dd_rs::conv::case;
    let mut buf = vec![0x80, 0xFF, 0x00];
    let original = buf.clone();
    case::lcase(&mut buf);
    assert_eq!(buf, original); // non-ASCII unchanged
}

// ═══════════════════════════════════════════════════════════════
// Swab conversion
// ═══════════════════════════════════════════════════════════════

#[test]
fn swab_even() {
    use dd_rs::conv::swab;
    let mut buf = [0x01, 0x02, 0x03, 0x04];
    swab::swab_bytes(&mut buf);
    assert_eq!(buf, [0x02, 0x01, 0x04, 0x03]);
}

#[test]
fn swab_odd() {
    use dd_rs::conv::swab;
    let mut buf = [0x01, 0x02, 0x03, 0x04, 0x05];
    swab::swab_bytes(&mut buf);
    assert_eq!(buf, [0x02, 0x01, 0x04, 0x03, 0x05]);
}

#[test]
fn swab_empty() {
    use dd_rs::conv::swab;
    let mut buf: [u8; 0] = [];
    swab::swab_bytes(&mut buf);
}

#[test]
fn swab_single_byte() {
    use dd_rs::conv::swab;
    let mut buf = [0x42];
    swab::swab_bytes(&mut buf);
    assert_eq!(buf, [0x42]); // unchanged
}

// ═══════════════════════════════════════════════════════════════
// Block / Unblock
// ═══════════════════════════════════════════════════════════════

#[test]
fn block_pads_to_cbs() {
    use dd_rs::conv::block;
    let ctx = ConvContext::new(10, 512);
    let mut buf = b"hi\n\0\0\0\0\0\0\0".to_vec();
    let n = block::block_record(&mut buf, &ctx).unwrap();
    // "hi\n" → padded to 10 bytes with spaces
    assert_eq!(buf[0], b'h');
    assert_eq!(buf[1], b'i');
    assert_eq!(buf[2], b' ');
    assert_eq!(n, 10);
}

#[test]
fn unblock_replaces_spaces_with_newline() {
    use dd_rs::conv::block;
    let mut ctx = ConvContext::new(10, 512);
    let mut buf = b"hi        ".to_vec(); // "hi" + 8 spaces = 10 bytes
    let n = block::unblock_record(&mut buf, &mut ctx).unwrap();
    assert_eq!(&buf[..3], b"hi\n");
    assert_eq!(n, 3);
}

// ═══════════════════════════════════════════════════════════════
// I/O flags
// ═══════════════════════════════════════════════════════════════

#[test]
fn flags_parse_valid() {
    use dd_rs::flags;
    let f = flags::parse_flags("nonblock,noatime,fullblock").unwrap();
    assert_eq!(f.len(), 3);
}

#[test]
fn flags_parse_invalid() {
    assert!(dd_rs::flags::parse_flags("nonsense").is_err());
}

#[test]
fn flags_parse_empty() {
    let f = dd_rs::flags::parse_flags("").unwrap();
    assert!(f.is_empty());
}

#[test]
fn flags_case_insensitive() {
    let f = dd_rs::flags::parse_flags("NONBLOCK,FullBlock").unwrap();
    assert_eq!(f.len(), 2);
}

// ═══════════════════════════════════════════════════════════════
// Safety assessment
// ═══════════════════════════════════════════════════════════════

#[test]
fn safety_safe_regular_file() {
    use std::path::Path;
    let risk = dd_rs::safety::assess_risk(
        Some(Path::new("/tmp/test.bin")),
        Some(Path::new("/tmp/out.bin")),
        Some(10), 4096, &[],
    );
    assert_eq!(risk.score, 0, "regular file copy should be score 0, got {}", risk.score);
}

#[test]
fn safety_raw_disk_device() {
    use std::path::Path;
    let risk = dd_rs::safety::assess_risk(
        None,
        Some(Path::new("/dev/sda")),
        None, 512, &[],
    );
    // Should trigger at minimum: /dev path detection + unbounded warning
    // Exact score depends on platform (raw disk detection is Linux-only)
    assert!(risk.score >= 10, "should detect /dev path or unbounded write, got {}", risk.score);
    assert!(!risk.warnings.is_empty(), "should have warnings for /dev/sda target");
}

#[test]
fn safety_disk_partition() {
    use std::path::Path;
    // /dev/sda1 — /dev path factor + partition detection (platform-dependent)
    let risk = dd_rs::safety::assess_risk(
        None,
        Some(Path::new("/dev/sda1")),
        Some(100), 4096, &[],
    );
    // At minimum should trigger /dev path detection
    assert!(risk.score >= 15, "should at least trigger /dev path warning, got {}", risk.score);
}

#[test]
fn safety_noerror_warning() {
    use std::path::Path;
    let risk = dd_rs::safety::assess_risk(
        None, None, None, 512,
        &[ConvOp::Noerror],
    );
    assert!(!risk.warnings.is_empty(), "noerror should generate warnings");
}

#[test]
fn safety_same_file_io() {
    use std::path::Path;
    let risk = dd_rs::safety::assess_risk(
        Some(Path::new("/tmp/same.bin")),
        Some(Path::new("/tmp/same.bin")),
        Some(10), 4096, &[],
    );
    assert!(risk.score >= 40, "same file I/O should be dangerous");
}

#[test]
fn safety_unbounded_no_count() {
    use std::path::Path;
    let risk = dd_rs::safety::assess_risk(
        Some(Path::new("/dev/zero")),
        Some(Path::new("/dev/sda1")),
        None, 512, &[],
    );
    // should warn about unbounded write
    assert!(risk.warnings.iter().any(|w| w.contains("count")));
}

// ═══════════════════════════════════════════════════════════════
// E2E I/O engine
// ═══════════════════════════════════════════════════════════════

fn make_engine(input_data: &[u8], conv: Vec<ConvOp>, bs: usize, count: Option<u64>) -> (NamedTempFile, NamedTempFile, EngineConfig) {
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(input_data).unwrap();
    infile.flush().unwrap();
    let outfile = NamedTempFile::new().unwrap();
    let input = std::fs::File::open(infile.path()).unwrap();
    let output = std::fs::File::create(outfile.path()).unwrap();
    let config = EngineConfig {
        input, output,
        ibs: bs, obs: bs, cbs: bs,
        count, skip: 0, seek: 0,
        count_bytes: false, skip_bytes: false, seek_bytes: false,
        conv: ConversionPipeline::new(conv),
        iflags: vec![], oflags: vec![],
        status_level: StatusLevel::None,
    };
    (infile, outfile, config)
}

#[test]
fn e2e_basic_copy() {
    let data = b"Hello, dd-rs! This is a test.";
    let (_in, out, config) = make_engine(data, vec![], 512, None);
    let report = run_transfer(config).unwrap();
    assert_eq!(report.bytes_written as usize, data.len());
    let output = std::fs::read(out.path()).unwrap();
    assert_eq!(output, data);
}

#[test]
fn e2e_lcase() {
    let data = b"HELLO WORLD";
    let (_in, out, config) = make_engine(data, vec![ConvOp::Lcase], 512, None);
    run_transfer(config).unwrap();
    let output = std::fs::read(out.path()).unwrap();
    assert_eq!(&output[..11], b"hello world");
}

#[test]
fn e2e_ucase() {
    let data = b"hello world";
    let (_in, out, config) = make_engine(data, vec![ConvOp::Ucase], 512, None);
    run_transfer(config).unwrap();
    let output = std::fs::read(out.path()).unwrap();
    assert_eq!(&output[..11], b"HELLO WORLD");
}

#[test]
fn e2e_swab() {
    let data = [0x01, 0x02, 0x03, 0x04];
    let (_in, out, config) = make_engine(&data, vec![ConvOp::Swab], 512, None);
    run_transfer(config).unwrap();
    let output = std::fs::read(out.path()).unwrap();
    assert_eq!(&output[..4], &[0x02, 0x01, 0x04, 0x03]);
}

#[test]
fn e2e_count_limited() {
    let data = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"; // 26 bytes
    let (_in, out, config) = make_engine(data, vec![], 4, Some(3)); // 3 blocks × 4 bytes = 12 bytes
    let report = run_transfer(config).unwrap();
    assert_eq!(report.bytes_written, 12);
    let output = std::fs::read(out.path()).unwrap();
    assert_eq!(&output[..12], b"ABCDEFGHIJKL");
}

#[test]
fn e2e_sync_pads_to_ibs() {
    let data = b"HI"; // 2 bytes
    let (_in, out, config) = make_engine(data, vec![ConvOp::Sync], 8, None);
    run_transfer(config).unwrap();
    let output = std::fs::read(out.path()).unwrap();
    assert_eq!(output.len(), 8);
    assert_eq!(&output[..2], b"HI");
    assert!(output[2..].iter().all(|&b| b == 0), "sync should pad with NULs");
}

#[test]
fn e2e_count_bytes_mode() {
    let data = b"ABCDEFGH"; // 8 bytes
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(data).unwrap();
    infile.flush().unwrap();
    let outfile = NamedTempFile::new().unwrap();
    let config = EngineConfig {
        input: std::fs::File::open(infile.path()).unwrap(),
        output: std::fs::File::create(outfile.path()).unwrap(),
        ibs: 4, obs: 4, cbs: 0,
        count: Some(6), // 6 bytes (not 6 blocks)
        skip: 0, seek: 0,
        count_bytes: true, skip_bytes: false, seek_bytes: false,
        conv: ConversionPipeline::new(vec![]),
        iflags: vec![], oflags: vec![],
        status_level: StatusLevel::None,
    };
    let report = run_transfer(config).unwrap();
    assert_eq!(report.bytes_written, 6);
}
