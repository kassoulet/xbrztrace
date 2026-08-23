//! Property-based tests (proptest) for the whole pipeline.
//!
//! The central property, asserted on hundreds of randomly generated grids:
//! **rasterizing the generated SVG back into a pixel grid must reproduce the
//! (scaled) input exactly.** Because the inputs are arbitrary, this exercises
//! region shapes the hand-written tests never thought of — isolated pixels,
//! nested holes, checkerboard noise, thin lines, semi-transparency — and
//! proves the tracer never leaves seams, gaps or overlapping fills.
//!
//! The xBRZ engine is additionally checked for its invariants (output
//! dimensions, determinism, uniform-input stability) on arbitrary random
//! pixels, which doubles as a panic fuzz.

mod common;

use std::collections::HashSet;

use xbrztrace::optimizer::optimize;
use xbrztrace::quantize::quantize;
use xbrztrace::svg_exporter::export;
use xbrztrace::vectorizer::vectorize;
use xbrztrace::xbrz_engine::{color_dist, scale_image, Argb, ArgbImage, ScalerConfig};
use proptest::prelude::*;

/// A fully opaque random color.
fn any_opaque() -> impl Strategy<Value = Argb> {
    (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| Argb::from_rgba(r, g, b, 255))
}

/// A grid whose pixels are drawn from a per-case palette of 2-4 opaque
/// random colors plus transparent. The transparent entry guarantees holes
/// and disconnected same-color regions appear in the inputs.
fn random_grid(w: usize, h: usize) -> impl Strategy<Value = ArgbImage> {
    prop::collection::vec(any_opaque(), 2..=4).prop_flat_map(move |mut palette| {
        palette.push(Argb(0));
        prop::collection::vec(prop::sample::select(palette), w * h)
            .prop_map(move |pixels| ArgbImage::new(w, h, pixels))
    })
}

/// A grid over a fixed palette with a semi-transparent color (alpha 128), so
/// the `fill-opacity` serialization round trip is exercised.
fn translucent_grid(w: usize, h: usize) -> impl Strategy<Value = ArgbImage> {
    let palette = [
        Argb::from_rgba(220, 60, 60, 255),
        Argb::from_rgba(48, 192, 48, 255),
        Argb::from_rgba(128, 255, 255, 128),
        Argb(0),
    ];
    prop::collection::vec(prop::sample::select(palette.to_vec()), w * h)
        .prop_map(move |pixels| ArgbImage::new(w, h, pixels))
}

/// (w, h, grid) pairs for the round-trip properties.
fn round_trip_cases() -> impl Strategy<Value = (usize, usize, ArgbImage)> {
    (1usize..=12, 1usize..=12)
        .prop_flat_map(|(w, h)| random_grid(w, h).prop_map(move |img| (w, h, img)))
}

/// (w, h, grid) pairs over the translucent palette.
fn translucent_cases() -> impl Strategy<Value = (usize, usize, ArgbImage)> {
    (1usize..=12, 1usize..=12)
        .prop_flat_map(|(w, h)| translucent_grid(w, h).prop_map(move |img| (w, h, img)))
}

/// (w, h, factor, grid) pairs for the full xBRZ pipeline property.
fn pipeline_cases() -> impl Strategy<Value = (usize, usize, u8, ArgbImage)> {
    (1usize..=8, 1usize..=8, 2u8..=3u8)
        .prop_flat_map(|(w, h, factor)| random_grid(w, h).prop_map(move |img| (w, h, factor, img)))
}

/// (w, h, factor, arbitrary pixels) for the engine fuzz property.
fn arbitrary_input_cases() -> impl Strategy<Value = (usize, usize, u8, Vec<u32>)> {
    (1usize..=16, 1usize..=16, 2u8..=6u8).prop_flat_map(|(w, h, factor)| {
        prop::collection::vec(any::<u32>(), w * h).prop_map(move |pixels| (w, h, factor, pixels))
    })
}

/// (tolerance, grid) pairs for the quantization properties.
fn quantize_cases() -> impl Strategy<Value = (f64, ArgbImage)> {
    (1.0f64..=100.0, 1usize..=12, 1usize..=12)
        .prop_flat_map(|(tol, w, h)| random_grid(w, h).prop_map(move |img| (tol, img)))
}

/// Shared round-trip body: vectorize, serialize, rasterize back, compare.
/// Uses `assert_eq!` (panic) rather than `prop_assert_eq!` because this is a
/// helper; proptest reports the failing case either way.
fn assert_round_trip(img: &ArgbImage) {
    let regions = vectorize(img, true);
    let svg = export(&regions, img.width, img.height);
    let raster = common::rasterize_svg(&svg, img.width, img.height);
    assert_eq!(&raster, &img.pixels);
}

/// Round trip through the full pipeline including the optimization pass:
/// vectorize, optimize, serialize, rasterize back, compare. The optimizer
/// must be invisible to the rendered output.
fn assert_optimized_round_trip(img: &ArgbImage) {
    let mut regions = vectorize(img, true);
    optimize(&mut regions);
    let svg = export(&regions, img.width, img.height);
    let raster = common::rasterize_svg(&svg, img.width, img.height);
    assert_eq!(&raster, &img.pixels);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The vectorizer must partition any grid exactly: SVG -> raster == input.
    #[test]
    fn vectorized_svg_rasterizes_back_to_exact_grid(case in round_trip_cases()) {
        let (_, _, img) = case;
        assert_round_trip(&img);
    }

    /// Same property on palettes that include a translucent color, which
    /// goes through `fill-opacity` in the serialized SVG.
    #[test]
    fn vectorized_svg_round_trips_with_translucency(case in translucent_cases()) {
        let (_, _, img) = case;
        assert_round_trip(&img);
    }

    /// Merging groups one region per distinct non-transparent color.
    #[test]
    fn merge_colors_yields_one_region_per_color(case in round_trip_cases()) {
        let (_, _, img) = case;
        let regions = vectorize(&img, true);
        let distinct: HashSet<Argb> = img
            .pixels
            .iter()
            .copied()
            .filter(|p| p.a() != 0)
            .collect();
        prop_assert_eq!(regions.len(), distinct.len());
        let region_colors: HashSet<Argb> = regions.iter().map(|r| r.color).collect();
        prop_assert_eq!(region_colors, distinct);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The optimizer must be invisible: stripping redundant control points
    /// and flattening loops never changes the rendered output.
    #[test]
    fn optimized_svg_rasterizes_back_to_exact_grid(case in round_trip_cases()) {
        let (_, _, img) = case;
        assert_optimized_round_trip(&img);
    }

    /// Same property on the translucent palette (exercises the fill-opacity
    /// serialization path through the optimizer).
    #[test]
    fn optimized_svg_round_trips_with_translucency(case in translucent_cases()) {
        let (_, _, img) = case;
        assert_optimized_round_trip(&img);
    }

    /// Optimization only removes points, never adds them, never shrinks a
    /// loop below three points, and is idempotent.
    #[test]
    fn optimize_is_idempotent_and_never_grows_paths(case in round_trip_cases()) {
        let (_, _, img) = case;
        let mut regions = vectorize(&img, true);
        let stats = optimize(&mut regions);
        prop_assert!(stats.points_after <= stats.points_before);
        prop_assert_eq!(
            stats.loops,
            regions.iter().map(|r| r.loops.len()).sum::<usize>()
        );
        for region in &regions {
            for loop_ in &region.loops {
                prop_assert!(
                    loop_.points.len() >= 3,
                    "optimizer shrank a loop below 3 points"
                );
            }
        }
        let mut twice = regions.clone();
        let second = optimize(&mut twice);
        prop_assert_eq!(twice, regions, "optimize must be idempotent");
        prop_assert_eq!(second.points_after, stats.points_after);
    }

    /// The xBRZ-scaling pipeline: scale a random grid, trace and serialize
    /// it, and rasterize back — must equal the scaled grid exactly.
    #[test]
    fn xbrz_to_svg_pipeline_rasterizes_back_exactly(case in pipeline_cases()) {
        let (_, _, factor, img) = case;
        let scaled = scale_image(&img, factor, &ScalerConfig::default());
        let regions = vectorize(&scaled, true);
        let svg = export(&regions, scaled.width, scaled.height);
        let raster = common::rasterize_svg(&svg, scaled.width, scaled.height);
        prop_assert_eq!(&raster, &scaled.pixels);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// xBRZ never panics on arbitrary pixels, always reports the correct
    /// output dimensions, and is deterministic (a pure function of input).
    #[test]
    fn xbrz_is_deterministic_and_panic_free_on_arbitrary_input(case in arbitrary_input_cases()) {
        let (w, h, factor, pixels) = case;
        let img = ArgbImage::new(w, h, pixels.iter().map(|&v| Argb(v)).collect());
        let a = scale_image(&img, factor, &ScalerConfig::default());
        let b = scale_image(&img, factor, &ScalerConfig::default());
        prop_assert_eq!(a.width, w * factor as usize);
        prop_assert_eq!(a.height, h * factor as usize);
        prop_assert_eq!(a.pixels, b.pixels);
    }

    /// A uniform image must scale to the same uniform color (the reference
    /// algorithm's no-blend path for identical neighbors).
    #[test]
    fn uniform_image_stays_uniform(
        color in any::<u32>(),
        factor in 2u8..=6u8,
        w in 1usize..=16,
        h in 1usize..=16,
    ) {
        let img = ArgbImage::new(w, h, vec![Argb(color); w * h]);
        let out = scale_image(&img, factor, &ScalerConfig::default());
        prop_assert_eq!(out.width, w * factor as usize);
        prop_assert_eq!(out.height, h * factor as usize);
        prop_assert!(out.pixels.iter().all(|&p| p == Argb(color)));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Quantization is deterministic and never moves a pixel's color further
    /// than the tolerance (its central contract).
    #[test]
    fn quantize_is_deterministic_and_stays_within_tolerance(case in quantize_cases()) {
        let (tol, img) = case;
        let a = quantize(&img, tol);
        let b = quantize(&img, tol);
        prop_assert_eq!(&a.pixels, &b.pixels);
        prop_assert_eq!((a.width, a.height), (img.width, img.height));
        for (&orig, &q) in img.pixels.iter().zip(a.pixels.iter()) {
            prop_assert!(
                color_dist(orig, q) <= tol,
                "pixel {orig:?} moved to {q:?}, distance {} > {tol}",
                color_dist(orig, q)
            );
        }
    }

    /// The quantized grid is still a valid grid: it vectorizes and
    /// rasterizes back exactly.
    #[test]
    fn quantized_grid_round_trips_through_svg(case in quantize_cases()) {
        let (tol, img) = case;
        let q = quantize(&img, tol);
        let regions = vectorize(&q, true);
        let svg = export(&regions, q.width, q.height);
        let raster = common::rasterize_svg(&svg, q.width, q.height);
        prop_assert_eq!(&raster, &q.pixels);
    }

    /// A zero tolerance is an identity operation.
    #[test]
    fn quantize_zero_tolerance_is_identity(case in round_trip_cases()) {
        let (_, _, img) = case;
        let q = quantize(&img, 0.0);
        prop_assert_eq!(q.pixels, img.pixels);
    }
}
