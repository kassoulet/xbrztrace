//! End-to-end vectorization tests on the 16x16 test sprite.
//!
//! The key property under test: rasterizing the generated SVG back into a
//! pixel grid must reproduce the upscaled image *exactly*. This catches any
//! seams, gaps, overlapping fills or geometry errors introduced by tracing.

mod common;

use std::collections::HashSet;

use brztracer::svg_exporter::export;
use brztracer::vectorizer::vectorize;
use brztracer::xbrz_engine::{scale_image, Argb, ArgbImage, ScalerConfig};

// ---------------------------------------------------------------------------
// Minimal SVG path rasterizer (test-only): fills a grid with even-odd rule.
// ---------------------------------------------------------------------------

fn read_num(bytes: &[u8], mut i: usize) -> (i32, usize) {
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    let start = i;
    if i < bytes.len() && bytes[i] == b'-' {
        i += 1;
    }
    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
        i += 1;
    }
    assert!(i > start, "expected a number in path data");
    (
        std::str::from_utf8(&bytes[start..i])
            .unwrap()
            .parse()
            .unwrap(),
        i,
    )
}

/// Split a path `d` value into loops of vertices (implicitly closed).
fn parse_path_data(d: &str) -> Vec<Vec<(i32, i32)>> {
    let bytes = d.as_bytes();
    let mut i = 0;
    let mut loops = Vec::new();
    let mut cur: Vec<(i32, i32)> = Vec::new();
    while i < bytes.len() {
        match bytes[i] as char {
            'M' | 'L' => {
                i += 1;
                let (x, n) = read_num(bytes, i);
                i = n;
                let (y, n) = read_num(bytes, i);
                i = n;
                cur.push((x, y));
            }
            'H' => {
                i += 1;
                let (x, n) = read_num(bytes, i);
                i = n;
                let (_, y) = *cur.last().unwrap();
                cur.push((x, y));
            }
            'V' => {
                i += 1;
                let (y, n) = read_num(bytes, i);
                i = n;
                let (x, _) = *cur.last().unwrap();
                cur.push((x, y));
            }
            'Z' => {
                loops.push(std::mem::take(&mut cur));
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            other => panic!("unexpected character {other:?} in path data"),
        }
    }
    if !cur.is_empty() {
        loops.push(cur);
    }
    loops
}

/// Vertical edges (x, y_min, y_max) of a loop, including the closing edge.
fn vertical_edges(loop_: &[(i32, i32)]) -> Vec<(i32, i32, i32)> {
    let mut out = Vec::new();
    for i in 0..loop_.len() {
        let (x1, y1) = loop_[i];
        let (x2, y2) = loop_[(i + 1) % loop_.len()];
        if x1 == x2 {
            out.push((x1, y1.min(y2), y1.max(y2)));
        }
    }
    out
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn parse_hex_color(s: &str) -> (u8, u8, u8) {
    let s = s.strip_prefix('#').unwrap();
    assert_eq!(s.len(), 6);
    (
        u8::from_str_radix(&s[0..2], 16).unwrap(),
        u8::from_str_radix(&s[2..4], 16).unwrap(),
        u8::from_str_radix(&s[4..6], 16).unwrap(),
    )
}

/// Rasterize an SVG produced by `svg_exporter::export` back into a grid.
fn rasterize_svg(svg: &str, w: usize, h: usize) -> Vec<Argb> {
    let mut grid = vec![Argb(0); w * h];
    for tag in svg.split("<path ").skip(1) {
        let d = attr(tag, "d").expect("path without d");
        let fill = attr(tag, "fill").expect("path without fill");
        let opacity: f64 = attr(tag, "fill-opacity")
            .map(|s| s.parse().unwrap())
            .unwrap_or(1.0);
        let (r, g, b) = parse_hex_color(fill);
        let alpha = (255.0 * opacity).round() as u8;
        let color = Argb::from_rgba(r, g, b, alpha);

        let edges: Vec<(i32, i32, i32)> = parse_path_data(d)
            .iter()
            .flat_map(|l| vertical_edges(l))
            .collect();

        for y in 0..h {
            let cy = y as f64 + 0.5;
            for x in 0..w {
                let cx = x as f64 + 0.5;
                let crossings = edges
                    .iter()
                    .filter(|(ex, ey1, ey2)| {
                        let crosses_x = *ex as f64 > cx;
                        let spans_y = (*ey1 as f64) < cy && cy < (*ey2 as f64);
                        crosses_x && spans_y
                    })
                    .count();
                if crossings % 2 == 1 {
                    grid[y * w + x] = color;
                }
            }
        }
    }
    grid
}

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
        let raster = rasterize_svg(&svg, scaled.width, scaled.height);
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
        let raster = rasterize_svg(&svg, scaled.width, scaled.height);
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
