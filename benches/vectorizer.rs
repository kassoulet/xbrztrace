//! Criterion benchmarks for vector tracing, SVG export and the full pipeline.
//!
//! Run with: `cargo bench --bench vectorizer`

use xbrztrace::optimizer::optimize;
use xbrztrace::svg_exporter::export;
use xbrztrace::vectorizer::vectorize;
use xbrztrace::xbrz_engine::{scale_image, ScalerConfig};
use criterion::{criterion_group, criterion_main, Criterion};

#[path = "support.rs"]
mod support;

fn vectorizer_bench(c: &mut Criterion) {
    // 64x64 scene upscaled 4x -> 256x256 grid, the kind of output the
    // vectorizer sees in real use.
    let scaled = scale_image(&support::scene(64, 64), 4, &ScalerConfig::default());
    assert_eq!((scaled.width, scaled.height), (256, 256));

    c.bench_function("vectorize/256x256/merged", |b| {
        b.iter(|| vectorize(&scaled, true))
    });
    c.bench_function("vectorize/256x256/unmerged", |b| {
        b.iter(|| vectorize(&scaled, false))
    });

    let regions = vectorize(&scaled, true);
    c.bench_function("export/256x256", |b| {
        b.iter(|| export(&regions, scaled.width, scaled.height))
    });
    c.bench_function("optimize/256x256", |b| {
        b.iter(|| {
            let mut r = regions.clone();
            optimize(&mut r)
        })
    });

    // End-to-end: xBRZ upscale + trace + optimize + serialize.
    let img = support::scene(64, 64);
    c.bench_function("pipeline/64x64@4x", |b| {
        b.iter(|| {
            let s = scale_image(&img, 4, &ScalerConfig::default());
            let mut r = vectorize(&s, true);
            optimize(&mut r);
            export(&r, s.width, s.height)
        })
    });
}

criterion_group!(benches, vectorizer_bench);
criterion_main!(benches);
