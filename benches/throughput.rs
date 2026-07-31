/// Throughput benchmarks for dd-rs.
///
/// Run: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::io::{Read, Write};
use tempfile::NamedTempFile;

use dd_rs::conv::{case, swab};
use dd_rs::io_engine::{EngineConfig, run_transfer};
use dd_rs::conv::ConversionPipeline;
use dd_rs::status::StatusLevel;

// ═══════════════════════════════════════════════════════════════
// Micro-benchmarks: individual conversions
// ═══════════════════════════════════════════════════════════════

fn bench_lcase(c: &mut Criterion) {
    let mut group = c.benchmark_group("conversions");
    for size in [1024, 65536, 1_048_576usize] {
        let data = vec![b'A'; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("lcase {}B", size), |b| {
            b.iter(|| {
                let mut buf = data.clone();
                case::lcase(black_box(&mut buf));
                black_box(&buf);
            })
        });
    }
    group.finish();
}

fn bench_ucase(c: &mut Criterion) {
    let mut group = c.benchmark_group("conversions");
    for size in [1024, 65536, 1_048_576usize] {
        let data = vec![b'a'; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("ucase {}B", size), |b| {
            b.iter(|| {
                let mut buf = data.clone();
                case::ucase(black_box(&mut buf));
                black_box(&buf);
            })
        });
    }
    group.finish();
}

fn bench_swab(c: &mut Criterion) {
    let mut group = c.benchmark_group("conversions");
    for size in [1024, 65536, 1_048_576usize] {
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("swab {}B", size), |b| {
            b.iter(|| {
                let mut buf = data.clone();
                swab::swab_bytes(black_box(&mut buf));
                black_box(&buf);
            })
        });
    }
    group.finish();
}

// ═══════════════════════════════════════════════════════════════
// Macro-benchmarks: full I/O engine throughput
// ═══════════════════════════════════════════════════════════════

fn bench_e2e_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e");

    for bs in [512, 4096, 65536, 1_048_576usize] {
        let data = vec![0u8; bs * 256]; // 256 blocks of `bs` bytes
        group.throughput(Throughput::Bytes(data.len() as u64));

        group.bench_function(format!("raw_copy bs={}", bs), |b| {
            b.iter(|| {
                let mut infile = NamedTempFile::new().unwrap();
                infile.write_all(&data).unwrap();
                infile.flush().unwrap();
                let outfile = NamedTempFile::new().unwrap();

                let config = EngineConfig {
                    input: std::fs::File::open(infile.path()).unwrap(),
                    output: std::fs::File::create(outfile.path()).unwrap(),
                    ibs: bs, obs: bs, cbs: 0,
                    count: None, skip: 0, seek: 0,
                    count_bytes: false, skip_bytes: false, seek_bytes: false,
                    conv: ConversionPipeline::new(vec![]),
                    iflags: vec![], oflags: vec![],
                    status_level: StatusLevel::None,
                };
                black_box(run_transfer(config).unwrap());
            })
        });

        group.bench_function(format!("lcase_copy bs={}", bs), |b| {
            let upper: Vec<u8> = (0..bs * 256).map(|i| if i % 2 == 0 { b'A' } else { b'Z' }).collect();
            b.iter(|| {
                let mut infile = NamedTempFile::new().unwrap();
                infile.write_all(&upper).unwrap();
                infile.flush().unwrap();
                let outfile = NamedTempFile::new().unwrap();

                let config = EngineConfig {
                    input: std::fs::File::open(infile.path()).unwrap(),
                    output: std::fs::File::create(outfile.path()).unwrap(),
                    ibs: bs, obs: bs, cbs: 0,
                    count: None, skip: 0, seek: 0,
                    count_bytes: false, skip_bytes: false, seek_bytes: false,
                    conv: ConversionPipeline::new(vec![dd_rs::conv::ConvOp::Lcase]),
                    iflags: vec![], oflags: vec![],
                    status_level: StatusLevel::None,
                };
                black_box(run_transfer(config).unwrap());
            })
        });
    }
    group.finish();
}

criterion_group!(conversions, bench_lcase, bench_ucase, bench_swab);
criterion_group!(throughput, bench_e2e_throughput);
criterion_main!(conversions, throughput);
