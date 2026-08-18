//! Vector tracing: convert the (xBRZ-upscaled) pixel grid into closed SVG
//! path loops.
//!
//! Instead of emitting one `<rect>` per pixel, the boundary of every
//! contiguous region of identical color is traced as a polygon of grid
//! vertices. Because all boundaries are shared grid edges, adjacent shapes
//! share their coordinates exactly — combined with integer coordinates and
//! `shape-rendering="crispEdges"` on the root element, this leaves no visible
//! seams or gaps between neighboring color shapes.
//!
//! The tracer walks each boundary with the region interior on its left and
//! always takes the sharpest left turn at a vertex (straight, then right),
//! which traces every loop exactly once. Holes of other colors come out as
//! separate loops, so `fill-rule="evenodd"` renders them correctly.

use crate::xbrz_engine::{Argb, ArgbImage};

/// A closed loop of grid-vertex coordinates (integer, in pixel units).
/// Consecutive points are never collinear; the closing segment back to the
/// first point is implied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathLoop {
    pub points: Vec<(i32, i32)>,
}

/// A renderable region: a fill color plus one or more loops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub color: Argb,
    pub loops: Vec<PathLoop>,
}

// Direction encoding (clockwise compass order): E=0, S=1, W=2, N=3.
const E: u8 = 0;
const S: u8 = 1;
const W: u8 = 2;
const N: u8 = 3;

// Edge slots in the per-pixel visited bitmap.
const EDGE_TOP: u8 = 0; // traversed W (interior below)
const EDGE_RIGHT: u8 = 1; // traversed N (interior west)
const EDGE_BOTTOM: u8 = 2; // traversed E (interior north)
const EDGE_LEFT: u8 = 3; // traversed S (interior east)

/// The directed edge of `(x, y)`'s pixel with the pixel interior on the left
/// starts at this vertex.
fn start_vertex(x: i32, y: i32, edge: u8) -> (i32, i32) {
    match edge {
        EDGE_TOP => (x + 1, y),
        EDGE_RIGHT => (x + 1, y + 1),
        EDGE_BOTTOM => (x, y + 1),
        _ => (x, y),
    }
}

fn edge_direction(edge: u8) -> u8 {
    match edge {
        EDGE_TOP => W,
        EDGE_RIGHT => N,
        EDGE_BOTTOM => E,
        _ => S,
    }
}

/// The pixel that owns the directed edge starting at vertex `(vx, vy)` in
/// direction `d` (the pixel on the left of travel), and which of its edges
/// it is.
fn owner(vx: i32, vy: i32, d: u8) -> (i32, i32, u8) {
    match d {
        E => (vx, vy - 1, EDGE_BOTTOM),
        S => (vx, vy, EDGE_LEFT),
        W => (vx - 1, vy, EDGE_TOP),
        _ => (vx - 1, vy - 1, EDGE_RIGHT),
    }
}

/// The pixel adjacent on the right of travel of the directed edge starting
/// at vertex `(vx, vy)` in direction `d`.
fn right_neighbor(vx: i32, vy: i32, d: u8) -> (i32, i32) {
    match d {
        E => (vx, vy),
        S => (vx - 1, vy),
        W => (vx - 1, vy - 1),
        _ => (vx, vy - 1),
    }
}

/// Is the directed edge starting at `(vx, vy)` in direction `d` a boundary
/// edge of the region of `color`? Requires the owner pixel (interior, on the
/// left) to have `color` and the right neighbor to differ (or be out of
/// bounds).
fn edge_is_boundary(img: &ArgbImage, color: Argb, vx: i32, vy: i32, d: u8) -> bool {
    let (ox, oy, _) = owner(vx, vy, d);
    let (rw, rh) = (img.width as i32, img.height as i32);
    if ox < 0 || oy < 0 || ox >= rw || oy >= rh {
        return false;
    }
    if img.get(ox as usize, oy as usize) != color {
        return false;
    }
    let (rx, ry) = right_neighbor(vx, vy, d);
    if rx < 0 || ry < 0 || rx >= rw || ry >= rh {
        return true;
    }
    img.get(rx as usize, ry as usize) != color
}

fn is_visited(visited: &[u8], vx: i32, vy: i32, d: u8, w: i32, h: i32) -> bool {
    let (ox, oy, oedge) = owner(vx, vy, d);
    if ox < 0 || oy < 0 || ox >= w || oy >= h {
        return true; // cannot traverse an edge whose owner is out of bounds
    }
    visited[oy as usize * w as usize + ox as usize] & (1 << oedge) != 0
}

fn mark_visited(visited: &mut [u8], vx: i32, vy: i32, d: u8, w: i32) {
    let (ox, oy, oedge) = owner(vx, vy, d);
    visited[oy as usize * w as usize + ox as usize] |= 1 << oedge;
}

/// Trace one closed boundary loop of the color of the pixel at `(px, py)`,
/// starting from that pixel's `start_edge`. The walk keeps the region on its
/// left and prefers the sharpest left turn at every vertex.
fn trace_loop(
    img: &ArgbImage,
    visited: &mut [u8],
    px: i32,
    py: i32,
    start_edge: u8,
) -> Vec<(i32, i32)> {
    let w = img.width as i32;
    let h = img.height as i32;
    let color = img.get(px as usize, py as usize);

    let (sx, sy) = start_vertex(px, py, start_edge);
    let start_dir = edge_direction(start_edge);
    let mut points = vec![(sx, sy)];
    let (mut x, mut y) = (sx, sy);
    let mut d = start_dir;

    // Defensive bound: a loop cannot traverse more edges than exist.
    let max_edges = 4 * img.width * img.height + 16;
    for _ in 0..max_edges {
        mark_visited(visited, x, y, d, w);
        let (nx, ny) = match d {
            E => (x + 1, y),
            S => (x, y + 1),
            W => (x - 1, y),
            _ => (x, y - 1),
        };

        let next = [(d + 3) % 4, d, (d + 1) % 4].iter().copied().find(|&c| {
            edge_is_boundary(img, color, nx, ny, c) && !is_visited(visited, nx, ny, c, w, h)
        });

        let Some(nd) = next else {
            break; // unreachable for a well-formed boundary; be defensive
        };

        if (nx, ny) == (sx, sy) && nd == start_dir {
            points.push((nx, ny));
            break;
        }
        points.push((nx, ny));
        x = nx;
        y = ny;
        d = nd;
    }
    points
}

/// Drop points that lie on a straight line between their neighbors, keeping
/// only the corners of the loop.
fn simplify(points: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let n = points.len();
    if n <= 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = points[(i + n - 1) % n];
        let cur = points[i];
        let next = points[(i + 1) % n];
        let d1 = (cur.0 - prev.0, cur.1 - prev.1);
        let d2 = (next.0 - cur.0, next.1 - cur.1);
        let collinear = (d1.0 != 0 || d1.1 != 0) && d1 == d2;
        if !collinear {
            out.push(cur);
        }
    }
    out
}

/// Convert the pixel grid into regions. With `merge_colors` set, all loops of
/// the same color are grouped into a single region (one `<path>` per color);
/// otherwise every connected region becomes its own region.
///
/// Fully transparent colors are skipped: they have nothing to render.
pub fn vectorize(img: &ArgbImage, merge_colors: bool) -> Vec<Region> {
    let w = img.width as i32;
    let mut visited = vec![0u8; img.width * img.height];

    // Collect loops grouped by color.
    let mut by_color: Vec<(Argb, Vec<PathLoop>)> = Vec::new();
    for y in 0..img.height {
        for x in 0..img.width {
            let color = img.get(x, y);
            if color.a() == 0 {
                continue; // nothing to render
            }
            let base = (y as i32) * w + (x as i32);
            for edge in [EDGE_TOP, EDGE_RIGHT, EDGE_BOTTOM, EDGE_LEFT] {
                if visited[base as usize] & (1 << edge) != 0 {
                    continue;
                }
                let (sx, sy) = start_vertex(x as i32, y as i32, edge);
                if !edge_is_boundary(img, color, sx, sy, edge_direction(edge)) {
                    continue;
                }
                let raw = trace_loop(img, &mut visited, x as i32, y as i32, edge);
                let points = simplify(&raw);
                if points.len() < 3 {
                    continue; // degenerate; skip defensively
                }
                match by_color.iter_mut().find(|(c, _)| *c == color) {
                    Some((_, loops)) => loops.push(PathLoop { points }),
                    None => by_color.push((color, vec![PathLoop { points }])),
                }
            }
        }
    }

    let mut regions: Vec<Region> = Vec::new();
    for (color, loops) in by_color {
        if merge_colors {
            regions.push(Region { color, loops });
        } else {
            for l in loops {
                regions.push(Region {
                    color,
                    loops: vec![l],
                });
            }
        }
    }

    // Deterministic output order.
    regions.sort_by_key(|r| r.color.0);
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_from_rows(rows: &[&str], palette: &[(char, Argb)]) -> ArgbImage {
        let h = rows.len();
        let w = rows[0].len();
        let mut pixels = Vec::with_capacity(w * h);
        for row in rows {
            assert_eq!(row.len(), w, "ragged test row");
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

    /// Signed shoelace area of a loop. Outer boundaries (traced with the
    /// interior on the left, counter-clockwise in y-down coordinates) are
    /// negative and hole boundaries are positive, so the absolute value of
    /// the sum over all loops of a region equals the region's filled area.
    fn area(loop_points: &[(i32, i32)]) -> i64 {
        let mut sum = 0i64;
        for i in 0..loop_points.len() {
            let (x1, y1) = loop_points[i];
            let (x2, y2) = loop_points[(i + 1) % loop_points.len()];
            sum += x1 as i64 * y2 as i64 - x2 as i64 * y1 as i64;
        }
        sum
    }

    #[test]
    fn single_pixel_region_is_a_unit_square() {
        let img = grid_from_rows(&["A.", ".."], &[('A', Argb::from_rgba(255, 0, 0, 255))]);
        let regions = vectorize(&img, true);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].loops.len(), 1);
        let pts = &regions[0].loops[0].points;
        assert_eq!(area(pts).abs(), 2); // 1x1 square
    }

    #[test]
    fn regions_are_merged_by_color_and_cover_exactly() {
        // Two separate A blocks of the same color must merge into one region
        // with two loops, and the total traced area must equal the pixel area.
        let img = grid_from_rows(
            &[
                "AA..", //
                "AA..", //
                "..AA", //
                "..AA", //
            ],
            &[('A', Argb::from_rgba(10, 20, 30, 255))],
        );
        let regions = vectorize(&img, true);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].loops.len(), 2);

        let total: i64 = regions[0].loops.iter().map(|l| area(&l.points)).sum();
        assert_eq!(total.abs(), 16); // 8 pixels * 2 (shoelace units)

        let unmerged = vectorize(&img, false);
        assert_eq!(unmerged.len(), 2);
    }

    #[test]
    fn hole_is_traced_as_separate_loop() {
        // A ring of A with a B hole: A must produce two loops (outer + hole).
        let img = grid_from_rows(
            &[
                "AAAA", //
                "ABBA", //
                "ABBA", //
                "AAAA", //
            ],
            &[
                ('A', Argb::from_rgba(255, 0, 0, 255)),
                ('B', Argb::from_rgba(0, 0, 255, 255)),
            ],
        );
        let regions = vectorize(&img, true);
        let a = regions
            .iter()
            .find(|r| r.color == Argb::from_rgba(255, 0, 0, 255))
            .unwrap();
        assert_eq!(a.loops.len(), 2);
        let a_area: i64 = a.loops.iter().map(|l| area(&l.points)).sum();
        assert_eq!(a_area.abs(), 24); // 12 A pixels * 2 (outer minus hole)
    }

    #[test]
    fn transparent_pixels_are_skipped() {
        let img = grid_from_rows(
            &[
                "A.", //
                "..", //
            ],
            &[('A', Argb::from_rgba(0, 0, 0, 0))],
        );
        let regions = vectorize(&img, true);
        assert!(regions.is_empty());
    }

    #[test]
    fn full_image_is_a_single_rectangle() {
        let img = grid_from_rows(
            &[
                "AAAA", //
                "AAAA", //
                "AAAA", //
            ],
            &[('A', Argb::from_rgba(1, 2, 3, 255))],
        );
        let regions = vectorize(&img, true);
        assert_eq!(regions.len(), 1);
        let pts = &regions[0].loops[0].points;
        // A 4x3 rectangle has 4 corner points.
        assert_eq!(pts.len(), 4);
        assert_eq!(area(pts).abs(), 24);
    }

    #[test]
    fn checkerboard_produces_correct_loop_count() {
        let img = grid_from_rows(
            &[
                "ABAB", //
                "BABA", //
                "ABAB", //
                "BABA", //
            ],
            &[
                ('A', Argb::from_rgba(255, 0, 0, 255)),
                ('B', Argb::from_rgba(0, 255, 0, 255)),
            ],
        );
        let regions = vectorize(&img, true);
        assert_eq!(regions.len(), 2);
        for r in &regions {
            assert_eq!(r.loops.len(), 8); // 8 isolated pixels per color
        }
    }
}
