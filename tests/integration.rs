/// Integration tests for dd-rs.
///
/// These tests verify the full pipeline: argument parsing → file I/O →
/// conversion → output validation. They use tempfile for isolated test
/// environments.

use std::io::Write;
use tempfile::NamedTempFile;

use dd_rs::conv::{ConvOp, ConversionPipeline};
use dd_rs::sizes;

// =============================================================================
// Size parsing tests
// =============================================================================

#[test]
fn test_size_parsing_all_suffixes() {
    // Basic
    assert_eq!(sizes::parse_size("512").unwrap().bytes, 512);
    assert_eq!(sizes::parse_size("0").unwrap().bytes, 0);
    assert_eq!(sizes::parse_size("1").unwrap().bytes, 1);

    // Single-char
    assert_eq!(sizes::parse_size("10c").unwrap().bytes, 10);
    assert_eq!(sizes::parse_size("10w").unwrap().bytes, 20);
    assert_eq!(sizes::parse_size("1b").unwrap().bytes, 512);

    // Binary (1024)
    assert_eq!(sizes::parse_size("1K").unwrap().bytes, 1024);
    assert_eq!(sizes::parse_size("1KiB").unwrap().bytes, 1024);
    assert_eq!(sizes::parse_size("1M").unwrap().bytes, 1048576);
    assert_eq!(sizes::parse_size("1MiB").unwrap().bytes, 1048576);
    assert_eq!(sizes::parse_size("1G").unwrap().bytes, 1073741824);
    assert_eq!(sizes::parse_size("1GiB").unwrap().bytes, 1073741824);

    // Decimal (1000)
    assert_eq!(sizes::parse_size("1kB").unwrap().bytes, 1000);
    assert_eq!(sizes::parse_size("1KB").unwrap().bytes, 1000);
    assert_eq!(sizes::parse_size("1MB").unwrap().bytes, 1_000_000);

    // x-syntax
    assert_eq!(sizes::parse_size("4xM").unwrap().bytes, 4 * 1048576);
    assert_eq!(sizes::parse_size("1xK").unwrap().bytes, 1024);

    // GNU byte count
    let s = sizes::parse_size("100B").unwrap();
    assert_eq!(s.bytes, 100);
    assert!(s.explicit_bytes);

    // Hex
    assert_eq!(sizes::parse_size("0xFF").unwrap().bytes, 255);
    assert_eq!(sizes::parse_size("0x1000").unwrap().bytes, 4096);
}

// =============================================================================
// Conversion tests
// =============================================================================

#[test]
fn test_conv_parse_list() {
    let ops = ConvOp::parse_list("noerror,sync,notrunc").unwrap();
    assert_eq!(ops.len(), 3);
    assert!(ops.contains(&ConvOp::Noerror));
    assert!(ops.contains(&ConvOp::Sync));
    assert!(ops.contains(&ConvOp::Notrunc));
}

#[test]
fn test_conv_pipeline_order() {
    // The pipeline should enforce canonical ordering regardless of input order
    let ops = ConvOp::parse_list("swab,sync,lcase").unwrap();
    let pipeline = ConversionPipeline::new(ops);
    let data_ops: Vec<_> = pipeline
        .ops()
        .iter()
        .filter(|o| o.is_data_conversion())
        .cloned()
        .collect();
    // lcase before swab before sync
    assert!(data_ops.iter().position(|o| *o == ConvOp::Lcase).unwrap()
        < data_ops.iter().position(|o| *o == ConvOp::Swab).unwrap());
    assert!(data_ops.iter().position(|o| *o == ConvOp::Swab).unwrap()
        < data_ops.iter().position(|o| *o == ConvOp::Sync).unwrap());
}

#[test]
fn test_conv_ebcdic_implies_block() {
    let ops = ConvOp::parse_list("ebcdic").unwrap();
    let pipeline = ConversionPipeline::new(ops);
    assert!(pipeline.ops().contains(&ConvOp::Block));
}

#[test]
fn test_conv_ascii_implies_unblock() {
    let ops = ConvOp::parse_list("ascii").unwrap();
    let pipeline = ConversionPipeline::new(ops);
    assert!(pipeline.ops().contains(&ConvOp::Unblock));
}

// =============================================================================
// Case conversion tests
// =============================================================================

#[test]
fn test_lcase_conversion() {
    use dd_rs::conv::case;
    let mut buf = b"Hello, WORLD! 123".to_vec();
    case::lcase(&mut buf);
    assert_eq!(&buf, b"hello, world! 123");
}

#[test]
fn test_ucase_conversion() {
    use dd_rs::conv::case;
    let mut buf = b"Hello, world! 123".to_vec();
    case::ucase(&mut buf);
    assert_eq!(&buf, b"HELLO, WORLD! 123");
}

// =============================================================================
// Swab tests
// =============================================================================

#[test]
fn test_swab_even_length() {
    use dd_rs::conv::swab;
    let mut buf = [0x01, 0x02, 0x03, 0x04];
    swab::swab_bytes(&mut buf);
    assert_eq!(buf, [0x02, 0x01, 0x04, 0x03]);
}

#[test]
fn test_swab_odd_length() {
    use dd_rs::conv::swab;
    let mut buf = [0x01, 0x02, 0x03, 0x04, 0x05];
    swab::swab_bytes(&mut buf);
    assert_eq!(buf, [0x02, 0x01, 0x04, 0x03, 0x05]); // last byte unchanged
}

// =============================================================================
// I/O flags parsing
// =============================================================================

#[test]
fn test_parse_iflags() {
    let flags = dd_rs::flags::parse_flags("nonblock,noatime").unwrap();
    assert_eq!(flags.len(), 2);
    assert!(flags.contains(&dd_rs::flags::IoFlag::Nonblock));
    assert!(flags.contains(&dd_rs::flags::IoFlag::Noatime));
}

#[test]
fn test_parse_invalid_flag() {
    assert!(dd_rs::flags::parse_flags("nonexistent").is_err());
}

// =============================================================================
// End-to-end I/O engine test
// =============================================================================

#[test]
fn test_basic_copy() {
    let mut input_file = NamedTempFile::new().unwrap();
    input_file.write_all(b"Hello, dd-rs!").unwrap();
    input_file.flush().unwrap();

    let output_file = NamedTempFile::new().unwrap();

    // Re-open for reading
    let input = std::fs::File::open(input_file.path()).unwrap();
    let output = std::fs::File::create(output_file.path()).unwrap();

    let config = EngineConfig {
        input,
        output,
        ibs: 512,
        obs: 512,
        cbs: 0,
        count: None,
        skip: 0,
        seek: 0,
        count_bytes: false,
        skip_bytes: false,
        seek_bytes: false,
        conv: ConversionPipeline::new(vec![]),
        iflags: vec![],
        oflags: vec![],
        status_level: dd_rs::status::StatusLevel::None,
    };

    let report = dd_rs::io_engine::run_transfer(config).unwrap();
    assert_eq!(report.bytes_written, 14);

    // Verify output
    let output_content = std::fs::read(output_file.path()).unwrap();
    assert_eq!(&output_content, b"Hello, dd-rs!");
}

// Need to import EngineConfig
use dd_rs::io_engine::EngineConfig;
