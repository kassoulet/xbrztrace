//! xBRZ pixel-art scaling engine.
//!
//! A from-scratch Rust implementation of the xBRZ algorithm (originally by
//! Zenju), supporting 2x–6x scaling with alpha-channel support. The
//! implementation is written against the published algorithm and is verified
//! bit-for-bit against the reference C++ implementation via golden fixtures
//! (see `tests/xbrz_reference.rs`).
//!
//! Pipeline per input pixel:
//!   1. Preprocessing: a 4x4 neighborhood is inspected and each of the
//!      pixel's four corners is classified as "no blend", "normal blend" or
//!      "dominant blend" based on a YCbCr color-distance edge heuristic.
//!   2. Blending: a 3x3 neighborhood is evaluated in four rotations; each
//!      corner applies a scale-factor-specific weight table (straight, steep
//!      or diagonal line blending, or a rounded corner) to the subpixels
//!      adjacent to it.

/// A 32-bit ARGB pixel, alpha in the most significant byte.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Argb(pub u32);

impl Argb {
    #[inline]
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Argb {
        Argb(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    #[inline]
    pub fn a(self) -> u32 {
        self.0 >> 24
    }

    #[inline]
    pub fn r(self) -> u32 {
        (self.0 >> 16) & 0xff
    }

    #[inline]
    pub fn g(self) -> u32 {
        (self.0 >> 8) & 0xff
    }

    #[inline]
    pub fn b(self) -> u32 {
        self.0 & 0xff
    }
}

/// An RGBA pixel grid.
#[derive(Clone, Debug)]
pub struct ArgbImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Argb>,
}

impl ArgbImage {
    pub fn new(width: usize, height: usize, pixels: Vec<Argb>) -> ArgbImage {
        debug_assert_eq!(pixels.len(), width * height);
        ArgbImage {
            width,
            height,
            pixels,
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Argb {
        self.pixels[y * self.width + x]
    }
}

/// Tuning knobs for the scaler (reference defaults).
#[derive(Clone, Copy, Debug)]
pub struct ScalerConfig {
    /// Weight of the luminance component in the YCbCr color distance.
    pub luminance_weight: f64,
    /// Distance below which two colors are considered equal.
    pub equal_color_tolerance: f64,
    /// Ratio above which a corner blend is considered "dominant".
    pub dominant_direction_threshold: f64,
    /// Ratio used to discriminate steep vs. shallow line blending.
    pub steep_direction_threshold: f64,
}

impl Default for ScalerConfig {
    fn default() -> Self {
        ScalerConfig {
            luminance_weight: 1.0,
            equal_color_tolerance: 30.0,
            dominant_direction_threshold: 3.6,
            steep_direction_threshold: 2.2,
        }
    }
}

// ---------------------------------------------------------------------------
// Color distance
// ---------------------------------------------------------------------------

/// Perceptual YCbCr (ITU-R BT.2020) distance between two colors.
///
/// Channel differences are quantized exactly like the reference
/// implementation's precomputed lookup table: each difference `d` is snapped
/// to `((d + 255) / 2) * 2 - 255` (integer division truncates toward zero),
/// and the result is stored as a 32-bit float. Reproducing both steps is what
/// makes this port bit-exact with the reference.
#[inline]
fn dist_ycbcr(p1: Argb, p2: Argb) -> f64 {
    fn quant(d: i32) -> i32 {
        (d + 255) / 2 * 2 - 255
    }

    let r_diff = quant(p1.r() as i32 - p2.r() as i32);
    let g_diff = quant(p1.g() as i32 - p2.g() as i32);
    let b_diff = quant(p1.b() as i32 - p2.b() as i32);

    const K_B: f64 = 0.0593; // ITU-R BT.2020
    const K_R: f64 = 0.2627;
    const K_G: f64 = 1.0 - K_B - K_R;
    const SCALE_B: f64 = 0.5 / (1.0 - K_B);
    const SCALE_R: f64 = 0.5 / (1.0 - K_R);

    // The division by 255 is deliberately skipped to keep the distance scale
    // comparable to plain RGB distances.
    let y = K_R * r_diff as f64 + K_G * g_diff as f64 + K_B * b_diff as f64;
    let c_b = SCALE_B * (b_diff as f64 - y);
    let c_r = SCALE_R * (r_diff as f64 - y);

    (y * y + c_b * c_b + c_r * c_r).sqrt() as f32 as f64
}

/// Alpha-aware color distance: transparent pixels are far away from opaque
/// ones, and fully transparent pixels are all treated as equally distant.
///
/// Note: like the reference implementation's fast path, `luminance_weight`
/// is not applied here (the YCbCr distance is computed with a fixed weight).
///
/// Public so other stages (e.g. color quantization) share the engine's
/// notion of "similar color".
#[inline]
pub fn color_dist(p1: Argb, p2: Argb) -> f64 {
    let d = dist_ycbcr(p1, p2);
    let a1 = p1.a() as f64 / 255.0;
    let a2 = p2.a() as f64 / 255.0;
    if a1 < a2 {
        a1 * d + 255.0 * (a2 - a1)
    } else {
        a2 * d + 255.0 * (a1 - a2)
    }
}

// ---------------------------------------------------------------------------
// Corner preprocessing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BlendType {
    None = 0,
    Normal = 1,
    Dominant = 2,
}

/// 4x4 input kernel. The input pixel is `f`; `F G / J K` are the four pixels
/// meeting at the corner under evaluation (the bottom-right corner of `f`).
/// Only the fields actually consulted by [`preprocess_corners`] are kept.
struct Kernel4 {
    b: Argb,
    c: Argb,
    e: Argb,
    f: Argb,
    g: Argb,
    h: Argb,
    i: Argb,
    j: Argb,
    k: Argb,
    l: Argb,
    n: Argb,
    o: Argb,
}

/// Classify the corner between `F, G, J, K`; returns `[f, g, j, k]` blend
/// decisions. An edge running along the J-G diagonal blends corners F and K;
/// an edge along the F-K diagonal blends J and G.
fn preprocess_corners(k: &Kernel4, cfg: &ScalerConfig) -> [BlendType; 4] {
    let mut result = [BlendType::None; 4];
    if (k.f == k.g && k.j == k.k) || (k.f == k.j && k.g == k.k) {
        return result;
    }

    let weight = 4.0;
    let jg = color_dist(k.i, k.f)
        + color_dist(k.f, k.c)
        + color_dist(k.n, k.k)
        + color_dist(k.k, k.h)
        + weight * color_dist(k.j, k.g);
    let fk = color_dist(k.e, k.j)
        + color_dist(k.j, k.o)
        + color_dist(k.b, k.g)
        + color_dist(k.g, k.l)
        + weight * color_dist(k.f, k.k);

    if jg < fk {
        let dominant = cfg.dominant_direction_threshold * jg < fk;
        if k.f != k.g && k.f != k.j {
            result[0] = if dominant {
                BlendType::Dominant
            } else {
                BlendType::Normal
            };
        }
        if k.k != k.j && k.k != k.g {
            result[3] = if dominant {
                BlendType::Dominant
            } else {
                BlendType::Normal
            };
        }
    } else if fk < jg {
        let dominant = cfg.dominant_direction_threshold * fk < jg;
        if k.j != k.f && k.j != k.k {
            result[2] = if dominant {
                BlendType::Dominant
            } else {
                BlendType::Normal
            };
        }
        if k.g != k.f && k.g != k.k {
            result[1] = if dominant {
                BlendType::Dominant
            } else {
                BlendType::Normal
            };
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Blend weight tables (one per scale factor)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum OpKind {
    /// Blend the line color over the current subpixel with opacity `m / n`.
    Grad { m: u32, n: u32 },
    /// Replace the subpixel with the line color.
    Set,
}

/// A single blend operation applied to subpixel `(i, j)` (corner-local
/// coordinates, bottom-right corner frame).
#[derive(Clone, Copy, Debug)]
struct Op {
    kind: OpKind,
    i: usize,
    j: usize,
}

struct ScaleTable {
    scale: usize,
    line_shallow: &'static [Op],
    line_steep: &'static [Op],
    line_steep_and_shallow: &'static [Op],
    line_diagonal: &'static [Op],
    corner: &'static [Op],
}

const fn grad(m: u32, n: u32, i: usize, j: usize) -> Op {
    Op {
        kind: OpKind::Grad { m, n },
        i,
        j,
    }
}

const fn set(i: usize, j: usize) -> Op {
    Op {
        kind: OpKind::Set,
        i,
        j,
    }
}

static TABLES: [ScaleTable; 5] = [
    // 2x
    ScaleTable {
        scale: 2,
        line_shallow: &[grad(1, 4, 1, 0), grad(3, 4, 1, 1)],
        line_steep: &[grad(1, 4, 0, 1), grad(3, 4, 1, 1)],
        line_steep_and_shallow: &[grad(1, 4, 1, 0), grad(1, 4, 0, 1), grad(5, 6, 1, 1)],
        line_diagonal: &[grad(1, 2, 1, 1)],
        corner: &[grad(21, 100, 1, 1)],
    },
    // 3x
    ScaleTable {
        scale: 3,
        line_shallow: &[
            grad(1, 4, 2, 0),
            grad(1, 4, 1, 2),
            grad(3, 4, 2, 1),
            set(2, 2),
        ],
        line_steep: &[
            grad(1, 4, 0, 2),
            grad(1, 4, 2, 1),
            grad(3, 4, 1, 2),
            set(2, 2),
        ],
        line_steep_and_shallow: &[
            grad(1, 4, 2, 0),
            grad(1, 4, 0, 2),
            grad(3, 4, 2, 1),
            grad(3, 4, 1, 2),
            set(2, 2),
        ],
        line_diagonal: &[grad(1, 8, 1, 2), grad(1, 8, 2, 1), grad(7, 8, 2, 2)],
        corner: &[grad(45, 100, 2, 2)],
    },
    // 4x
    ScaleTable {
        scale: 4,
        line_shallow: &[
            grad(1, 4, 3, 0),
            grad(1, 4, 2, 2),
            grad(3, 4, 3, 1),
            grad(3, 4, 2, 3),
            set(3, 2),
            set(3, 3),
        ],
        line_steep: &[
            grad(1, 4, 0, 3),
            grad(1, 4, 2, 2),
            grad(3, 4, 1, 3),
            grad(3, 4, 3, 2),
            set(2, 3),
            set(3, 3),
        ],
        line_steep_and_shallow: &[
            grad(3, 4, 3, 1),
            grad(3, 4, 1, 3),
            grad(1, 4, 3, 0),
            grad(1, 4, 0, 3),
            grad(1, 3, 2, 2),
            set(3, 3),
            set(3, 2),
            set(2, 3),
        ],
        line_diagonal: &[grad(1, 2, 3, 2), grad(1, 2, 2, 3), set(3, 3)],
        corner: &[grad(68, 100, 3, 3), grad(9, 100, 3, 2), grad(9, 100, 2, 3)],
    },
    // 5x
    ScaleTable {
        scale: 5,
        line_shallow: &[
            grad(1, 4, 4, 0),
            grad(1, 4, 3, 2),
            grad(1, 4, 2, 4),
            grad(3, 4, 4, 1),
            grad(3, 4, 3, 3),
            set(4, 2),
            set(4, 3),
            set(4, 4),
            set(3, 4),
        ],
        line_steep: &[
            grad(1, 4, 0, 4),
            grad(1, 4, 2, 3),
            grad(1, 4, 4, 2),
            grad(3, 4, 1, 4),
            grad(3, 4, 3, 3),
            set(2, 4),
            set(3, 4),
            set(4, 4),
            set(4, 3),
        ],
        line_steep_and_shallow: &[
            grad(1, 4, 0, 4),
            grad(1, 4, 2, 3),
            grad(3, 4, 1, 4),
            grad(1, 4, 4, 0),
            grad(1, 4, 3, 2),
            grad(3, 4, 4, 1),
            grad(2, 3, 3, 3),
            set(2, 4),
            set(3, 4),
            set(4, 4),
            set(4, 2),
            set(4, 3),
        ],
        line_diagonal: &[
            grad(1, 8, 4, 2),
            grad(1, 8, 3, 3),
            grad(1, 8, 2, 4),
            grad(7, 8, 4, 3),
            grad(7, 8, 3, 4),
            set(4, 4),
        ],
        corner: &[
            grad(86, 100, 4, 4),
            grad(23, 100, 4, 3),
            grad(23, 100, 3, 4),
        ],
    },
    // 6x
    ScaleTable {
        scale: 6,
        line_shallow: &[
            grad(1, 4, 5, 0),
            grad(1, 4, 4, 2),
            grad(1, 4, 3, 4),
            grad(3, 4, 5, 1),
            grad(3, 4, 4, 3),
            grad(3, 4, 3, 5),
            set(5, 2),
            set(5, 3),
            set(5, 4),
            set(5, 5),
            set(4, 4),
            set(4, 5),
        ],
        line_steep: &[
            grad(1, 4, 0, 5),
            grad(1, 4, 2, 4),
            grad(1, 4, 4, 3),
            grad(3, 4, 1, 5),
            grad(3, 4, 3, 4),
            grad(3, 4, 5, 3),
            set(2, 5),
            set(3, 5),
            set(4, 5),
            set(5, 5),
            set(4, 4),
            set(5, 4),
        ],
        line_steep_and_shallow: &[
            grad(1, 4, 0, 5),
            grad(1, 4, 2, 4),
            grad(3, 4, 1, 5),
            grad(3, 4, 3, 4),
            grad(1, 4, 5, 0),
            grad(1, 4, 4, 2),
            grad(3, 4, 5, 1),
            grad(3, 4, 4, 3),
            set(2, 5),
            set(3, 5),
            set(4, 5),
            set(5, 5),
            set(4, 4),
            set(5, 4),
            set(5, 2),
            set(5, 3),
        ],
        line_diagonal: &[
            grad(1, 2, 5, 3),
            grad(1, 2, 4, 4),
            grad(1, 2, 3, 5),
            set(4, 5),
            set(5, 5),
            set(5, 4),
        ],
        corner: &[
            grad(97, 100, 5, 5),
            grad(42, 100, 4, 5),
            grad(42, 100, 5, 4),
            grad(6, 100, 5, 3),
            grad(6, 100, 3, 5),
        ],
    },
];

// ---------------------------------------------------------------------------
// Per-pixel blending
// ---------------------------------------------------------------------------

/// 3x3 kernel; the input pixel is `e`.
#[derive(Clone, Copy)]
struct Kernel3 {
    a: Argb,
    b: Argb,
    c: Argb,
    d: Argb,
    e: Argb,
    f: Argb,
    g: Argb,
    h: Argb,
    i: Argb,
}

/// Rotate a 3x3 kernel 90 degrees clockwise.
fn rotate_kernel90(k: &Kernel3) -> Kernel3 {
    Kernel3 {
        a: k.g,
        b: k.d,
        c: k.a,
        d: k.h,
        e: k.e,
        f: k.b,
        g: k.i,
        h: k.f,
        i: k.c,
    }
}

/// Rotate the per-pixel blend-info byte so that the "bottom-right" corner
/// bits describe the corner under evaluation in the rotated frame.
fn rotate_blend_info(r: usize, b: u8) -> u8 {
    match r {
        0 => b,
        1 => b.rotate_left(2),
        2 => b.rotate_left(4),
        _ => b.rotate_left(6),
    }
}

/// Map a corner-local subpixel position to its position in the output block
/// for the given rotation.
fn rotate_pos(r: usize, scale: usize, i: usize, j: usize) -> (usize, usize) {
    match r {
        0 => (i, j),
        1 => (scale - 1 - j, i),
        2 => (scale - 1 - i, scale - 1 - j),
        _ => (j, scale - 1 - i),
    }
}

/// Intermediate color between two colors with alpha channels (NOT alpha
/// compositing): both the RGB channels and the alpha channel are interpolated.
#[inline]
fn gradient_argb(m: u32, n: u32, front: Argb, back: Argb) -> Argb {
    let weight_front = front.a() * m;
    let weight_back = back.a() * (n - m);
    let weight_sum = weight_front + weight_back;
    if weight_sum == 0 {
        return Argb(0);
    }
    let calc = |fc: u32, bc: u32| ((fc * weight_front + bc * weight_back) / weight_sum) as u8;
    Argb(
        ((weight_sum / n) as u8 as u32) << 24
            | ((calc(front.r(), back.r()) as u32) << 16)
            | ((calc(front.g(), back.g()) as u32) << 8)
            | calc(front.b(), back.b()) as u32,
    )
}

/// Blend the four corners of the input pixel into its `scale x scale` output
/// block. `dst` is the full output image; `block_start` points at the
/// top-left pixel of the block, `dst_width` is the output row stride.
#[allow(clippy::too_many_arguments)]
fn blend_pixel(
    dst: &mut [Argb],
    dst_width: usize,
    block_start: usize,
    scale: usize,
    table: &ScaleTable,
    k3: &Kernel3,
    blend_info: u8,
    cfg: &ScalerConfig,
) {
    let rotated = [
        *k3,
        rotate_kernel90(k3),
        rotate_kernel90(&rotate_kernel90(k3)),
        rotate_kernel90(&rotate_kernel90(&rotate_kernel90(k3))),
    ];

    for (r, ker) in rotated.iter().enumerate() {
        let blend = rotate_blend_info(r, blend_info);
        let corner = (blend >> 4) & 3; // corner under evaluation
        if corner >= BlendType::Normal as u8 {
            let e = ker.e;
            let f = ker.f;
            let g = ker.g;
            let h = ker.h;
            let i = ker.i;
            let c = ker.c;
            let d = ker.d;
            let b = ker.b;

            let eq = |p1: Argb, p2: Argb| color_dist(p1, p2) < cfg.equal_color_tolerance;

            let do_line_blend = if corner >= BlendType::Dominant as u8 {
                true
            } else {
                let top_r = (blend >> 2) & 3;
                let bottom_l = (blend >> 6) & 3;
                // Insular pixels (no double blending in an adjacent corner)…
                let insular = (top_r != BlendType::None as u8 && !eq(e, g))
                    || (bottom_l != BlendType::None as u8 && !eq(e, c));
                // …and L-shapes get a corner blend instead of a full line.
                let l_shape = !eq(e, i) && eq(g, h) && eq(h, i) && eq(i, f) && eq(f, c);
                !insular && !l_shape
            };

            // Choose the most similar of the two orthogonal neighbors.
            let px = if color_dist(e, f) <= color_dist(e, h) {
                f
            } else {
                h
            };

            let ops: &[Op] = if do_line_blend {
                let fg = color_dist(f, g);
                let hc = color_dist(h, c);
                let have_shallow = cfg.steep_direction_threshold * fg <= hc && e != g && d != g;
                let have_steep = cfg.steep_direction_threshold * hc <= fg && e != c && b != c;
                match (have_shallow, have_steep) {
                    (true, true) => table.line_steep_and_shallow,
                    (true, false) => table.line_shallow,
                    (false, true) => table.line_steep,
                    (false, false) => table.line_diagonal,
                }
            } else {
                table.corner
            };

            for op in ops {
                let (row, col) = rotate_pos(r, scale, op.i, op.j);
                let slot = &mut dst[block_start + row * dst_width + col];
                match op.kind {
                    OpKind::Set => *slot = px,
                    OpKind::Grad { m, n } => *slot = gradient_argb(m, n, px, *slot),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scale driver
// ---------------------------------------------------------------------------

/// Build the 4x4 kernel centered on `(x, y)` with clamped border reads.
fn build_kernel4(src: &[Argb], w: usize, h: usize, x: usize, y: usize) -> Kernel4 {
    let y_m1 = y.saturating_sub(1);
    let y_p1 = (y + 1).min(h - 1);
    let y_p2 = (y + 2).min(h - 1);
    let x_m1 = x.saturating_sub(1);
    let x_p1 = (x + 1).min(w - 1);
    let x_p2 = (x + 2).min(w - 1);

    let at = |cx: usize, cy: usize| src[cy * w + cx];
    Kernel4 {
        b: at(x, y_m1),
        c: at(x_p1, y_m1),
        e: at(x_m1, y),
        f: at(x, y),
        g: at(x_p1, y),
        h: at(x_p2, y),
        i: at(x_m1, y_p1),
        j: at(x, y_p1),
        k: at(x_p1, y_p1),
        l: at(x_p2, y_p1),
        n: at(x, y_p2),
        o: at(x_p1, y_p2),
    }
}

/// Build the 3x3 kernel centered on `(x, y)` with clamped border reads.
fn build_kernel3(src: &[Argb], w: usize, h: usize, x: usize, y: usize) -> Kernel3 {
    let y_m1 = y.saturating_sub(1);
    let y_p1 = (y + 1).min(h - 1);
    let x_m1 = x.saturating_sub(1);
    let x_p1 = (x + 1).min(w - 1);

    let at = |cx: usize, cy: usize| src[cy * w + cx];
    Kernel3 {
        a: at(x_m1, y_m1),
        b: at(x, y_m1),
        c: at(x_p1, y_m1),
        d: at(x_m1, y),
        e: at(x, y),
        f: at(x_p1, y),
        g: at(x_m1, y_p1),
        h: at(x, y_p1),
        i: at(x_p1, y_p1),
    }
}

/// Pack the four corner decisions of a kernel into a byte:
/// bits 0-1 = f, 2-3 = g, 4-5 = j, 6-7 = k.
fn pack_kernel_result(r: [BlendType; 4]) -> u8 {
    (r[0] as u8) | ((r[1] as u8) << 2) | ((r[2] as u8) << 4) | ((r[3] as u8) << 6)
}

/// Assemble the four corner blend decisions of pixel `(x, y)` from the four
/// kernels that contain it. Corners along the image border are never blended.
fn assemble_blend_info(x: usize, y: usize, w: usize, kernels: &[u8]) -> u8 {
    let mut info = 0u8;
    if y > 0 {
        if x > 0 {
            // top-left corner: k of kernel (x-1, y-1)
            info |= (kernels[(y - 1) * w + (x - 1)] >> 6) & 3;
        }
        // top-right corner: j of kernel (x, y-1)
        info |= ((kernels[(y - 1) * w + x] >> 4) & 3) << 2;
    }
    // bottom-right corner: f of kernel (x, y)
    info |= (kernels[y * w + x] & 3) << 4;
    if x > 0 {
        // bottom-left corner: g of kernel (x-1, y). A kernel centered at
        // (cx, cy) evaluates the vertex (cx+1, cy+1); its `g` result is the
        // bottom-left corner of pixel (cx+1, cy).
        info |= ((kernels[y * w + x - 1] >> 2) & 3) << 6;
    }
    info
}

/// Upscale `src` by `factor` (2..=6) using the xBRZ algorithm.
pub fn scale_image(src: &ArgbImage, factor: u8, cfg: &ScalerConfig) -> ArgbImage {
    assert!(
        (2..=6).contains(&factor),
        "xBRZ supports scale factors 2 through 6, got {factor}"
    );
    let scale = factor as usize;
    let src_width = src.width;
    let src_height = src.height;
    let dst_width = src_width * scale;
    let dst_height = src_height * scale;
    let mut dst = vec![Argb(0); dst_width * dst_height];
    let table = &TABLES[(factor - 2) as usize];
    debug_assert_eq!(table.scale, scale);

    // Step 1: preprocess every 4x4 kernel once and record the corner blend
    // decisions for its center pixel.
    let mut kernels = vec![0u8; src_width * src_height];
    for y in 0..src_height {
        for x in 0..src_width {
            let k4 = build_kernel4(&src.pixels, src_width, src_height, x, y);
            kernels[y * src_width + x] = pack_kernel_result(preprocess_corners(&k4, cfg));
        }
    }

    // Step 2: for each input pixel, fill its scale x scale block and blend
    // any corners that need it.
    for y in 0..src_height {
        for x in 0..src_width {
            let px = src.pixels[y * src_width + x];
            let block_start = y * scale * dst_width + x * scale;
            for sy in 0..scale {
                // The block is `scale` columns wide; its rows are separated
                // by the full output row stride.
                let start = block_start + sy * dst_width;
                dst[start..start + scale].fill(px);
            }

            let info = assemble_blend_info(x, y, src_width, &kernels);
            if info != 0 {
                let k3 = build_kernel3(&src.pixels, src_width, src_height, x, y);
                blend_pixel(
                    &mut dst,
                    dst_width,
                    block_start,
                    scale,
                    table,
                    &k3,
                    info,
                    cfg,
                );
            }
        }
    }

    ArgbImage::new(dst_width, dst_height, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_block_stays_solid() {
        let src = ArgbImage::new(4, 4, vec![Argb::from_rgba(10, 20, 30, 255); 16]);
        for factor in 2..=6 {
            let out = scale_image(&src, factor, &ScalerConfig::default());
            assert_eq!(out.width, 4 * factor as usize);
            assert_eq!(out.height, 4 * factor as usize);
            assert!(out
                .pixels
                .iter()
                .all(|&p| p == Argb::from_rgba(10, 20, 30, 255)));
        }
    }

    #[test]
    fn horizontal_edge_stays_horizontal() {
        // Top half red, bottom half green: the scaled image must be exactly
        // the same split, with no bleed across the boundary.
        let w = 6;
        let h = 6;
        let red = Argb::from_rgba(255, 0, 0, 255);
        let green = Argb::from_rgba(0, 255, 0, 255);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for _ in 0..w {
                pixels.push(if y < 3 { red } else { green });
            }
        }
        let src = ArgbImage::new(w, h, pixels);
        for factor in 2..=6 {
            let out = scale_image(&src, factor, &ScalerConfig::default());
            let s = factor as usize;
            for y in 0..out.height {
                for x in 0..out.width {
                    let expected = if y < 3 * s { red } else { green };
                    assert_eq!(
                        out.pixels[y * out.width + x],
                        expected,
                        "factor {factor} at ({x},{y})"
                    );
                }
            }
        }
    }

    #[test]
    fn output_dimensions_and_factor_validation() {
        let src = ArgbImage::new(2, 3, vec![Argb(0); 6]);
        let out = scale_image(&src, 5, &ScalerConfig::default());
        assert_eq!((out.width, out.height), (10, 15));

        let result = std::panic::catch_unwind(|| scale_image(&src, 7, &ScalerConfig::default()));
        assert!(result.is_err());
    }
}
