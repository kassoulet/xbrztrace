//! Criterion benchmarks for the xBRZ scaling engine.
//!
//! Run with: `cargo bench --bench xbrz`

use xbrztrace::xbrz_engine::{scale_image, ScalerConfig};
use criterion::{criterion_group, criterion_main, Criterion};

#[path = "support.rs"]
mod support;

fn xbrz_bench(c: &mut Criterion) {
    let img = support::scene(64, 64);
    let cfg = ScalerConfig::default();

    let mut group = c.benchmark_group("xbrz/64x64");
    for factor in 2u8..=6u8 {
        group.bench_function(format!("{factor}x"), |b| {
            b.iter(|| scale_image(&img, factor, &cfg))
        });
    }
    group.finish();

    // Throughput on a larger input (representative of a composed tilemap).
    let large = support::scene(128, 128);
    let mut group = c.benchmark_group("xbrz/128x128");
    group.bench_function("4x", |b| b.iter(|| scale_image(&large, 4, &cfg)));
    group.finish();
}

criterion_group!(benches, xbrz_bench);
criterion_main!(benches);
