//! Multi-band blending with Laplacian pyramids and push-pull hole filling.

use super::*;
use crate::photo_util::par_map;

// ---------- multi-band blending ----------

/// Number of pyramid levels used by [`multiband`] for a seam mask of this size.
pub fn blend_levels(w: usize, h: usize) -> usize {
    let mut s = w.min(h);
    let mut n = 1;
    while s > 16 && n < 7 {
        s /= 2;
        n += 1;
    }
    n
}

fn reduce(w: usize, h: usize, ch: usize, src: &[f32]) -> (usize, usize, Vec<f32>) {
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let k = [1.0f32, 4.0, 6.0, 4.0, 1.0];
    // Separable 5-tap binomial, then decimate.
    let mut tmp = vec![0.0f32; nw * h * ch];
    for y in 0..h {
        for x in 0..nw {
            for c in 0..ch {
                let mut s = 0.0;
                for (t, kv) in k.iter().enumerate() {
                    let xx = (2 * x as i64 + t as i64 - 2).clamp(0, w as i64 - 1) as usize;
                    s += kv * src[(y * w + xx) * ch + c];
                }
                tmp[(y * nw + x) * ch + c] = s / 16.0;
            }
        }
    }
    let mut out = vec![0.0f32; nw * nh * ch];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..ch {
                let mut s = 0.0;
                for (t, kv) in k.iter().enumerate() {
                    let yy = (2 * y as i64 + t as i64 - 2).clamp(0, h as i64 - 1) as usize;
                    s += kv * tmp[(yy * nw + x) * ch + c];
                }
                out[(y * nw + x) * ch + c] = s / 16.0;
            }
        }
    }
    (nw, nh, out)
}

fn expand(sw: usize, sh: usize, ch: usize, src: &[f32], w: usize, h: usize) -> Vec<f32> {
    // Bilinear upsampling at half-pixel-aligned positions.
    let mut out = vec![0.0f32; w * h * ch];
    for y in 0..h {
        let fy = ((y as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy - fy.floor());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx - fx.floor());
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..ch {
                let p = |xx: usize, yy: usize| src[(yy * sw + xx) * ch + c];
                let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * tx;
                let bot = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * tx;
                out[(y * w + x) * ch + c] = top + (bot - top) * ty;
            }
        }
    }
    out
}

/// Fills pixels with zero `wt` from their surroundings (push-pull), so pyramid levels don't
/// pull in black from outside an image.
fn push_pull(w: usize, h: usize, ch: usize, px: &mut [f32], wt: &[f32]) {
    if w <= 1 && h <= 1 {
        return;
    }
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let mut cpx = vec![0.0f32; nw * nh * ch];
    let mut cw = vec![0.0f32; nw * nh];
    for y in 0..h {
        for x in 0..w {
            let (i, j) = (y * w + x, (y / 2) * nw + x / 2);
            let a = wt[i];
            cw[j] += a;
            for c in 0..ch {
                cpx[j * ch + c] += a * px[i * ch + c];
            }
        }
    }
    for j in 0..nw * nh {
        if cw[j] > 0.0 {
            for c in 0..ch {
                cpx[j * ch + c] /= cw[j];
            }
            cw[j] = 1.0;
        }
    }
    if cw.contains(&0.0) && cw.iter().any(|v| *v > 0.0) {
        push_pull(nw, nh, ch, &mut cpx, &cw);
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if wt[i] < 1.0 {
                let j = (y / 2) * nw + x / 2;
                let a = wt[i];
                for c in 0..ch {
                    px[i * ch + c] = a * px[i * ch + c] + (1.0 - a) * cpx[j * ch + c];
                }
            }
        }
    }
}

/// Multi-band blend of `imgs` into a `w × h` panorama (origin 0,0) with one weight map per image
/// over the same grid as its ROI (`weights[i]` has `w_i × h_i` entries). Returns the colour
/// channels (`ch` per pixel) and the total coverage.
pub fn multiband(
    w: usize,
    h: usize,
    imgs: &[RoiImage],
    weights: &[Vec<f32>],
    levels: usize,
) -> (Vec<f32>, Vec<f32>) {
    let ch = imgs.first().map_or(1, |i| i.ch);
    let levels = levels.max(1);
    let align = 1i32 << (levels - 1);
    // Level sizes of the panorama.
    let mut sizes = vec![(w, h)];
    for _ in 1..levels {
        let Some(&(pw, ph)) = sizes.last() else { break };
        sizes.push((pw.div_ceil(2), ph.div_ceil(2)));
    }
    let mut acc: Vec<Vec<f32>> = sizes
        .iter()
        .map(|&(lw, lh)| vec![0.0; lw * lh * ch])
        .collect();
    let mut wsum: Vec<Vec<f32>> = sizes.iter().map(|&(lw, lh)| vec![0.0; lw * lh]).collect();
    let margin = align * 4;
    // Each image on its ROI grown by a margin and aligned to the coarsest level's grid.
    let bands = par_map(imgs.len(), |k| {
        let im = &imgs[k];
        let gx0 = ((im.x0 - margin).max(0) / align) * align;
        let gy0 = ((im.y0 - margin).max(0) / align) * align;
        let gx1 = (im.x0 + im.w as i32 + margin).min(w as i32);
        let gy1 = (im.y0 + im.h as i32 + margin).min(h as i32);
        let (gw, gh) = ((gx1 - gx0).max(1) as usize, (gy1 - gy0).max(1) as usize);
        let mut px = vec![0.0f32; gw * gh * ch];
        let mut cov = vec![0.0f32; gw * gh];
        let mut wt = vec![0.0f32; gw * gh];
        for y in 0..im.h {
            for x in 0..im.w {
                let (gx, gy) = (im.x0 + x as i32 - gx0, im.y0 + y as i32 - gy0);
                if gx < 0 || gy < 0 || gx as usize >= gw || gy as usize >= gh {
                    continue;
                }
                let (i, g) = (y * im.w + x, gy as usize * gw + gx as usize);
                cov[g] = im.alpha[i];
                wt[g] = weights[k][i] * im.alpha[i].min(1.0);
                px[g * ch..(g + 1) * ch].copy_from_slice(&im.px[i * ch..(i + 1) * ch]);
            }
        }
        let bin: Vec<f32> = cov
            .iter()
            .map(|a| if *a > 0.5 { 1.0 } else { 0.0 })
            .collect();
        push_pull(gw, gh, ch, &mut px, &bin);
        // Laplacian of the colours, Gaussian of the weights.
        let mut lap: Vec<(usize, usize, Vec<f32>)> = Vec::new();
        let mut gau: Vec<Vec<f32>> = Vec::new();
        let (mut cw, mut chh, mut cur, mut cwt) = (gw, gh, px, wt);
        for l in 0..levels {
            if l + 1 == levels {
                lap.push((cw, chh, cur.clone()));
                gau.push(cwt.clone());
                break;
            }
            let (nw, nh, down) = reduce(cw, chh, ch, &cur);
            let up = expand(nw, nh, ch, &down, cw, chh);
            lap.push((cw, chh, cur.iter().zip(&up).map(|(a, b)| a - b).collect()));
            gau.push(cwt.clone());
            let (_, _, wdown) = reduce(cw, chh, 1, &cwt);
            cw = nw;
            chh = nh;
            cur = down;
            cwt = wdown;
        }
        (gx0, gy0, lap, gau)
    });
    for (gx0, gy0, lap, gau) in &bands {
        for (l, ((lw, lh, band), g)) in lap.iter().zip(gau).enumerate() {
            let (ox, oy) = ((gx0 >> l) as usize, (gy0 >> l) as usize);
            let (pw, ph) = sizes[l];
            for y in 0..*lh {
                let py = oy + y;
                if py >= ph {
                    break;
                }
                for x in 0..*lw {
                    let px = ox + x;
                    if px >= pw {
                        break;
                    }
                    let (i, p) = (y * lw + x, py * pw + px);
                    let k = g[i];
                    if k <= 0.0 {
                        continue;
                    }
                    wsum[l][p] += k;
                    for c in 0..ch {
                        acc[l][p * ch + c] += k * band[i * ch + c];
                    }
                }
            }
        }
    }
    for l in 0..levels {
        let (pw, ph) = sizes[l];
        for p in 0..pw * ph {
            let k = wsum[l][p];
            for c in 0..ch {
                acc[l][p * ch + c] = if k > 1e-8 {
                    acc[l][p * ch + c] / k
                } else {
                    0.0
                };
            }
        }
    }
    // Collapse.
    // `levels ≥ 1`, so there is always a coarsest level.
    let Some(mut cur) = acc.pop() else {
        return (vec![0.0; w * h * ch], vec![0.0; w * h]);
    };
    for l in (0..levels - 1).rev() {
        let (pw, ph) = sizes[l];
        let (sw, sh) = sizes[l + 1];
        let up = expand(sw, sh, ch, &cur, pw, ph);
        cur = acc[l].iter().zip(&up).map(|(a, b)| a + b).collect();
    }
    // Coverage: union of the images.
    let mut cov = vec![0.0f32; w * h];
    for im in imgs {
        for y in 0..im.h {
            for x in 0..im.w {
                let (px, py) = (im.x0 + x as i32, im.y0 + y as i32);
                if px < 0 || py < 0 || px as usize >= w || py as usize >= h {
                    continue;
                }
                let p = py as usize * w + px as usize;
                cov[p] = cov[p].max(im.alpha[y * im.w + x]);
            }
        }
    }
    (cur, cov)
}
