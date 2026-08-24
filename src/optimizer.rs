//! SVGO-style path optimization applied to traced regions before
//! serialization.
//!
//! The vectorizer already merges collinear runs while tracing, so on its
//! current output this pass mostly *guarantees* invariants rather than
//! finding new savings. It is defense in depth: if the tracing strategy ever
//! changes — or a future pass produces loops some other way — the emitted
//! path data stays provably minimal.
//!
//! The passes mirror SVGO plugins:
//!
//! - *cleanupPathData*: [`strip_redundant_points`] removes zero-length
//!   segments, duplicate consecutive points, and a trailing point that
//!   repeats the first point (whose closing `Z` command already implies the
//!   return to the start).
//! - *convertPathData / flatten*: the same pass then drops every point that
//!   lies on the straight line between its neighbors — including across the
//!   implied closing segment — so each loop is flattened to a minimal simple
//!   polygon with no curves and no wasted vertices.
//! - *removeUnknownsAndDefaults* (in [`crate::svg_exporter`]): the default
//!   `fill-rule="evenodd"` is dropped for single-loop paths, where the
//!   default `nonzero` rule fills identically.
//!
//! All passes are exact: they never move a vertex, so the rasterized output
//! is bit-for-bit identical to the unoptimized one (asserted by property
//! tests in `tests/property.rs`).

use crate::vectorizer::Region;

/// Aggregate statistics for one [`optimize`] run, reported by `--verbose`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptimizeStats {
    /// Number of loops processed.
    pub loops: usize,
    /// Total control points across all loops before optimization.
    pub points_before: usize,
    /// Total control points across all loops after optimization.
    pub points_after: usize,
}

/// Run the optimization passes over every loop of every region.
///
/// Returns the number of control points stripped, for verbose reporting.
pub fn optimize(regions: &mut [Region]) -> OptimizeStats {
    let mut stats = OptimizeStats::default();
    for region in regions {
        for loop_ in &mut region.loops {
            stats.loops += 1;
            stats.points_before += loop_.points.len();
            strip_redundant_points(&mut loop_.points);
            stats.points_after += loop_.points.len();
        }
    }
    stats
}

/// Strip redundant control points from one closed loop, flattening it into a
/// minimal simple polygon:
///
/// 1. A trailing point equal to the first point is dropped — `Z` already
///    closes the loop, so the explicit duplicate is a zero-length segment.
/// 2. Duplicate consecutive points (zero-length segments) are dropped.
/// 3. A point that lies on the straight line between its two neighbors is
///    dropped, where the loop's neighbors wrap around the implied closing
///    segment. This iterates to a fixpoint, because removing one point can
///    make its former neighbors collinear.
///
/// The loop is never shrunk below three points (a degenerate loop is left
/// untouched rather than destroyed).
pub fn strip_redundant_points(points: &mut Vec<(i32, i32)>) {
    if points.len() <= 3 {
        return;
    }

    // A trailing point duplicating the start is a zero-length closing
    // segment; `Z` already returns to the first point.
    while points.len() > 3 && points.last() == Some(&points[0]) {
        points.pop();
    }

    // Collapse each run of consecutive duplicates to a single point (keeping
    // the first of the run, considering the loop cyclically), so no
    // zero-length segment remains. A duplicated corner is a real vertex —
    // both copies must not be dropped.
    let mut dedup: Vec<(i32, i32)> = Vec::with_capacity(points.len());
    for (i, &p) in points.iter().enumerate() {
        let prev = points[(i + points.len() - 1) % points.len()];
        if p != prev {
            dedup.push(p);
        }
    }
    if dedup.len() < 3 {
        return; // fully degenerate; leave the input untouched
    }
    *points = dedup;
    if points.len() <= 3 {
        return;
    }

    // Flatten: drop points collinear with both neighbors, iterating to a
    // fixpoint (removing one point can make its neighbors collinear). The
    // loop is closed, so each point's neighbors wrap around the implied
    // closing segment.
    loop {
        let n = points.len();
        if n <= 3 {
            break;
        }
        let mut out = Vec::with_capacity(n);
        let mut changed = false;
        for i in 0..n {
            let prev = points[(i + n - 1) % n];
            let cur = points[i];
            let next = points[(i + 1) % n];
            let d1 = (cur.0 - prev.0, cur.1 - prev.1);
            let d2 = (next.0 - cur.0, next.1 - cur.1);
            // After dedup adjacent points are distinct, so only collinearity
            // makes a point redundant (the zero checks are kept as defense
            // for callers that bypass dedup).
            let zero = (d1.0 == 0 && d1.1 == 0) || (d2.0 == 0 && d2.1 == 0);
            if zero || cross(d1, d2) == 0 {
                changed = true;
            } else {
                out.push(cur);
            }
        }
        if !changed || out.len() < 3 {
            break;
        }
        *points = out;
    }
}

/// 2D cross product (z component), in i64 to avoid overflow on large grids.
fn cross(a: (i32, i32), b: (i32, i32)) -> i64 {
    a.0 as i64 * b.1 as i64 - a.1 as i64 * b.0 as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectorizer::vectorize;
    use crate::xbrz_engine::{Argb, ArgbImage};

    fn loop_points(points: &[(i32, i32)]) -> Vec<(i32, i32)> {
        points.to_vec()
    }

    #[test]
    fn trailing_point_duplicating_start_is_removed() {
        // An explicit final vertex equal to the start: `Z` already closes.
        let mut pts = loop_points(&[(0, 0), (4, 0), (4, 4), (0, 4), (0, 0)]);
        strip_redundant_points(&mut pts);
        assert_eq!(pts, vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    }

    #[test]
    fn consecutive_duplicates_are_removed() {
        let mut pts = loop_points(&[(0, 0), (4, 0), (4, 0), (4, 4), (0, 4)]);
        strip_redundant_points(&mut pts);
        assert_eq!(pts, vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    }

    #[test]
    fn collinear_points_are_flattened() {
        // A square traced through every unit step: only the four corners
        // may survive.
        let mut pts = loop_points(&[
            (0, 0),
            (1, 0),
            (2, 0),
            (2, 1),
            (2, 2),
            (1, 2),
            (0, 2),
            (0, 1),
        ]);
        strip_redundant_points(&mut pts);
        assert_eq!(pts, vec![(0, 0), (2, 0), (2, 2), (0, 2)]);
    }

    #[test]
    fn collinear_across_implied_closing_segment() {
        // The last point lies on the line between the previous point and the
        // first point; the implied closing segment makes it redundant.
        let mut pts = loop_points(&[(0, 0), (4, 0), (4, 4), (0, 4), (0, 2)]);
        strip_redundant_points(&mut pts);
        assert_eq!(pts, vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    }

    #[test]
    fn different_length_collinear_steps_are_flattened() {
        // General collinearity (cross product), not just equal vectors: the
        // point after a long run is still redundant.
        let mut pts = loop_points(&[(0, 0), (5, 0), (5, 3), (0, 3), (0, 2)]);
        strip_redundant_points(&mut pts);
        assert_eq!(pts, vec![(0, 0), (5, 0), (5, 3), (0, 3)]);
    }

    #[test]
    fn genuine_corners_are_kept() {
        let mut pts = loop_points(&[(0, 0), (4, 0), (4, 4), (2, 4), (2, 2), (0, 2)]);
        strip_redundant_points(&mut pts);
        // (2, 4) is a real corner (incoming E, outgoing N); nothing may move.
        assert_eq!(pts, vec![(0, 0), (4, 0), (4, 4), (2, 4), (2, 2), (0, 2)]);
    }

    #[test]
    fn degenerate_loops_are_left_untouched() {
        for n in 0..=3 {
            let pts: Vec<(i32, i32)> = (0..n).map(|i| (i, 0)).collect();
            let mut copy = pts.clone();
            strip_redundant_points(&mut copy);
            assert_eq!(copy, pts, "loop of {n} points must be unchanged");
        }
    }

    #[test]
    fn strip_is_idempotent() {
        let pts = vec![
            (0, 0),
            (1, 0),
            (2, 0),
            (2, 1),
            (2, 2),
            (1, 2),
            (0, 2),
            (0, 1),
            (0, 0),
        ];
        let mut once = pts.clone();
        strip_redundant_points(&mut once);
        let mut twice = once.clone();
        strip_redundant_points(&mut twice);
        assert_eq!(twice, once);
    }

    #[test]
    fn vectorizer_output_is_already_minimal() {
        // A single isolated pixel: the tracer emits a square, the optimizer
        // must not change it.
        let img = ArgbImage::new(1, 1, vec![Argb::from_rgba(255, 0, 0, 255)]);
        let mut regions = vectorize(&img, true);
        let stats = optimize(&mut regions);
        assert_eq!(stats.points_before, 4);
        assert_eq!(stats.points_after, 4);
        assert_eq!(regions[0].loops[0].points.len(), 4);
    }

    #[test]
    fn checkerboard_loops_are_untouched_by_optimize() {
        // 4x4 checkerboard: 8 loops per color, each already a unit square.
        let mut pixels = vec![Argb(0); 16];
        for y in 0..4 {
            for x in 0..4 {
                if (x + y) % 2 == 0 {
                    pixels[y * 4 + x] = Argb::from_rgba(255, 0, 0, 255);
                } else {
                    pixels[y * 4 + x] = Argb::from_rgba(0, 255, 0, 255);
                }
            }
        }
        let img = ArgbImage::new(4, 4, pixels);
        let mut regions = vectorize(&img, true);
        let stats = optimize(&mut regions);
        assert_eq!(stats.loops, 16);
        assert_eq!(stats.points_before, stats.points_after);
        for r in &regions {
            for l in &r.loops {
                assert_eq!(l.points.len(), 4);
            }
        }
    }

    #[test]
    fn optimize_aggregates_stats() {
        let mut pixels = vec![Argb(0); 8 * 8];
        let red = Argb::from_rgba(255, 0, 0, 255);
        for y in 1..4 {
            for x in 1..4 {
                pixels[y * 8 + x] = red;
            }
        }
        let img = ArgbImage::new(8, 8, pixels);
        let mut regions = vectorize(&img, true);
        let stats = optimize(&mut regions);
        assert_eq!(stats.loops, 1);
        assert_eq!(stats.points_before, 4);
        assert_eq!(stats.points_after, 4);
    }
}
