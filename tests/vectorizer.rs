//! End-to-end vectorization tests on the 16x16 test sprite.
//!
//! The key property under test: rasterizing the generated SVG back into a
//! pixel grid must reproduce the upscaled image *exactly*. This catches any
//! seams, gaps, overlapping fills or geometry errors introduced by tracing.

mod common;

use std::collections::HashSet;

use xbrztrace::svg_exporter::export;
use xbrztrace::vectorizer::vectorize;
use xbrztrace::xbrz_engine::{scale_image, Argb, ArgbImage, ScalerConfig};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn round_trip_16x16_sprite_is_exact() {
    let sprite = common::ghost();
    for factor in [2u8, 3u8, 4u8, 6u8] {
        let scaled = scale_image(&sprite, factor, &ScalerConfig::default());
        let regions = vectorize(&scaled, true);
        let svg = export(&regions, scaled.width, scaled.height);
        let raster = common::rasterize_svg(&svg, scaled.width, scaled.height);
        assert_eq!(
            raster,
            scaled.pixels,
            "SVG round-trip mismatch at {factor}x ({} colors, {} paths)",
            regions.len(),
            regions.len()
        );
    }
}

#[test]
fn round_trip_diagonal_pattern_is_exact() {
    // A pure diagonal edge produces heavy xBRZ blending; the traced paths
    // must still partition the output exactly.
    let src = common::pattern_diag();
    for factor in [2u8, 4u8, 5u8] {
        let scaled = scale_image(&src, factor, &ScalerConfig::default());
        let regions = vectorize(&scaled, true);
        let svg = export(&regions, scaled.width, scaled.height);
        let raster = common::rasterize_svg(&svg, scaled.width, scaled.height);
        assert_eq!(raster, scaled.pixels, "round-trip mismatch at {factor}x");
    }
}

#[test]
fn merged_paths_equal_color_count() {
    let sprite = common::ghost();
    let scaled = scale_image(&sprite, 4, &ScalerConfig::default());

    let expected_colors: HashSet<Argb> = scaled
        .pixels
        .iter()
        .copied()
        .filter(|p| p.a() != 0)
        .collect();

    let merged = vectorize(&scaled, true);
    assert_eq!(merged.len(), expected_colors.len());

    // Every merged region has a distinct, non-transparent color.
    let colors: HashSet<Argb> = merged.iter().map(|r| r.color).collect();
    assert_eq!(colors.len(), merged.len());
    assert!(merged.iter().all(|r| r.color.a() != 0));
}

#[test]
fn unmerged_paths_have_more_elements() {
    let sprite = common::ghost();
    let scaled = scale_image(&sprite, 4, &ScalerConfig::default());
    let merged = vectorize(&scaled, true);
    let unmerged = vectorize(&scaled, false);
    assert!(unmerged.len() > merged.len());
    // Without merging, every region is a single loop.
    assert!(unmerged.iter().all(|r| r.loops.len() == 1));
}

#[test]
fn svg_has_correct_dimensions_and_viewbox() {
    let sprite = common::ghost();
    let scaled = scale_image(&sprite, 4, &ScalerConfig::default());
    let regions = vectorize(&scaled, true);
    let svg = export(&regions, scaled.width, scaled.height);
    assert!(svg.contains(&format!(
        "width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"",
        scaled.width, scaled.height, scaled.width, scaled.height
    )));
    assert!(svg.contains("shape-rendering=\"crispEdges\""));
}

#[test]
fn ghost_colors_appear_in_output() {
    let sprite = common::ghost();
    let scaled = scale_image(&sprite, 4, &ScalerConfig::default());
    let regions = vectorize(&scaled, true);
    let svg = export(&regions, scaled.width, scaled.height);

    // The outline color must survive xBRZ as a region of its own.
    let outline = Argb::from_rgba(40, 40, 48, 255);
    assert!(
        regions.iter().any(|r| r.color == outline),
        "outline color missing from vectorized output"
    );
    assert!(svg.contains("fill=\"#282830\""));
}

#[test]
fn deterministic_output() {
    let sprite = common::ghost();
    let scaled = scale_image(&sprite, 4, &ScalerConfig::default());
    let a = export(&vectorize(&scaled, true), scaled.width, scaled.height);
    let b = export(&vectorize(&scaled, true), scaled.width, scaled.height);
    assert_eq!(a, b);
}

#[test]
fn transparent_only_image_yields_empty_svg() {
    let img = ArgbImage::new(8, 8, vec![Argb(0); 64]);
    let regions = vectorize(&img, true);
    assert!(regions.is_empty());
    let svg = export(&regions, 8, 8);
    assert!(!svg.contains("<path"));
}
