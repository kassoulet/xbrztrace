//! Shared test helpers: ASCII-art grids -> ArgbImage, the standard patterns
//! used by the fixture and vectorizer tests, and an independent SVG
//! rasterizer used for round-trip verification.
//! Each integration-test binary only uses a subset of these helpers.
#![allow(dead_code)]

use xbrztrace::xbrz_engine::{Argb, ArgbImage};

/// Build an image from rows of palette characters. All rows must have the
/// same length; unknown characters map to fully transparent black.
pub fn grid_from_rows(rows: &[&str], palette: &[(char, Argb)]) -> ArgbImage {
    let h = rows.len();
    let w = rows[0].len();
    let mut pixels = Vec::with_capacity(w * h);
    for row in rows {
        assert_eq!(row.len(), w, "ragged test row: {row:?}");
        for ch in row.chars() {
            let color = palette
                .iter()
                .find(|(c, _)| *c == ch)
                .map(|(_, a)| *a)
                .unwrap_or(Argb(0));
            pixels.push(color);
        }
    }
    ArgbImage::new(w, h, pixels)
}

/// Pattern "diag": a diagonal edge across the whole image (exercises line
/// blending). ARGB values identical to the fixture generator's.
pub fn pattern_diag() -> ArgbImage {
    let rows = [
        "AAAAAABB", "AAAAABBB", "AAAABBBB", "AAABBBBB", "AABBBBBB", "ABBBBBBB", "BBBBBBBB",
        "BBBBBBBB",
    ];
    let palette = [
        ('A', Argb::from_rgba(255, 80, 80, 255)),
        ('B', Argb::from_rgba(48, 192, 48, 255)),
    ];
    grid_from_rows(&rows, &palette)
}

/// Pattern "mix": solid blocks, L-corners, a semi-transparent island, an
/// isolated transparent pixel and a checkerboard row.
pub fn pattern_mix() -> ArgbImage {
    let rows = [
        "AAAABBBB", "AAAABBBB", "AACCCCBB", "AACDDCBB", "AACDDCBB", "AACCCCBB", "AEAAABBB",
        "ABABABAB",
    ];
    let palette = [
        ('A', Argb::from_rgba(255, 80, 80, 255)),
        ('B', Argb::from_rgba(48, 192, 48, 255)),
        ('C', Argb::from_rgba(255, 255, 255, 255)),
        ('D', Argb::from_rgba(128, 255, 255, 128)),
        ('E', Argb::from_rgba(0, 0, 0, 0)),
    ];
    grid_from_rows(&rows, &palette)
}

/// The 16x16 test sprite: a little ghost with outline, body, eyes and pupils
/// on a transparent background.
pub fn ghost_palette() -> [(char, Argb); 5] {
    [
        ('.', Argb::from_rgba(0, 0, 0, 0)),
        ('#', Argb::from_rgba(40, 40, 48, 255)),
        ('R', Argb::from_rgba(220, 60, 60, 255)),
        ('W', Argb::from_rgba(240, 240, 245, 255)),
        ('K', Argb::from_rgba(20, 20, 25, 255)),
    ]
}

pub fn ghost() -> ArgbImage {
    let rows = [
        "................",
        "..##########....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        ".#RWWWWWWRR#....",
        ".#RWWWWWWRR#....",
        ".#RWWWWWWRR#....",
        ".#RRRRRRRRR#....",
        ".#RKKRKKRRR#....",
        ".#RKKRKKRRR#....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        "..####..####....",
        "................",
        "................",
    ];
    grid_from_rows(&rows, &ghost_palette())
}

// ---------------------------------------------------------------------------
// Independent SVG rasterizer (test-only)
// ---------------------------------------------------------------------------
// Fills a grid using the even-odd rule against the vertical edges of every
// loop, sampling pixel centers. Deliberately simple and independent of the
// code under test: it only knows the SVG text format emitted by the exporter.

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
pub fn rasterize_svg(svg: &str, w: usize, h: usize) -> Vec<Argb> {
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
