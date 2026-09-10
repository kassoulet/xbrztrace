//! SVG serialization: emit valid, compact `<svg>` markup from traced regions.

use std::fmt::Write;

use crate::vectorizer::Region;
use crate::xbrz_engine::Argb;

/// Serialize the traced regions into an SVG document of the given pixel
/// dimensions (which are the dimensions of the upscaled image, and thus the
/// natural `viewBox` of the output).
///
/// - Fills are emitted as hex colors; colors with alpha < 255 also get a
///   `fill-opacity` attribute.
/// - Every region becomes one `<path>` whose loops are concatenated as
///   subpaths with `fill-rule="evenodd"`, so holes render correctly.
/// - `shape-rendering="crispEdges"` plus integer grid coordinates keep
///   adjacent shapes perfectly flush — no antialiasing seams or hairline gaps.
pub fn export(regions: &[Region], width: usize, height: usize) -> String {
    let mut svg = String::with_capacity(256 + regions.len() * 96);
    writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" shape-rendering=\"crispEdges\">"
    )
    .unwrap();

    for region in regions {
        let mut d = String::new();
        for loop_ in &region.loops {
            write_path_data(&mut d, loop_);
        }
        write!(
            svg,
            "  <path d=\"{d}\" fill=\"{}\"",
            hex_color(region.color)
        )
        .unwrap();
        if region.color.a() < 255 {
            write!(svg, " fill-opacity=\"{}\"", region.color.a() as f64 / 255.0).unwrap();
        }
        // `fill-rule="evenodd"` only matters when a path carries multiple
        // subpaths (holes or disconnected loops of one color); for a single
        // loop the default `nonzero` rule fills identically, so the
        // attribute is dropped (SVGO's removeUnknownsAndDefaults style).
        if region.loops.len() > 1 {
            svg.push_str(" fill-rule=\"evenodd\"");
        }
        svg.push_str("/>\n");
    }
    svg.push_str("</svg>\n");
    svg
}

fn hex_color(color: Argb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// Append the path data (`M`/`H`/`V`/`L`/`Z`) for one loop. Consecutive
/// collinear segments were already merged by the vectorizer, so horizontal
/// and vertical runs emit as single `H`/`V` commands.
#[inline]
fn push_i32(s: &mut String, n: i32) {
    if n == 0 {
        s.push('0');
        return;
    }
    if n < 0 {
        s.push('-');
    }
    let mut val = n.unsigned_abs();
    let mut buf = [0u8; 10];
    let mut i = 10;
    while val > 0 {
        i -= 1;
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    // SAFETY: buf contains only ASCII digits ('0'..'9')
    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[i..]) });
}

/// Append the path data (`M`/`H`/`V`/`L`/`Z`) for one loop. Consecutive
/// collinear segments were already merged by the vectorizer, so horizontal
/// and vertical runs emit as single `H`/`V` commands.
fn write_path_data(d: &mut String, loop_: &crate::vectorizer::PathLoop) {
    let points = &loop_.points;
    if points.len() < 3 {
        return;
    }
    d.push('M');
    push_i32(d, points[0].0);
    d.push(' ');
    push_i32(d, points[0].1);

    for pair in points.windows(2) {
        let (px, py) = pair[0];
        let (cx, cy) = pair[1];
        if py == cy {
            d.push('H');
            push_i32(d, cx);
        } else if px == cx {
            d.push('V');
            push_i32(d, cy);
        } else {
            d.push('L');
            push_i32(d, cx);
            d.push(' ');
            push_i32(d, cy);
        }
    }
    d.push('Z');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectorizer::{vectorize, PathLoop};
    use crate::xbrz_engine::ArgbImage;

    #[test]
    fn exports_simple_square() {
        let mut pixels = vec![Argb(0); 4 * 4];
        let red = Argb::from_rgba(255, 80, 80, 255);
        for y in 1..3 {
            for x in 1..3 {
                pixels[y * 4 + x] = red;
            }
        }
        let img = ArgbImage::new(4, 4, pixels);
        let regions = vectorize(&img, true);
        let svg = export(&regions, 4, 4);

        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\" viewBox=\"0 0 4 4\""));
        assert!(svg.contains("fill=\"#ff5050\""));
        // The 2x2 square collapses to the four corner vertices after
        // simplification: M1 1 V3 H3 V1 Z (start, down, right, up, close).
        assert!(svg.contains("M1 1V3H3V1Z"), "unexpected path data: {svg}");
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn opaque_color_has_no_fill_opacity() {
        let img = ArgbImage::new(1, 1, vec![Argb::from_rgba(1, 2, 3, 255)]);
        let svg = export(&vectorize(&img, true), 1, 1);
        assert!(svg.contains("fill=\"#010203\""));
        assert!(!svg.contains("fill-opacity"));
    }

    #[test]
    fn translucent_color_gets_fill_opacity() {
        let img = ArgbImage::new(1, 1, vec![Argb::from_rgba(200, 100, 50, 128)]);
        let svg = export(&vectorize(&img, true), 1, 1);
        assert!(svg.contains("fill=\"#c86432\""));
        assert!(svg.contains("fill-opacity=\"0.5019607843137255\""));
    }

    #[test]
    fn empty_regions_produce_minimal_document() {
        let svg = export(&[], 8, 8);
        assert_eq!(
            svg,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\" viewBox=\"0 0 8 8\" shape-rendering=\"crispEdges\">\n</svg>\n"
        );
    }

    #[test]
    fn single_loop_path_omits_redundant_fill_rule() {
        let img = ArgbImage::new(1, 1, vec![Argb::from_rgba(1, 2, 3, 255)]);
        let svg = export(&vectorize(&img, true), 1, 1);
        assert!(svg.contains("fill=\"#010203\""));
        assert!(!svg.contains("fill-rule"), "single loop needs no fill-rule");
    }

    #[test]
    fn multi_loop_path_keeps_evenodd_fill_rule() {
        // Two loops of one color: one region with two subpaths, which needs
        // even-odd fill to render holes correctly.
        let loop_a = PathLoop {
            points: vec![(0, 0), (2, 0), (2, 2), (0, 2)],
        };
        let loop_b = PathLoop {
            points: vec![(4, 4), (6, 4), (6, 6), (4, 6)],
        };
        let regions = vec![Region {
            color: Argb::from_rgba(0, 0, 0, 255),
            loops: vec![loop_a, loop_b],
        }];
        let svg = export(&regions, 8, 8);
        assert!(svg.contains("fill-rule=\"evenodd\""));
    }

    #[test]
    fn merged_path_contains_multiple_loops() {
        let loop_a = PathLoop {
            points: vec![(0, 0), (2, 0), (2, 2), (0, 2)],
        };
        let loop_b = PathLoop {
            points: vec![(4, 4), (6, 4), (6, 6), (4, 6)],
        };
        let regions = vec![Region {
            color: Argb::from_rgba(0, 0, 0, 255),
            loops: vec![loop_a, loop_b],
        }];
        let svg = export(&regions, 8, 8);
        assert!(svg.contains("M0 0H2V2H0ZM4 4H6V6H4Z"));
    }

    #[test]
    fn push_i32_handles_min_and_negative_values() {
        let mut s = String::new();
        push_i32(&mut s, 0);
        assert_eq!(s, "0");

        s.clear();
        push_i32(&mut s, 12345);
        assert_eq!(s, "12345");

        s.clear();
        push_i32(&mut s, -12345);
        assert_eq!(s, "-12345");

        s.clear();
        push_i32(&mut s, i32::MIN);
        assert_eq!(s, "-2147483648");

        s.clear();
        push_i32(&mut s, i32::MAX);
        assert_eq!(s, "2147483647");
    }

    #[test]
    fn degenerate_loops_handled_gracefully() {
        let empty_loop = PathLoop { points: vec![] };
        let single_point_loop = PathLoop {
            points: vec![(1, 1)],
        };
        let regions = vec![Region {
            color: Argb::from_rgba(255, 0, 0, 255),
            loops: vec![empty_loop, single_point_loop],
        }];
        let svg = export(&regions, 4, 4);
        assert!(svg.contains("d=\"\""));
        assert!(svg.contains("fill=\"#ff0000\""));
    }
}
