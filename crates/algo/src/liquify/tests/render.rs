//! Rendering: identity, proxy previews and replay determinism across depths.

use super::*;

pub(super) fn sample(st: SampleType) -> Surface {
    let mut s = Surface::new(PixelFormat::new(ColorMode::Rgb, st, true));
    for y in 0..64 {
        for x in 0..96 {
            let v = ((x / 4 + y / 4) % 2) as f32;
            s.write_pixel(x, y, &[v, (x as f32) / 96.0, (y as f32) / 64.0, 1.0]);
        }
    }
    s
}

fn worst(a: &Surface, b: &Surface, r: Rect) -> f32 {
    a.read_region(r)
        .iter()
        .zip(b.read_region(r))
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

#[test]
fn no_strokes_is_identity_at_every_depth() {
    for st in [SampleType::U8, SampleType::U16, SampleType::F32] {
        let s = sample(st);
        let f = LiquifyField::new(bounds(), 2.0);
        assert_eq!(apply_liquify(&s, &f), s);
        // A field with exactly-zero strokes (zero pressure) is still identity.
        let mut st0 = LiquifyStroke::new(LiquifyTool::ForwardWarp, 30.0);
        st0.pressure = 0.0;
        st0.points = vec![vec![40.0, 30.0], vec![60.0, 30.0]];
        let f = LiquifyField::from_strokes(bounds(), 2.0, &[st0]);
        assert!(f.is_identity());
    }
}

#[test]
fn full_resolution_preview_matches_the_unit_scale_proxy_and_partial_updates() {
    let canvas = Rect::new(0, 0, 96, 64);
    for sample_type in [SampleType::U8, SampleType::U16, SampleType::F32] {
        let source = sample(sample_type);
        let mut stroke = LiquifyStroke::new(LiquifyTool::ForwardWarp, 32.0);
        stroke.points = vec![vec![38.0, 30.0, 1.0], vec![52.0, 34.0, 0.8]];
        let mut field = LiquifyField::from_strokes(canvas, 2.0, &[stroke]);

        let proxy = ProxyImage::new(&source, canvas, 96);
        assert_eq!(proxy.scale, 1.0);
        let mut want = vec![[0; 4]; proxy.w * proxy.h];
        proxy.render(&field, [0, 0, proxy.w, proxy.h], &mut want);

        let mut got = vec![[0; 4]; proxy.w * proxy.h];
        let mut scratch = Vec::new();
        assert!(ProxyImage::render_full_resolution(
            &source,
            &field,
            canvas,
            canvas,
            &mut got,
            &mut scratch
        ));
        assert_eq!(got, want, "full-resolution render at {sample_type:?}");

        let mut more = LiquifyStroke::new(LiquifyTool::Bloat, 16.0);
        more.points = vec![vec![43.0, 32.0, 1.0]];
        let dirty = field.apply_stroke(&more);
        proxy.render(&field, proxy.proxy_rect(dirty), &mut want);
        assert!(ProxyImage::render_full_resolution(
            &source,
            &field,
            canvas,
            dirty,
            &mut got,
            &mut scratch
        ));
        assert_eq!(got, want, "partial update at {sample_type:?}");
    }
}

#[test]
fn replay_is_deterministic_and_depths_agree() {
    let mut s = LiquifyStroke::new(LiquifyTool::TwirlCw, 50.0);
    s.points = vec![
        vec![48.0, 32.0, 0.5],
        vec![50.0, 33.0, 1.0],
        vec![52.0, 34.0],
    ];
    let a = LiquifyField::from_strokes(bounds(), 2.0, std::slice::from_ref(&s));
    let b = LiquifyField::from_strokes(bounds(), 2.0, std::slice::from_ref(&s));
    assert_eq!(a, b);
    let r8 = apply_liquify(&sample(SampleType::U8), &a);
    let r32 = apply_liquify(&sample(SampleType::F32), &a);
    assert!(worst(&r8, &r32, bounds()) < 2.0 / 255.0 + 1e-4);
}

#[test]
fn proxy_matches_the_full_render_roughly() {
    let src = sample(SampleType::U8);
    let mut s = LiquifyStroke::new(LiquifyTool::Bloat, 40.0);
    s.points = vec![vec![48.0, 32.0]; 8];
    let f = LiquifyField::from_strokes(bounds(), 2.0, &[s]);
    let p = ProxyImage::new(&src, bounds(), 96);
    assert_eq!((p.w, p.h, p.scale), (96, 64, 1.0));
    let mut out = vec![[0u8; 4]; p.w * p.h];
    p.render(&f, [0, 0, p.w, p.h], &mut out);
    let full = apply_liquify(&src, &f);
    let mut bad = 0;
    for y in 0..64 {
        for x in 0..96 {
            let q = full.pixel(x, y);
            let o = out[y as usize * 96 + x as usize];
            if (f32::from(o[1]) / 255.0 - q[1]).abs() > 0.1 {
                bad += 1;
            }
        }
    }
    assert!(bad < 96 * 64 / 50, "{bad}");
}
