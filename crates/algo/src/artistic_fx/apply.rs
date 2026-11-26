//! Running a stack of effects: the dispatcher and the per-effect pixel code.

use super::*;
use crate::artistic::{GalleryEffect, GalleryFilter as F};
use crate::fxutil::{rgba, set_rgba};
use crate::image::Image;
use crate::noise::hash01;
use crate::Ctx;
use rust_motion_geom::Rect;
use std::f32::consts::PI;

/// Runs a stack of effects over `out` (the source covers `out` grown by [`reach`]).
pub(crate) fn run(effects: &[GalleryEffect], src: &Image, out: Rect, ctx: &Ctx) -> Vec<f32> {
    let n = src.ch;
    let r = src.rect;
    let (w, h) = (r.width() as usize, r.height() as usize);
    if effects.is_empty() || w == 0 || h == 0 {
        return src.crop(out);
    }
    let mut win = Win {
        r,
        w,
        h,
        c: src
            .data
            .chunks_exact(n)
            .map(|px| rgb(rgba(ctx, px)))
            .collect(),
    };
    for e in effects {
        apply(e, &mut win, ctx);
    }
    let mut res = src.crop(out);
    let ow = out.width() as usize;
    for (i, px) in res.chunks_exact_mut(n).enumerate() {
        let (x, y) = (out.x0 + (i % ow) as i32, out.y0 + (i / ow) as i32);
        let a = if ctx.alpha { px[n - 1] } else { 1.0 };
        let c = win.at(x, y);
        set_rgba(ctx, px, [clamp01(c[0]), clamp01(c[1]), clamp01(c[2]), a]);
        if ctx.alpha {
            px[n - 1] = a;
        }
    }
    res
}

fn apply(e: &GalleryEffect, win: &mut Win, ctx: &Ctx) {
    let g = |k: &str| e.get(k);
    let (w, h) = (win.w, win.h);
    let (fg, bg) = (rgb(e.foreground), rgb(e.background));
    match e.filter {
        // ---------------- Artistic ----------------
        F::ColoredPencil => {
            let pw = g("pencilWidth");
            let pressure = g("strokePressure") / 15.0;
            let paper = map(bg, |v| v * (0.7 + 0.3 * g("paperBrightness") / 50.0));
            let l = win.lum();
            let e2 = grad_mag(&l, w, h);
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let d = 1.0 - l[i];
                let h1 = strokes(x, y, (1, 1), 5.0 + pw * 3.0, 0.5 + pw * 0.45, 11);
                let h2 = strokes(x, y, (1, -1), 5.0 + pw * 3.0, 0.5 + pw * 0.45, 12);
                let hatch = h1.max(h2 * smoothstep(0.3, 0.8, d));
                let cov = clamp01(d * (0.55 + pressure) + e2[i] * 3.0 - (1.0 - hatch) * 0.8 + 0.25);
                win.c[i] = mix(paper, map(win.c[i], |v| v * 0.88), cov);
            }
        }
        F::Cutout => {
            blur3(&mut win.c, w, h, cutout_sigma(e));
            let levels = g("numberOfLevels").round();
            for p in &mut win.c {
                // Flat tones per luminance level, keeping each region's hue.
                let l = lum(p).max(1e-4);
                let k = posterize(l, levels) / l;
                *p = map(*p, |v| posterize(clamp01(v * k), levels * 2.0));
            }
        }
        F::DryBrush | F::Fresco => {
            let fresco = e.filter == F::Fresco;
            kuwahara(win, 1 + g("brushSize") as usize);
            unsharp(win, 1.0, if fresco { 0.9 } else { 0.5 });
            let levels = 3.0 + g("brushDetail");
            let amp = 0.02 + (g("texture") - 1.0) * 0.05;
            let l = if fresco {
                grad_mag(&win.lum(), w, h)
            } else {
                Vec::new()
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let n = (hash01(x, y, 0, 21) - 0.5) * amp;
                let mut p = map(win.c[i], |v| posterize(v, levels) * 0.6 + v * 0.4 + n);
                if fresco {
                    p = map(p, |v| {
                        clamp01((v - 0.5) * 1.3 + 0.45).powf(1.15)
                            * (1.0 - clamp01(l[i] * 2.5) * 0.6)
                    });
                }
                win.c[i] = p;
            }
        }
        F::FilmGrain => {
            let grain = g("grain") / 20.0;
            let (area, inten) = (g("highlightArea"), g("intensity"));
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let l = lum(&win.c[i]);
                let n = (hash01(x, y, 0, 31) - 0.5) * grain * 0.5 * (0.3 + 2.8 * l * (1.0 - l));
                win.c[i] = highlights(map(win.c[i], |v| v + n), l, area, inten);
            }
        }
        F::NeonGlow => {
            let size = g("glowSize");
            let br = g("glowBrightness") / 50.0;
            let glow = rgb(e.color);
            let l = win.lum();
            let mut m: Vec<f32> = if size >= 0.0 {
                l.clone()
            } else {
                l.iter().map(|v| 1.0 - v).collect()
            };
            blur1(&mut m, w, h, size.abs() * 0.6 + 0.5);
            for i in 0..win.c.len() {
                let base = mix(fg, bg, l[i]);
                let k = clamp01(m[i] * 1.2) * br * 2.0;
                win.c[i] = [0, 1, 2].map(|c| 1.0 - (1.0 - base[c]) * (1.0 - clamp01(glow[c] * k)));
            }
        }
        F::PaintDaubs => {
            let ty = e.choice("brushType");
            kuwahara(win, daubs_radius(e));
            let sharp = g("sharpness") / 40.0;
            match ty {
                "wideBlurry" => blur3(&mut win.c, w, h, 1.5),
                "wideSharp" => unsharp(win, 1.5, sharp * 4.0),
                _ => unsharp(win, 1.2, sharp * 2.5),
            }
            if matches!(ty, "lightRough" | "darkRough" | "sparkle") {
                let edges = if ty == "sparkle" {
                    grad_mag(&win.lum(), w, h)
                } else {
                    Vec::new()
                };
                for i in 0..win.c.len() {
                    let (x, y) = win.xy(i);
                    let n = fbm(x as f32 / 2.0, y as f32 / 2.0, 41) - 0.5;
                    win.c[i] = match ty {
                        "lightRough" => map(win.c[i], |v| v + n.max(0.0) * 0.35),
                        "darkRough" => map(win.c[i], |v| v * (1.0 - n.max(0.0) * 0.7)),
                        _ => map(win.c[i], |v| {
                            v + clamp01(edges[i] * 4.0)
                                * smoothstep(0.75, 0.95, hash01(x, y, 0, 42))
                                * 0.8
                        }),
                    };
                }
            }
        }
        F::PaletteKnife => {
            kuwahara(win, (g("strokeSize") / 3.0).max(1.0) as usize);
            let levels = 2.0 + g("strokeDetail") * 3.0;
            for p in &mut win.c {
                *p = map(*p, |v| posterize(v, levels) * 0.5 + v * 0.5);
            }
            blur3(&mut win.c, w, h, g("softness") * 0.5);
        }
        F::PlasticWrap => {
            let hs = g("highlightStrength") / 20.0;
            let mut l = win.lum();
            blur1(&mut l, w, h, plastic_sigma(e));
            let (gx, gy) = sobel(&l, w, h);
            let lv = normalize3([-0.45, -0.65, 1.0]);
            let hv = normalize3([lv[0], lv[1], lv[2] + 1.0]);
            for i in 0..win.c.len() {
                let k = 14.0;
                let n = normalize3([-k * gx[i], -k * gy[i], 1.0]);
                let ndh = (n[0] * hv[0] + n[1] * hv[1] + n[2] * hv[2]).max(0.0);
                let tilt = 1.0 - n[2];
                let spec = ndh.powf(30.0) * smoothstep(0.01, 0.12, tilt)
                    + smoothstep(0.25, 0.6, tilt) * 0.25;
                win.c[i] = map(win.c[i], |v| v * 0.88 + spec * hs * 1.3);
            }
        }
        F::PosterEdges => {
            let mut l = win.lum();
            blur1(&mut l, w, h, 0.6 + g("edgeThickness") * 0.6);
            let edge = grad_mag(&l, w, h);
            let inten = g("edgeIntensity") / 10.0;
            let levels = g("posterization") + 2.0;
            for i in 0..win.c.len() {
                let ink =
                    smoothstep(0.015, 0.06, edge[i] * (1.0 + inten * 3.0)) * (0.55 + inten * 0.45);
                win.c[i] = map(win.c[i], |v| posterize(v, levels) * (1.0 - ink));
            }
        }
        F::RoughPastels => {
            // Chalk strokes up to the right; Stroke Detail keeps more of the image inside them.
            let len = g("strokeLength");
            let det = (g("strokeDetail") - 1.0) / 19.0;
            if len >= 1.0 {
                let mut soft = win.c.clone();
                blur3(&mut soft, w, h, 0.6 + (1.0 - det) * 1.2);
                let src = Win {
                    r: win.r,
                    w,
                    h,
                    c: soft,
                };
                for i in 0..win.c.len() {
                    let (x, y) = win.xy(i);
                    let (c, t) = paint_stroke(&src, x, y, (1, -1), 2.0 + len, 1.5, 51);
                    // A chalk stroke is a little lighter in its middle, grainy along its length.
                    let n = (strokes(x, y, (1, -1), 2.0 + len * 0.5, 0.8, 52) - 0.5) * 0.12
                        + (0.5 - (t - 0.5).abs()) * 0.06;
                    win.c[i] = map(mix(c, win.c[i], det * 0.6), |v| v + n);
                }
            }
            // Bright areas are thick chalk; in dark ones it's scraped off to show the texture.
            texturize_by(
                win,
                e.choice("texture"),
                g("scaling"),
                g("relief"),
                e.choice("light"),
                e.flag("invert"),
                |p| 0.15 + 0.85 * (1.0 - clamp01(lum(p))),
            );
        }
        F::SmudgeStick => {
            line_blur3(&mut win.c, w, h, (1, 1), (g("strokeLength") * 1.5) as usize);
            let (area, inten) = (g("highlightArea"), g("intensity"));
            for p in &mut win.c {
                let l = lum(p);
                let q = map(*p, |v| v * (0.7 + 0.3 * l));
                *p = highlights(q, l, area, inten);
            }
        }
        F::Sponge => {
            kuwahara(win, 1 + g("brushSize") as usize);
            let def = g("definition") / 25.0;
            let sc = 1.5 + g("smoothness") * 0.5;
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let s = fbm(x as f32 / sc, y as f32 / sc, 61);
                let hole = smoothstep(0.58, 0.75, s);
                win.c[i] = map(win.c[i], |v| {
                    clamp01((v - 0.5) * (1.0 + def * 0.6) + 0.5) * (1.0 - hole * def * 0.4)
                });
            }
        }
        F::Underpainting => {
            let orig = win.c.clone();
            blur3(&mut win.c, w, h, 0.5 + g("brushSize") * 0.4);
            for (p, o) in win.c.iter_mut().zip(&orig) {
                *p = saturate(mix(*o, *p, 0.8), 1.15);
            }
            let cov = g("textureCoverage") / 40.0;
            texturize(
                win,
                e.choice("texture"),
                g("scaling"),
                g("relief") * (0.5 + cov * 1.5),
                e.choice("light"),
                e.flag("invert"),
            );
        }
        F::Watercolor => {
            kuwahara(win, watercolor_radius(e));
            let sh = g("shadowIntensity") / 10.0;
            let edge = grad_mag(&win.lum(), w, h);
            let amp = (g("texture") - 1.0) * 0.04;
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let n = (fbm(x as f32 / 3.0, y as f32 / 3.0, 71) - 0.5) * amp;
                let k = 1.0 - clamp01(edge[i] * (3.0 + sh * 8.0)) * 0.6;
                win.c[i] = map(saturate(win.c[i], 1.2), |v| {
                    clamp01(v * k + n).powf(1.1 + sh * 0.6)
                });
            }
        }
        // ---------------- Brush Strokes ----------------
        F::AccentedEdges => {
            let mut l = win.lum();
            blur1(&mut l, w, h, 0.4 + g("smoothness") * 0.3);
            let mut edge = grad_mag(&l, w, h);
            blur1(&mut edge, w, h, g("edgeWidth") * 0.5);
            let eb = g("edgeBrightness") / 50.0;
            for i in 0..win.c.len() {
                let k = clamp01(edge[i] * 6.0);
                win.c[i] = if eb >= 0.5 {
                    map(win.c[i], |v| v + (1.0 - v) * k * (eb - 0.5) * 2.0)
                } else {
                    map(win.c[i], |v| v * (1.0 - k * (0.5 - eb) * 2.0))
                };
            }
        }
        F::AngledStrokes => {
            // Light areas are painted in strokes going one way, dark areas the other way;
            // Direction Balance moves the split (0: all down-right, 100: all up-right).
            let len = g("strokeLength").max(3.0);
            let bal = g("directionBalance") / 100.0;
            let mut soft = win.c.clone();
            blur3(&mut soft, w, h, 0.8);
            let l: Vec<f32> = soft.iter().map(lum).collect();
            let src = Win {
                r: win.r,
                w,
                h,
                c: soft,
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let k = smoothstep(1.0 - bal - 0.05, 1.0 - bal + 0.05, l[i]);
                let (down, _) = paint_stroke(&src, x, y, (1, 1), len, 2.0, 83);
                let (up, _) = paint_stroke(&src, x, y, (1, -1), len, 2.0, 84);
                let n = (strokes(x, y, (1, 1), len * 0.5, 0.7, 85) - 0.5) * (1.0 - k)
                    + (strokes(x, y, (1, -1), len * 0.5, 0.7, 86) - 0.5) * k;
                win.c[i] = map(mix(down, up, k), |v| v + n * 0.1);
            }
            unsharp(win, 1.0, 0.2 + g("sharpness") / 10.0 * 2.3);
        }
        F::Crosshatch => {
            let len = g("strokeLength");
            let r = (len / 3.0) as usize;
            let st = g("strength");
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let n = (hash01(x, y, 0, 81) - 0.5) * 0.12 * st;
                win.c[i] = map(win.c[i], |v| v + n);
            }
            let mut a = win.c.clone();
            line_blur3(&mut a, w, h, (1, 1), r);
            line_blur3(&mut win.c, w, h, (1, -1), r);
            for i in 0..win.c.len() {
                win.c[i] = mix(a[i], win.c[i], 0.5);
            }
            unsharp(win, 1.0, g("sharpness") / 20.0 * 3.0);
        }
        F::DarkStrokes => {
            let (bal, bi, wi) = (
                g("balance") / 10.0,
                g("blackIntensity") / 10.0,
                g("whiteIntensity") / 10.0,
            );
            let l = win.lum();
            let mut a = win.c.clone();
            line_blur3(&mut a, w, h, (1, 1), 3);
            line_blur3(&mut win.c, w, h, (1, -1), 8);
            let t = 0.25 + bal * 0.5;
            for i in 0..win.c.len() {
                let d = map(a[i], |v| v * (1.0 - bi * 0.85));
                let li = map(win.c[i], |v| v + (1.0 - v) * wi * 0.5);
                win.c[i] = mix(d, li, smoothstep(t - 0.1, t + 0.1, l[i]));
            }
        }
        F::InkOutlines => {
            let (di, li) = (g("darkIntensity") / 50.0, g("lightIntensity") / 50.0);
            let l = win.lum();
            let edge = grad_mag(&l, w, h);
            line_blur3(&mut win.c, w, h, (1, 1), (g("strokeLength") / 2.0) as usize);
            for i in 0..win.c.len() {
                let ink = clamp01(edge[i] * 5.0) * di * 1.5;
                let (lt, dk) = (
                    smoothstep(0.5, 1.0, l[i]) * li,
                    smoothstep(0.5, 0.0, l[i]) * di * 0.8,
                );
                win.c[i] = map(win.c[i], |v| {
                    (v + (1.0 - v) * lt) * (1.0 - dk) * (1.0 - clamp01(ink))
                });
            }
        }
        F::Spatter => {
            // Smoothness smooths the random displacement: a larger feature size and, as with any
            // averaged noise, a smaller reach. Low smoothness sprays specks out to the full
            // radius; high smoothness leaves a torn but attached, hard edge (what Photoshop
            // shows at Smoothness 15).
            let rad = g("sprayRadius");
            let feature = 1.0 + g("smoothness") * 0.35;
            let reach = (rad * 1.2 / feature).min(rad);
            let src = Win {
                r: win.r,
                w,
                h,
                c: win.c.clone(),
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 / feature, y as f32 / feature);
                let dx = (fbm(fx, fy, 91) - 0.5) * 3.0 * reach;
                let dy = (fbm(fx, fy, 92) - 0.5) * 3.0 * reach;
                win.c[i] = src.sample(x as f32 + 0.5 + dx, y as f32 + 0.5 + dy);
            }
        }
        F::SprayedStrokes => {
            let rad = g("sprayRadius");
            let sc = 1.2;
            let src = Win {
                r: win.r,
                w,
                h,
                c: win.c.clone(),
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 / sc, y as f32 / sc);
                let dx = (vnoise(fx, fy, 91) - 0.5) * 2.0 * rad;
                let dy = (vnoise(fx, fy, 92) - 0.5) * 2.0 * rad;
                win.c[i] = src.sample(x as f32 + 0.5 + dx, y as f32 + 0.5 + dy);
            }
            line_blur3(
                &mut win.c,
                w,
                h,
                direction(e.choice("strokeDirection")),
                (g("strokeLength") / 2.0) as usize,
            );
        }
        F::SumiE => {
            let sw = g("strokeWidth");
            let (pr, con) = (g("strokePressure") / 15.0, g("contrast") / 40.0);
            blur3(&mut win.c, w, h, sw * 0.2);
            line_blur3(&mut win.c, w, h, (1, 1), (sw / 2.0) as usize);
            for p in &mut win.c {
                let l = lum(p);
                let ink = smoothstep(0.65, 0.15, l).powf(1.0 - pr * 0.5);
                *p = map(*p, |v| {
                    clamp01((v - 0.5) * (1.0 + con) + 0.5) * (1.0 - ink * 0.9 * (0.5 + con))
                });
            }
        }
        // ---------------- Distort ----------------
        F::DiffuseGlow => {
            let (gr, ga, ca) = (
                g("graininess") / 10.0,
                g("glowAmount") / 20.0,
                g("clearAmount") / 20.0,
            );
            let thr = 0.2 + ca * 0.7;
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let l = lum(&win.c[i]);
                let mut m = smoothstep(thr - 0.3, thr + 0.1, l) * ga * 1.4;
                if hash01(x, y, 0, 101) < gr * 0.3 * (m + 0.15) {
                    m += 0.5;
                }
                win.c[i] = mix(win.c[i], bg, clamp01(m));
            }
        }
        F::Glass => {
            let dist = g("distortion");
            let step = 0.5 + g("smoothness") * 0.5;
            let kind = e.choice("texture");
            let s = (g("scaling") / 100.0).clamp(0.5, 2.0);
            let sign = if e.flag("invert") { -1.0 } else { 1.0 };
            let max = dist * 2.0;
            let src = Win {
                r: win.r,
                w,
                h,
                c: win.c.clone(),
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32, y as f32);
                let gx = texture(kind, fx + step, fy, s) - texture(kind, fx - step, fy, s);
                let gy = texture(kind, fx, fy + step, s) - texture(kind, fx, fy - step, s);
                let dx = (gx * sign * dist * 3.0).clamp(-max, max);
                let dy = (gy * sign * dist * 3.0).clamp(-max, max);
                win.c[i] = src.sample(fx + 0.5 + dx, fy + 0.5 + dy);
            }
        }
        F::OceanRipple => {
            let s = 2.0 + g("rippleSize") * 1.2;
            let mag = g("rippleMagnitude") * 0.9;
            let src = Win {
                r: win.r,
                w,
                h,
                c: win.c.clone(),
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 / s, y as f32 / s);
                let dx = (vnoise(fx, fy, 111) - 0.5) * 2.0 * mag;
                let dy = (vnoise(fx, fy, 112) - 0.5) * 2.0 * mag;
                win.c[i] = src.sample(x as f32 + 0.5 + dx, y as f32 + 0.5 + dy);
            }
        }
        // ---------------- Sketch ----------------
        F::BasRelief => {
            let mut l = win.lum();
            let orig = l.clone();
            blur1(&mut l, w, h, basrelief_sigma(e));
            let (gx, gy) = sobel(&l, w, h);
            let lt = light_dir(e.choice("light"));
            let k = 3.0 + g("detail") * 0.4;
            for i in 0..win.c.len() {
                let t = clamp01(orig[i] * 0.5 + 0.25 + k * -(gx[i] * lt.0 + gy[i] * lt.1));
                win.c[i] = mix(fg, bg, t);
            }
        }
        F::ChalkCharcoal => {
            let (ca, ch, pr) = (
                g("charcoalArea") / 20.0,
                g("chalkArea") / 20.0,
                g("strokePressure") / 5.0,
            );
            let mid = mix(fg, bg, 0.5);
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let l = lum(&win.c[i]);
                let cn = strokes(x, y, (1, 1), 9.0, 0.9, 121) - 0.5;
                let kn = strokes(x, y, (1, -1), 9.0, 0.9, 122) - 0.5;
                let cc = smoothstep(
                    0.25,
                    0.6,
                    (1.0 - l) * (0.4 + ca * 1.2) + cn * (0.6 + pr * 0.4) - 0.15,
                );
                let kc = smoothstep(
                    0.25,
                    0.6,
                    l * (0.4 + ch * 1.2) + kn * (0.6 + pr * 0.4) - 0.15,
                );
                win.c[i] = mix(mix(mid, fg, cc), bg, kc * (1.0 - cc));
            }
        }
        F::Charcoal => {
            let (th, det, bal) = (
                g("charcoalThickness"),
                g("detail") / 5.0,
                g("lightDarkBalance") / 100.0,
            );
            let mut l = win.lum();
            blur1(&mut l, w, h, 1.0);
            let edge = grad_mag(&l, w, h);
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let sn = strokes(x, y, (1, 1), 8.0 + th * 3.0, 0.6 + th * 0.5, 131) - 0.5;
                let cov = smoothstep(1.0 - bal - 0.2, 1.0 - bal + 0.2, 1.0 - l[i] + sn * 0.6)
                    + clamp01(edge[i] * det * 6.0);
                win.c[i] = mix(bg, fg, clamp01(cov));
            }
        }
        F::Chrome => {
            let (det, sm) = (g("detail") / 10.0, g("smoothness") / 10.0);
            let mut l = win.lum();
            blur1(&mut l, w, h, 1.0 + sm * 4.0);
            let edge = grad_mag(&l, w, h);
            for i in 0..win.c.len() {
                let v = 0.5 + 0.5 * ((l[i] * (2.5 + det * 3.0) + edge[i] * 6.0) * 2.0 * PI).sin();
                win.c[i] = grey(smoothstep(0.05, 0.95, v));
            }
        }
        F::ConteCrayon => {
            let (fl, bl) = (g("foregroundLevel") / 15.0, g("backgroundLevel") / 15.0);
            let mid = mix(fg, bg, 0.5);
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let l = lum(&win.c[i]);
                let sn = strokes(x, y, (1, -1), 7.0, 1.0, 141) - 0.5;
                let cf = clamp01((0.6 - l) * 2.0 * fl * 1.4 + sn * 0.5);
                let cb = clamp01((l - 0.4) * 2.0 * bl * 1.4 - sn * 0.5);
                win.c[i] = mix(mix(mid, fg, cf), bg, cb * (1.0 - cf));
            }
            texturize(
                win,
                e.choice("texture"),
                g("scaling"),
                g("relief"),
                e.choice("light"),
                e.flag("invert"),
            );
        }
        F::GraphicPen => {
            let r = (g("strokeLength") / 2.0) as usize;
            let bal = g("lightDarkBalance") / 100.0;
            let l = win.lum();
            let mut n: Vec<f32> = (0..win.c.len())
                .map(|i| {
                    let (x, y) = win.xy(i);
                    hash01(x, y, 0, 151)
                })
                .collect();
            line_blur(&mut n, 1, w, h, direction(e.choice("strokeDirection")), r);
            let sd = 0.2887 / ((2 * r + 1) as f32).sqrt();
            for i in 0..win.c.len() {
                let z = (n[i] - 0.5) / sd;
                let t = l[i] + z * 0.16;
                win.c[i] = mix(bg, fg, smoothstep(bal + 0.04, bal - 0.04, t));
            }
        }
        F::HalftonePattern => {
            let size = g("size");
            let con = g("contrast") / 50.0;
            let cell = 3.0 + size * 2.0;
            let mut l = win.lum();
            blur1(&mut l, w, h, 0.5 + size * 0.3);
            let b = ctx.bounds;
            let (cx, cy) = ((b.x0 + b.x1) as f32 / 2.0, (b.y0 + b.y1) as f32 / 2.0);
            let ty = e.choice("patternType");
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let d = clamp01((1.0 - l[i] - 0.5) * (1.0 + con * 3.0) + 0.5);
                let aa = 1.0 / cell;
                let ink = match ty {
                    "circle" => {
                        let r = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() / cell;
                        let q = (r.fract() - 0.5).abs() * 2.0;
                        smoothstep(d + aa, d - aa, q)
                    }
                    "line" => {
                        let q = ((fy / cell).fract() - 0.5).abs() * 2.0;
                        smoothstep(d + aa, d - aa, q)
                    }
                    _ => {
                        let (u, v) = ((fx / cell).fract() - 0.5, (fy / cell).fract() - 0.5);
                        let q = (u * u + v * v).sqrt() * 2.0;
                        let rr = d.sqrt() * 1.15;
                        smoothstep(rr + aa, rr - aa, q)
                    }
                };
                win.c[i] = mix(bg, fg, ink);
            }
        }
        F::NotePaper => {
            let (bal, gr, rel) = (
                g("imageBalance") / 50.0,
                g("graininess") / 20.0,
                g("relief") / 25.0,
            );
            let mut l = win.lum();
            blur1(&mut l, w, h, 1.0);
            let mut m: Vec<f32> = l
                .iter()
                .map(|&v| smoothstep(bal - 0.05, bal + 0.05, v))
                .collect();
            blur1(&mut m, w, h, 1.2);
            let (gx, gy) = sobel(&m, w, h);
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let n = (hash01(x, y, 0, 161) - 0.5) * gr * 0.3;
                let f = shade(gx[i], gy[i], light_dir("topLeft"), rel * 6.0).clamp(0.0, 2.0);
                win.c[i] = map(mix(fg, bg, m[i]), |v| (v + n) * f);
            }
        }
        F::Photocopy => {
            let (det, dk) = (g("detail"), g("darkness") / 50.0);
            let l = win.lum();
            let mut b = l.clone();
            blur1(&mut b, w, h, 1.0 + det * 0.5);
            for i in 0..win.c.len() {
                let hp = l[i] - b[i];
                let ink = smoothstep(0.0, 0.06, -hp * (1.0 + dk * 8.0))
                    + smoothstep(0.35, 0.05, l[i]) * dk * 1.5;
                win.c[i] = mix(bg, fg, clamp01(ink));
            }
        }
        F::Plaster => {
            let bal = g("imageBalance") / 50.0;
            let mut l = win.lum();
            blur1(&mut l, w, h, 0.5 + g("smoothness") * 0.8);
            let mut m: Vec<f32> = l
                .iter()
                .map(|&v| smoothstep(bal + 0.1, bal - 0.1, v))
                .collect();
            blur1(&mut m, w, h, 1.5);
            let (gx, gy) = sobel(&m, w, h);
            let lt = light_dir(e.choice("light"));
            for i in 0..win.c.len() {
                let f = shade(gx[i], gy[i], lt, 6.0).clamp(0.0, 2.0);
                win.c[i] = map(mix(bg, fg, m[i] * 0.6), |v| v * f);
            }
        }
        F::Reticulation => {
            let (den, fl, bl) = (
                g("density") / 50.0,
                g("foregroundLevel") / 50.0,
                g("backgroundLevel") / 50.0,
            );
            let ink_c = mix(bg, fg, 0.3 + fl * 0.7);
            let paper = mix(bg, fg, bl * 0.6);
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let n =
                    0.5 * hash01(x, y, 0, 171) + 0.5 * vnoise(x as f32 / 1.8, y as f32 / 1.8, 172);
                let t = (1.0 - lum(&win.c[i])) + (n - 0.5) * (0.4 + den * 1.2);
                win.c[i] = mix(paper, ink_c, smoothstep(0.45, 0.55, t));
            }
        }
        F::Stamp => {
            let bal = g("lightDarkBalance") / 50.0;
            let mut l = win.lum();
            blur1(&mut l, w, h, 0.3 + g("smoothness") * 0.3);
            for i in 0..win.c.len() {
                win.c[i] = mix(bg, fg, smoothstep(bal + 0.02, bal - 0.02, l[i]));
            }
        }
        F::TornEdges => {
            let (bal, sm, con) = (g("imageBalance") / 50.0, g("smoothness"), g("contrast"));
            let mut l = win.lum();
            blur1(&mut l, w, h, 1.0);
            let rough = (16.0 - sm) / 15.0 * 0.35;
            let aa = 0.25 / con;
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let t = l[i] + (fbm(x as f32 / 2.0, y as f32 / 2.0, 181) - 0.5) * rough;
                win.c[i] = mix(bg, fg, smoothstep(bal + aa, bal - aa, t));
            }
        }
        F::WaterPaper => {
            let fl = g("fiberLength");
            let (br, co) = (g("brightness") / 100.0, g("contrast") / 100.0);
            let r = (fl / 6.0) as usize;
            line_blur3(&mut win.c, w, h, (1, 0), r);
            line_blur3(&mut win.c, w, h, (0, 1), r);
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let fib =
                    strokes(x, y, (1, 0), fl, 1.0, 191).max(strokes(x, y, (0, 1), fl, 1.0, 192));
                win.c[i] = map(win.c[i], |v| {
                    (v * (0.85 + 0.25 * fib) - 0.5) * (co * 1.6 + 0.2) + 0.5 + (br - 0.5) * 0.8
                });
            }
        }
        // ---------------- Stylize ----------------
        F::GlowingEdges => {
            blur3(&mut win.c, w, h, 0.3 + g("smoothness") * 0.25);
            let mut e3 = vec![[0.0f32; 3]; win.c.len()];
            for c in 0..3 {
                let plane: Vec<f32> = win.c.iter().map(|p| p[c]).collect();
                for (o, v) in e3.iter_mut().zip(grad_mag(&plane, w, h)) {
                    o[c] = v;
                }
            }
            blur3(&mut e3, w, h, g("edgeWidth") * 0.4);
            let k = 2.0 + g("edgeBrightness") * 1.2;
            for (p, q) in win.c.iter_mut().zip(&e3) {
                *p = map(*q, |v| clamp01(v * k));
            }
        }
        // ---------------- Texture ----------------
        F::Craquelure => {
            let (sp, dep, bri) = (
                g("crackSpacing"),
                g("crackDepth") / 10.0,
                g("crackBrightness") / 10.0,
            );
            let cell = sp * 1.3 + 1.0;
            let width = 0.8 + dep * 0.8;
            let crack = |x: f32, y: f32| {
                let (d1, d2, _) = voronoi(x, y, cell, 201);
                1.0 - smoothstep(0.0, width, d2 - d1)
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let c0 = crack(fx, fy);
                let (gx, gy) = (crack(fx + 1.0, fy) - c0, crack(fx, fy + 1.0) - c0);
                let f = shade(-gx, -gy, light_dir("topLeft"), dep * 2.0).clamp(0.0, 2.0);
                win.c[i] = map(win.c[i], |v| {
                    v * (0.55 + bri * 0.5) * (1.0 - c0 * dep * 0.6) * f
                });
            }
        }
        F::Grain => {
            let (it, co) = (g("intensity") / 100.0, g("contrast") / 100.0);
            let ty = e.choice("grainType");
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let hc = |c: u32| hash01(x, y, c, 211) - 0.5;
                let p = win.c[i];
                let q = match ty {
                    "soft" => [
                        p[0] + hc(0) * it * 0.4,
                        p[1] + hc(1) * it * 0.4,
                        p[2] + hc(2) * it * 0.4,
                    ],
                    "sprinkles" => {
                        if hash01(x, y, 3, 211) < it * 0.15 {
                            bg
                        } else {
                            p
                        }
                    }
                    "clumped" => map(p, |v| {
                        v + (vnoise(x as f32 / 2.0, y as f32 / 2.0, 212) - 0.5) * it * 1.2
                    }),
                    "contrasty" => {
                        map(p, |v| (v - 0.5) * (1.0 + it * 2.0) + 0.5 + hc(4) * it * 0.5)
                    }
                    "enlarged" => {
                        let (fx, fy) = (x as f32 / 3.0, y as f32 / 3.0);
                        [
                            p[0] + (vnoise(fx, fy, 213) - 0.5) * it,
                            p[1] + (vnoise(fx, fy, 214) - 0.5) * it,
                            p[2] + (vnoise(fx, fy, 215) - 0.5) * it,
                        ]
                    }
                    "stippled" => {
                        let st = if lum(&p) > hash01(x, y, 5, 211) {
                            bg
                        } else {
                            fg
                        };
                        mix(p, st, 0.3 + it * 0.7)
                    }
                    "horizontal" => map(p, |v| {
                        v + (vnoise(x as f32 / 25.0, y as f32, 216) - 0.5) * it
                    }),
                    "vertical" => map(p, |v| {
                        v + (vnoise(x as f32, y as f32 / 25.0, 217) - 0.5) * it
                    }),
                    "speckle" => {
                        if hash01(x, y, 6, 211) < it * 0.1 {
                            mix(p, fg, 0.85)
                        } else {
                            p
                        }
                    }
                    _ => [
                        p[0] + hc(0) * it * 0.8,
                        p[1] + hc(1) * it * 0.8,
                        p[2] + hc(2) * it * 0.8,
                    ],
                };
                win.c[i] = map(q, |v| (v - 0.5) * (0.5 + co) + 0.5);
            }
        }
        F::MosaicTiles => {
            let (ts, gw, lg) = (g("tileSize"), g("groutWidth"), g("lightenGrout") / 10.0);
            let edge = |x: f32, y: f32| {
                let jx = x + (vnoise(x / ts * 0.7, y / ts * 0.7, 221) - 0.5) * ts * 0.3;
                let jy = y + (vnoise(x / ts * 0.7, y / ts * 0.7, 222) - 0.5) * ts * 0.3;
                let (u, v) = (jx.rem_euclid(ts), jy.rem_euclid(ts));
                u.min(ts - u).min(v.min(ts - v))
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let de = edge(fx, fy);
                let grout = smoothstep(gw * 0.5 + 0.7, gw * 0.5 - 0.3, de);
                let hgt = |xx: f32, yy: f32| smoothstep(gw * 0.5, gw * 0.5 + 2.5, edge(xx, yy));
                let s = (hgt(fx - 1.0, fy - 1.0) - hgt(fx + 1.0, fy + 1.0)) * 0.35;
                let l = lum(&win.c[i]);
                let gc = grey(clamp01(l * 0.5 + lg * 0.45));
                win.c[i] = mix(map(win.c[i], |v| v * (1.0 + s)), gc, grout);
            }
        }
        F::Patchwork => {
            let size = patch_size(e);
            let rel = g("relief") / 25.0;
            let src = Win {
                r: win.r,
                w,
                h,
                c: win.c.clone(),
            };
            let hv = |x: f32, y: f32| {
                let (bx, by) = ((x / size).floor() as i32, (y / size).floor() as i32);
                let (u, v) = (x.rem_euclid(size), y.rem_euclid(size));
                let bevel = smoothstep(0.0, 1.5, u.min(size - u).min(v.min(size - v)));
                hash01(bx, by, 0, 231) * bevel
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (bx, by) = ((fx / size).floor(), (fy / size).floor());
                let c = src.at(((bx + 0.5) * size) as i32, ((by + 0.5) * size) as i32);
                let s = (hv(fx - 1.0, fy - 1.0) - hv(fx + 1.0, fy + 1.0)) * rel * 1.5;
                win.c[i] = map(c, |v| v * (1.0 + s));
            }
        }
        F::StainedGlass => {
            let cell = glass_cell(e);
            let bt = g("borderThickness");
            let li = g("lightIntensity") / 10.0;
            let b = ctx.bounds;
            let (cx, cy) = ((b.x0 + b.x1) as f32 / 2.0, (b.y0 + b.y1) as f32 / 2.0);
            let half = (b.width().max(b.height()) as f32 / 2.0).max(1.0);
            let src = Win {
                r: win.r,
                w,
                h,
                c: win.c.clone(),
            };
            for i in 0..win.c.len() {
                let (x, y) = win.xy(i);
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (d1, d2, p) = voronoi(fx, fy, cell, 241);
                let border = smoothstep(bt * 0.5 + 0.6, bt * 0.5 - 0.4, (d2 - d1) * 0.5);
                let dn = (((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() / half).min(1.0);
                let col = map(src.at(p.0 as i32, p.1 as i32), |v| {
                    v * (1.0 + li * 0.6 * (1.0 - dn))
                });
                win.c[i] = mix(col, fg, border);
            }
        }
        F::Texturizer => texturize(
            win,
            e.choice("texture"),
            g("scaling"),
            g("relief"),
            e.choice("light"),
            e.flag("invert"),
        ),
    }
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}
