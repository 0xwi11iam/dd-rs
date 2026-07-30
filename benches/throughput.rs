/// Throughput benchmarks for dd-rs.
///
/// Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use dd_rs::conv::{case, swab};

fn bench_lcase(c: &mut Criterion) {
    let data = vec![b'A'; 1024 * 1024]; // 1 MiB
    c.bench_function("lcase 1MiB", |b| {
        b.iter(|| {
            let mut buf = data.clone();
            case::lcase(black_box(&mut buf));
            black_box(&buf);
        })
    });
}

fn bench_swab(c: &mut Criterion) {
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
    c.bench_function("swab 1MiB", |b| {
        b.iter(|| {
            let mut buf = data.clone();
            swab::swab_bytes(black_box(&mut buf));
            black_box(&buf);
        })
    });
}

criterion_group!(benches, bench_lcase, bench_swab);
criterion_main!(benches);
