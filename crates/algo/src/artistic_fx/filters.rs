//! Blur, edge, directional and Kuwahara filters shared by the effects.

use super::*;
use crate::fxutil::{box_blur_n, gauss_blur_n};
use crate::photo_util::{par_rows, par_rows2};

pub(super) fn blur1(p: &mut [f32], w: usize, h: usize, sigma: f32) {
    gauss_blur_n(p, w, h, 1, sigma);
}

pub(super) fn blur3(c: &mut [Px], w: usize, h: usize, sigma: f32) {
    gauss_blur_n(c.as_flattened_mut(), w, h, 3, sigma);
}

/// Sobel gradient (per pixel, edges clamped) of a plane.
pub(super) fn sobel(p: &[f32], w: usize, h: usize) -> (Vec<f32>, Vec<f32>) {
    let at = |x: isize, y: isize| {
        p[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize]
    };
    let mut gx = vec![0.0; w * h];
    let mut gy = vec![0.0; w * h];
    par_rows2(&mut gx, &mut gy, w, |y, gxr, gyr| {
        let y = y as isize;
        for x in 0..w as isize {
            let xi = x as usize;
            gxr[xi] = ((at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x - 1, y) + at(x - 1, y + 1)))
                / 8.0;
            gyr[xi] = ((at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x, y - 1) + at(x + 1, y - 1)))
                / 8.0;
        }
    });
    (gx, gy)
}

pub(super) fn grad_mag(p: &[f32], w: usize, h: usize) -> Vec<f32> {
    let (gx, gy) = sobel(p, w, h);
    gx.iter()
        .zip(&gy)
        .map(|(a, b)| (a * a + b * b).sqrt())
        .collect()
}

/// Running-mean blur of radius `r` steps along direction `d` ((1,0), (0,1), (1,1) or (1,-1))
/// of an interleaved `n`-channel buffer.
pub(super) fn line_blur(buf: &mut [f32], n: usize, w: usize, h: usize, d: (i32, i32), r: usize) {
    if r == 0 || w == 0 || h == 0 {
        return;
    }
    let (wi, hi) = (w as i32, h as i32);
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && x < wi && y < hi;
    let norm = 1.0 / (2 * r + 1) as f64;
    let mut idx = Vec::new();
    let mut vals = Vec::new();
    for sy in 0..hi {
        for sx in 0..wi {
            if inside(sx - d.0, sy - d.1) {
                continue; // not the start of a line
            }
            idx.clear();
            let (mut x, mut y) = (sx, sy);
            while inside(x, y) {
                idx.push((y as usize * w + x as usize) * n);
                x += d.0;
                y += d.1;
            }
            let m = idx.len() as isize;
            for c in 0..n {
                vals.clear();
                vals.extend(idx.iter().map(|&o| buf[o + c] as f64));
                let at = |k: isize| vals[k.clamp(0, m - 1) as usize];
                let mut acc: f64 = (-(r as isize)..=r as isize).map(at).sum();
                for (k, &o) in idx.iter().enumerate() {
                    buf[o + c] = (acc * norm) as f32;
                    acc += at(k as isize + r as isize + 1) - at(k as isize - r as isize);
                }
            }
        }
    }
}

pub(super) fn line_blur3(c: &mut [Px], w: usize, h: usize, d: (i32, i32), r: usize) {
    line_blur(c.as_flattened_mut(), 3, w, h, d, r);
}

pub(super) fn direction(name: &str) -> (i32, i32) {
    match name {
        "horizontal" => (1, 0),
        "vertical" => (0, 1),
        "leftDiagonal" => (1, 1),
        _ => (1, -1), // rightDiagonal: bottom-left to top-right
    }
}

/// Kuwahara smoothing (the flattest of four quadrant means): the painterly base of several
/// Artistic filters. Box means make the cost independent of `r`.
pub(super) fn kuwahara(win: &mut Win, r: usize) {
    let h2 = r.div_ceil(2).max(1);
    let (w, h) = (win.w, win.h);
    let mut m: Vec<f32> = Vec::with_capacity(w * h * 5);
    for p in &win.c {
        let l = lum(p);
        m.extend_from_slice(&[p[0], p[1], p[2], l, l * l]);
    }
    box_blur_n(&mut m, w, h, 5, h2);
    let o = h2 as isize;
    let get = |x: isize, y: isize| {
        let i =
            ((y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize) * 5;
        &m[i..i + 5]
    };
    par_rows(&mut win.c, w, 1, |y, row| {
        let y = y as isize;
        for x in 0..w as isize {
            let mut best = f32::MAX;
            let mut col = [0.0; 3];
            for (dx, dy) in [(-o, -o), (o, -o), (-o, o), (o, o)] {
                let q = get(x + dx, y + dy);
                let var = q[4] - q[3] * q[3];
                if var < best {
                    best = var;
                    col = [q[0], q[1], q[2]];
                }
            }
            row[x as usize] = col;
        }
    });
}

pub(super) fn kuwahara_reach(r: usize) -> i32 {
    2 * r.div_ceil(2).max(1) as i32 + 1
}

pub(super) fn unsharp(win: &mut Win, sigma: f32, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let mut b = win.c.clone();
    blur3(&mut b, win.w, win.h, sigma);
    for (p, q) in win.c.iter_mut().zip(&b) {
        *p = [
            p[0] + (p[0] - q[0]) * amount,
            p[1] + (p[1] - q[1]) * amount,
            p[2] + (p[2] - q[2]) * amount,
        ];
    }
}
