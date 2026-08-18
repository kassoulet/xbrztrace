//! Shared test helpers: ASCII-art grids -> ArgbImage, plus the standard
//! patterns used by the fixture and vectorizer tests.
//! Each integration-test binary only uses a subset of these helpers.
#![allow(dead_code)]

use brztracer::xbrz_engine::{Argb, ArgbImage};

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
