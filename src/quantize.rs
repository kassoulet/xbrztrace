//! Color quantization: merge near-duplicate colors before vectorization.
//!
//! Lossy inputs (JPEG) carry compression noise — a flat region arrives as
//! dozens of subtly different colors, each of which would become its own
//! region and blow up the SVG. [`quantize`] collapses every cluster of colors
//! that are within `tolerance` of a representative into that representative,
//! turning noise back into clean flat regions.
//!
//! The distance metric is the engine's alpha-aware perceptual distance
//! ([`crate::xbrz_engine::color_dist`]), so "similar" means the same thing
//! here as in the scaler's equal-color test.
//!
//! # Algorithm
//!
//! Greedy clustering: distinct colors are visited in order of frequency
//! (most frequent first, ARGB as tie-break), so the dominant shade of each
//! noise cloud becomes its cluster's representative. Each color either joins
//! the first representative within `tolerance`, or starts a new cluster.
//!
//! To keep this near-linear instead of O(n·k), representatives are indexed
//! in a spatial hash over RGB space. A cell spans `2·tolerance` RGB units;
//! the YCbCr transform used by `color_dist` contracts distances (its largest
//! singular value is well below 1), so any two colors within `tolerance` in
//! perceptual distance differ by at most `2·tolerance` in every RGB channel
//! and always land in the same or an adjacent cell — checking the 27-cell
//! neighborhood therefore finds every mergeable representative. (The only
//! exception is two very transparent pixels with wildly different RGB: their
//! alpha-scaled distance can drop below tolerance across distant cells. Such
//! a miss is harmless — those pixels are nearly invisible, and each still
//! maps to a color within tolerance of itself.)
//!
//! The result is deterministic: the same input and tolerance always produce
//! the same output image.

use std::collections::HashMap;

use crate::xbrz_engine::{color_dist, Argb, ArgbImage};

/// Merge near-duplicate colors in `img` into representatives, replacing every
/// pixel with its cluster's color.
///
/// A color joins a cluster when its perceptual distance to the cluster's
/// representative is `<= tolerance` (see [`crate::xbrz_engine::color_dist`]
/// for the metric and scale — the engine's default equal-color threshold is
/// 30). Tolerances `<= 0` return the image unchanged.
pub fn quantize(img: &ArgbImage, tolerance: f64) -> ArgbImage {
    if tolerance <= 0.0 || img.pixels.is_empty() {
        return img.clone();
    }

    // Count distinct colors; frequency decides representative order.
    let mut counts: HashMap<Argb, usize> = HashMap::with_capacity(img.pixels.len() / 4 + 1);
    for &p in &img.pixels {
        *counts.entry(p).or_insert(0) += 1;
    }
    if counts.len() <= 1 {
        return img.clone();
    }

    // Most frequent first (ties by ARGB): the dominant shade becomes the rep.
    let mut order: Vec<Argb> = counts.keys().copied().collect();
    order.sort_by(|a, b| counts[b].cmp(&counts[a]).then(a.0.cmp(&b.0)));

    let cell = (2.0 * tolerance).max(1.0) as u32;
    let max_idx = 255 / cell;
    let cell_key =
        |c: Argb| -> u32 { (c.r() / cell) | ((c.g() / cell) << 8) | ((c.b() / cell) << 16) };

    // Spatial hash: cell -> cluster ids whose representative lives there.
    let mut cells: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut reps: Vec<Argb> = Vec::new();
    let mut remap: HashMap<Argb, Argb> = HashMap::with_capacity(counts.len());

    for &c in &order {
        let (rk, gk, bk) = (c.r() / cell, c.g() / cell, c.b() / cell);
        let mut rep: Option<Argb> = None;
        'search: for dr in -1i64..=1 {
            for dg in -1i64..=1 {
                for db in -1i64..=1 {
                    let key = ((rk as i64 + dr).clamp(0, max_idx as i64) as u32)
                        | (((gk as i64 + dg).clamp(0, max_idx as i64) as u32) << 8)
                        | (((bk as i64 + db).clamp(0, max_idx as i64) as u32) << 16);
                    if let Some(ids) = cells.get(&key) {
                        for &id in ids {
                            if color_dist(c, reps[id]) <= tolerance {
                                rep = Some(reps[id]);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }

        match rep {
            Some(r) => {
                remap.insert(c, r);
            }
            None => {
                remap.insert(c, c);
                cells.entry(cell_key(c)).or_default().push(reps.len());
                reps.push(c);
            }
        }
    }

    let pixels = img.pixels.iter().map(|&p| remap[&p]).collect();
    ArgbImage::new(img.width, img.height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn row(colors: &[Argb]) -> ArgbImage {
        ArgbImage::new(colors.len(), 1, colors.to_vec())
    }

    fn distinct(img: &ArgbImage) -> HashSet<Argb> {
        img.pixels.iter().copied().collect()
    }

    #[test]
    fn near_duplicates_merge_into_one_color() {
        let base = Argb::from_rgba(200, 100, 50, 255);
        let a = Argb::from_rgba(204, 102, 52, 255);
        let b = Argb::from_rgba(196, 98, 48, 255);
        let out = quantize(&row(&[base, a, b]), 30.0);
        assert_eq!(distinct(&out).len(), 1);
        // Every output color is within tolerance of its input.
        for (orig, q) in [base, a, b].iter().zip(out.pixels.iter()) {
            assert!(color_dist(*orig, *q) <= 30.0);
        }
    }

    #[test]
    fn distant_colors_are_kept() {
        let red = Argb::from_rgba(255, 0, 0, 255);
        let blue = Argb::from_rgba(0, 0, 255, 255);
        let out = quantize(&row(&[red, blue]), 30.0);
        assert_eq!(distinct(&out).len(), 2);
    }

    #[test]
    fn most_frequent_shade_becomes_representative() {
        // The majority variant (appears 3x) must win as the cluster rep.
        let main = Argb::from_rgba(200, 100, 50, 255);
        let v1 = Argb::from_rgba(202, 98, 52, 255);
        let v2 = Argb::from_rgba(198, 102, 48, 255);
        let out = quantize(&row(&[v1, main, v2, main, main]), 30.0);
        assert!(out.pixels.iter().all(|&p| p == main));
    }

    #[test]
    fn tolerance_bounds_the_maximum_shift() {
        // A gradient of colors each 15 apart (perceptual distance ~9):
        // clusters chain, but every pixel must land within tolerance of its
        // original color.
        let mut colors = Vec::new();
        for i in 0..10u8 {
            colors.push(Argb::from_rgba(100 + i * 15, 50, 50, 255));
        }
        let img = row(&colors);
        let out = quantize(&img, 30.0);
        assert!(distinct(&out).len() < colors.len(), "expected some merging");
        for (orig, q) in img.pixels.iter().zip(out.pixels.iter()) {
            assert!(color_dist(*orig, *q) <= 30.0, "{orig:?} -> {q:?}");
        }
    }

    #[test]
    fn transparent_and_opaque_do_not_merge() {
        let opaque = Argb::from_rgba(200, 100, 50, 255);
        let transparent = Argb::from_rgba(200, 100, 50, 0);
        let out = quantize(&row(&[opaque, transparent]), 100.0);
        assert_eq!(distinct(&out).len(), 2);
    }

    #[test]
    fn transparent_pixels_with_close_rgb_merge() {
        // Fully transparent pixels are distance 0 apart, so ones with close
        // RGB collapse into a single representative. (Transparent pixels
        // with wildly different RGB may stay separate — the spatial hash
        // only compares near neighbors; harmless, since the vectorizer
        // skips fully transparent pixels anyway.)
        let t1 = Argb(0x00000000);
        let t2 = Argb(0x00010101);
        let out = quantize(&row(&[t1, t2]), 30.0);
        assert_eq!(distinct(&out).len(), 1);
    }

    #[test]
    fn zero_or_negative_tolerance_is_a_no_op() {
        let img = row(&[
            Argb::from_rgba(200, 100, 50, 255),
            Argb::from_rgba(204, 102, 52, 255),
        ]);
        assert_eq!(quantize(&img, 0.0).pixels, img.pixels);
        assert_eq!(quantize(&img, -5.0).pixels, img.pixels);
    }

    #[test]
    fn single_color_and_empty_images_are_unchanged() {
        let solid = ArgbImage::new(4, 4, vec![Argb::from_rgba(1, 2, 3, 255); 16]);
        assert_eq!(quantize(&solid, 30.0).pixels, solid.pixels);
        let empty = ArgbImage::new(0, 0, vec![]);
        assert_eq!(quantize(&empty, 30.0).pixels, empty.pixels);
    }

    #[test]
    fn output_is_deterministic() {
        let img = row(&[
            Argb::from_rgba(200, 100, 50, 255),
            Argb::from_rgba(204, 102, 52, 255),
            Argb::from_rgba(10, 200, 90, 255),
            Argb::from_rgba(12, 198, 92, 255),
            Argb(0),
        ]);
        assert_eq!(quantize(&img, 30.0).pixels, quantize(&img, 30.0).pixels);
    }
}
