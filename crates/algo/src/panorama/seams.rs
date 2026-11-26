//! Seam finding: minimum-cost paths through the overlap of each new image.

// ---------- seams ----------

/// Seam labels on a `w × h` grid (`-1` = no image): images are added in `order`; each new image
/// takes the part of its overlap with the composite that lies on its side of the cheapest path
/// through the overlap (cost = colour difference).
pub fn seam_labels(
    w: usize,
    h: usize,
    luma: &[Vec<f32>],
    cover: &[Vec<bool>],
    order: &[usize],
) -> Vec<i32> {
    let mut label = vec![-1i32; w * h];
    for &img in order {
        let cov = &cover[img];
        let mut overlap = vec![false; w * h];
        let (mut sx, mut sy, mut no) = (0.0f64, 0.0f64, 0usize);
        let (mut nx, mut ny, mut nn) = (0.0f64, 0.0f64, 0usize);
        for i in 0..w * h {
            if !cov[i] {
                continue;
            }
            nx += (i % w) as f64;
            ny += (i / w) as f64;
            nn += 1;
            if label[i] >= 0 {
                overlap[i] = true;
                sx += (i % w) as f64;
                sy += (i / w) as f64;
                no += 1;
            } else {
                label[i] = img as i32;
            }
        }
        if no == 0 || nn == no {
            continue;
        }
        let (ox, oy) = (sx / no as f64, sy / no as f64);
        let (cx, cy) = (nx / nn as f64, ny / nn as f64);
        let vertical = (cx - ox).abs() >= (cy - oy).abs();
        // Image on the increasing side along the seam's cross axis?
        let positive = if vertical { cx > ox } else { cy > oy };
        let cost = |i: usize| -> f32 { 0.002 + (luma[img][i] - luma[label[i] as usize][i]).abs() };
        let (lines, len) = if vertical { (h, w) } else { (w, h) };
        let idx = |line: usize, k: usize| if vertical { line * w + k } else { k * w + line };
        // DP over lines (rows for a vertical seam), within the overlap.
        let mut acc = vec![f32::INFINITY; w * h];
        let mut from = vec![usize::MAX; w * h];
        let mut prev_line: Option<usize> = None;
        for line in 0..lines {
            let any = (0..len).any(|k| overlap[idx(line, k)]);
            if !any {
                continue;
            }
            for k in 0..len {
                let i = idx(line, k);
                if !overlap[i] {
                    continue;
                }
                let mut best = (0.0f32, usize::MAX);
                if let Some(pl) = prev_line.filter(|pl| *pl + 1 == line) {
                    best = (f32::INFINITY, usize::MAX);
                    for dk in [-1i64, 0, 1] {
                        let kk = k as i64 + dk;
                        if kk < 0 || kk >= len as i64 {
                            continue;
                        }
                        let j = idx(pl, kk as usize);
                        if acc[j] < best.0 {
                            best = (acc[j], kk as usize);
                        }
                    }
                    if best.1 == usize::MAX {
                        best = (0.0, usize::MAX);
                    }
                }
                acc[i] = best.0 + cost(i);
                from[i] = best.1;
            }
            prev_line = Some(line);
        }
        // Backtrack from the cheapest end; lines not reached keep their own best cell.
        let mut seam = vec![usize::MAX; lines];
        let mut line = lines;
        while line > 0 {
            line -= 1;
            if seam[line] != usize::MAX {
                continue;
            }
            let Some(k) = (0..len)
                .filter(|&k| overlap[idx(line, k)])
                .min_by(|&a, &b| acc[idx(line, a)].total_cmp(&acc[idx(line, b)]))
            else {
                continue;
            };
            let (mut l, mut k) = (line, k);
            loop {
                seam[l] = k;
                let f = from[idx(l, k)];
                if f == usize::MAX || l == 0 {
                    break;
                }
                l -= 1;
                if seam[l] != usize::MAX {
                    break;
                }
                k = f;
            }
        }
        for line in 0..lines {
            let s = seam[line];
            if s == usize::MAX {
                continue;
            }
            for k in 0..len {
                let i = idx(line, k);
                if overlap[i] && ((positive && k > s) || (!positive && k < s)) {
                    label[i] = img as i32;
                }
            }
        }
    }
    label
}
