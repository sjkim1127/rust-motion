//! Stroke walking: dab spacing, path sampling and far-off segments.

use super::*;

/// The segment walk before #937: every dab, whether or not it can reach the field.
fn full_walk(f: &mut LiquifyField, s: &LiquifyStroke, a: [f64; 3], b: [f64; 3]) -> Rect {
    f.walk_segment(s, a, b, false)
}

#[test]
fn a_segment_walks_only_the_dabs_that_can_reach_the_field() {
    // #937: a segment to a far-off point walked one dab per spacing step along its whole
    // length (~3e14 for 1e15 px). Skipping the dabs that can't reach a field node must leave
    // every result bit-identical to the full walk.
    use LiquifyTool::*;
    let bounds = Rect::new(-7, 3, 89, 67);
    let segments: [([f64; 3], [f64; 3]); 10] = [
        ([10.0, 20.0, 1.0], [70.0, 50.0, 0.5]),        // inside
        ([-900.0, 30.0, 1.0], [40.0, 31.0, 1.0]),      // in from far left
        ([40.0, 30.0, 1.0], [40.0, 4000.0, 0.2]),      // out downwards
        ([-500.0, -500.0, 1.0], [600.0, 600.0, 1.0]),  // diagonal through
        ([-300.0, -9.0, 1.0], [300.0, -9.0, 1.0]),     // parallel, just above (the brush reaches)
        ([-300.0, -400.0, 1.0], [300.0, -400.0, 1.0]), // parallel, far above (reaches nothing)
        ([88.5, -50.0, 1.0], [88.5, 120.0, 1.0]),      // vertical, along the right edge
        ([200.0, 200.0, 1.0], [201.0, 900.0, 1.0]),    // entirely outside
        ([0.0, 0.0, 1.0], [0.25, 0.1, 1.0]),           // shorter than one step
        ([60.0, 40.0, 1.0], [-2000.0, 45.0, 1.0]),     // continues the stroke before it, out left
    ];
    for tool in [
        ForwardWarp,
        PushLeft,
        Reconstruct,
        Smooth,
        TwirlCw,
        TwirlCcw,
        Pucker,
        Bloat,
        Freeze,
        Thaw,
    ] {
        for size in [6.0, 40.0] {
            for (a, b) in segments {
                let s = LiquifyStroke::new(tool, size);
                let (mut fast, mut full) = (
                    LiquifyField::new(bounds, 2.0),
                    LiquifyField::new(bounds, 2.0),
                );
                // Some displacement first, so Reconstruct and Smooth have something to undo.
                for f in [&mut fast, &mut full] {
                    f.stroke_segment(
                        &LiquifyStroke::new(ForwardWarp, 30.0),
                        [20.0, 20.0, 1.0],
                        [60.0, 40.0, 1.0],
                    );
                }
                let dirty = fast.stroke_segment(&s, a, b);
                assert_eq!(
                    dirty,
                    full_walk(&mut full, &s, a, b),
                    "{tool:?} {size} {a:?} {b:?}"
                );
                assert!(
                    fast.d == full.d && fast.freeze == full.freeze,
                    "{tool:?} {size} {a:?} {b:?}"
                );
            }
        }
    }
}

/// The same path sampled by `parts` pointer events per corner-to-corner leg, unevenly.
pub(super) fn sampled(path: &[[f64; 3]], parts: usize) -> Vec<Vec<f64>> {
    let mut out = vec![path[0].to_vec()];
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        // Uneven steps, as a real pointer gives: t grows quadratically.
        for k in 1..=parts {
            let t = (k * k) as f64 / (parts * parts) as f64;
            out.push((0..3).map(|i| a[i] + (b[i] - a[i]) * t).collect());
        }
    }
    out
}

/// #2606: dabs restarted at every pointer event, so a fast mouse (few far-apart events) and a
/// slow one (many close ones) warped the same path differently. Spacing now runs on along the
/// whole stroke: any sampling of a path gives the same field.
#[test]
fn the_result_does_not_depend_on_how_finely_the_path_was_sampled() {
    use LiquifyTool::*;
    let path = [
        [20.0, 20.0, 1.0],
        [62.0, 26.0, 0.6],
        [70.0, 50.0, 0.9],
        [28.0, 55.0, 1.0],
        [30.0, 30.0, 1.0],
    ];
    for tool in [
        ForwardWarp,
        PushLeft,
        TwirlCw,
        Bloat,
        Reconstruct,
        Smooth,
        Freeze,
    ] {
        for size in [12.0, 40.0] {
            let field = |parts: usize| {
                let mut warp = LiquifyStroke::new(ForwardWarp, 30.0);
                warp.points = vec![vec![30.0, 30.0], vec![50.0, 40.0]];
                let mut s = LiquifyStroke::new(tool, size);
                s.points = sampled(&path, parts);
                // Some displacement first, so Reconstruct and Smooth have something to change.
                LiquifyField::from_strokes(bounds(), 2.0, &[warp, s])
            };
            let coarse = field(1);
            for parts in [3, 17, 60] {
                let fine = field(parts);
                let dd = coarse
                    .d
                    .iter()
                    .zip(&fine.d)
                    .map(|(p, q)| (p[0] - q[0]).abs().max((p[1] - q[1]).abs()))
                    .fold(0.0f32, f32::max);
                let df = coarse
                    .freeze
                    .iter()
                    .zip(&fine.freeze)
                    .map(|(p, q)| (p - q).abs())
                    .fold(0.0f32, f32::max);
                assert!(dd < 1e-3 && df < 1e-4, "{tool:?} {size}px, {parts} events per leg: field differs by {dd} px, mask by {df}");
            }
        }
    }
}

/// A long drag whose events land exactly on dab positions, along a diagonal: rounding must not
/// add or drop the dab at a segment's end, or the deep displacement a long Forward Warp builds
/// up differs by a whole dab between a slow and a fast pointer.
#[test]
fn a_long_diagonal_drag_gives_one_field_at_any_event_spacing() {
    let field = |step: f64| {
        let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 40.0);
        s.points = (0..=(300.0 / step) as usize)
            .map(|k| vec![40.0 + k as f64 * step * 0.6, 20.0 + k as f64 * step * 0.8])
            .collect();
        LiquifyField::from_strokes(Rect::new(0, 0, 320, 300), 2.0, &[s])
    };
    let (slow, fast) = (field(2.0), field(60.0));
    let dd = slow
        .d
        .iter()
        .zip(&fast.d)
        .map(|(p, q)| (p[0] - q[0]).abs().max((p[1] - q[1]).abs()))
        .fold(0.0f32, f32::max);
    assert!(
        slow.max_displacement() > 50.0,
        "the drag builds up a deep displacement"
    );
    assert!(dd < 1e-3, "slow and fast pointer fields differ by {dd} px");
}

#[test]
fn a_stroke_interrupted_by_a_jump_restarts_its_spacing() {
    // A segment that does not start where the last one ended begins a new run of dabs there.
    let s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 20.0);
    let mut f = LiquifyField::new(bounds(), 2.0);
    f.stroke_begin(&s, [10.0, 10.0, 1.0]);
    f.stroke_segment(&s, [10.0, 10.0, 1.0], [10.3, 10.0, 1.0]);
    let mut g = LiquifyField::new(bounds(), 2.0);
    f.stroke_segment(&s, [40.0, 30.0, 1.0], [50.0, 30.0, 1.0]);
    g.stroke_begin(&s, [40.0, 30.0, 1.0]);
    g.stroke_segment(&s, [40.0, 30.0, 1.0], [50.0, 30.0, 1.0]);
    assert_eq!(f, g);
    // Hostile points: nothing panics and the next segment starts afresh.
    f.stroke_segment(&s, [f64::NAN, 3.0, 1.0], [4.0, f64::INFINITY, 1.0]);
    f.stroke_segment(&s, [4.0, 3.0, f64::NAN], [9.0, 3.0, 1.0]);
}

#[test]
fn a_segment_to_a_far_off_point_returns() {
    // The issue's request: about 3.3e14 steps before the fix. Also a segment whose length
    // overflows f64.
    let mut f = LiquifyField::new(Rect::new(0, 0, 24, 16), 2.0);
    let s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 30.0);
    let dirty = f.stroke_segment(&s, [0.0, 0.0, 1.0], [1e15, 0.0, 1.0]);
    assert!(!dirty.is_empty() && dirty.x1 <= 64, "{dirty:?}");
    f.stroke_segment(&s, [-1e308, 5.0, 1.0], [1e308, 5.0, 1.0]);
    f.stroke_segment(
        &LiquifyStroke::new(LiquifyTool::Bloat, 30.0),
        [5.0, -1e15, 1.0],
        [5.0, 1e15, 1.0],
    );
}
