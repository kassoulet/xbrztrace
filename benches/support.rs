//! Shared helpers for the criterion benches: deterministic pixel-art scene
//! generation. Kept dependency-free (no rand) so benches are reproducible.

use xbrztrace::xbrz_engine::{Argb, ArgbImage};

/// SplitMix64 — tiny deterministic PRNG (public-domain algorithm).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn opaque(rng: &mut Rng) -> Argb {
    Argb::from_rgba(
        rng.below(256) as u8,
        rng.below(256) as u8,
        rng.below(256) as u8,
        255,
    )
}

/// A deterministic pixel-art scene: transparent background, solid-color
/// rectangles, one-pixel-wide diagonal lines and a checkerboard patch.
/// This gives the scaler corners, edges, diagonals and transparent holes to
/// chew on — representative of real sprite work.
pub fn scene(w: usize, h: usize) -> ArgbImage {
    let mut rng = Rng(0xB2A9_5E9D_1F0C_8A71);
    let mut pixels = vec![Argb(0); w * h];

    let set = |pixels: &mut [Argb], x: usize, y: usize, c: Argb| {
        pixels[y * w + x] = c;
    };

    // Solid rectangles.
    for _ in 0..(w * h / 12).max(6) {
        let c = opaque(&mut rng);
        let x0 = rng.below(w as u64) as usize;
        let y0 = rng.below(h as u64) as usize;
        let x1 = (x0 + 1 + rng.below(6) as usize).min(w);
        let y1 = (y0 + 1 + rng.below(6) as usize).min(h);
        for y in y0..y1 {
            for x in x0..x1 {
                set(&mut pixels, x, y, c);
            }
        }
    }

    // One-pixel diagonal lines (top-left to bottom-right).
    for _ in 0..4 {
        let c = opaque(&mut rng);
        let x0 = rng.below(w as u64) as usize;
        let y0 = rng.below(h as u64) as usize;
        let len = (w.max(h) / 4).max(4);
        for i in 0..len {
            let x = x0 + i;
            let y = y0 + i;
            if x < w && y < h {
                set(&mut pixels, x, y, c);
            }
        }
    }

    // A checkerboard patch.
    let c1 = opaque(&mut rng);
    let c2 = opaque(&mut rng);
    let (x0, y0) = (
        rng.below(w.saturating_sub(4) as u64) as usize,
        rng.below(h.saturating_sub(4) as u64) as usize,
    );
    let size = 4usize.min(w - x0).min(h - y0);
    for y in y0..y0 + size {
        for x in x0..x0 + size {
            let c = if (x + y) % 2 == 0 { c1 } else { c2 };
            set(&mut pixels, x, y, c);
        }
    }

    ArgbImage::new(w, h, pixels)
}
