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
        svg.push_str(" fill-rule=\"evenodd\"/>\n");
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
fn write_path_data(d: &mut String, loop_: &crate::vectorizer::PathLoop) {
    let points = &loop_.points;
    debug_assert!(points.len() >= 3);
    write!(d, "M{} {}", points[0].0, points[0].1).unwrap();
    for pair in points.windows(2) {
        let (px, py) = pair[0];
        let (cx, cy) = pair[1];
        if py == cy {
            write!(d, "H{}", cx).unwrap();
        } else if px == cx {
            write!(d, "V{}", cy).unwrap();
        } else {
            write!(d, "L{} {}", cx, cy).unwrap();
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
}
