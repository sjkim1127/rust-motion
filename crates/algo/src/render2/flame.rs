//! Filter › Render › Flame: turbulent flame strands.

use super::*;

// ---------------------------------------------------------------------------
// Flame
// ---------------------------------------------------------------------------

/// Flame placement (Photoshop's six flame types).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FlameType {
    /// One flame whose spine is the path (path start = flame base).
    #[default]
    OneFlameAlongPath,
    /// Flames at `interval` along the path, rising from its left side.
    MultipleFlamesAlongPath,
    /// Flames at `interval` along the path, pointing along the path's direction.
    MultipleFlamesPathDirections,
    /// Like `multipleFlamesAlongPath` with strongly varying lengths.
    MultipleFlamesVariousLength,
    /// A candle flame at the start of each subpath.
    CandleLight,
    /// Flames at `interval` along the path, all pointing at `angle`.
    MultipleFlamesOneDirection,
}

/// Overall motion of the flame lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FlameStyle {
    #[default]
    Normal,
    Violent,
    Flat,
}

/// How the flame lines converge towards the tip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FlameShape {
    #[default]
    Parallel,
    ToCenter,
    Spread,
    Oval,
    Pointed,
}

/// Flame options (Photoshop's Flame dialog, Basic + Advanced).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FlameSpec {
    pub flame_type: FlameType,
    /// Flame length in px (1–1000).
    pub length: f32,
    pub randomize_length: bool,
    /// Flame width in px (1–1000).
    pub width: f32,
    /// Direction for `multipleFlamesOneDirection` (degrees, 0 = up, clockwise).
    pub angle: f32,
    /// Spacing between flames along the path in px (1–1000).
    pub interval: f32,
    /// Spread the flames evenly so a closed path loops seamlessly.
    pub adjust_interval_for_loops: bool,
    /// `Some` = custom flame colour; `None` = natural flame colours.
    pub color: Option<[f32; 4]>,
    /// 0–100.
    pub turbulent: f32,
    /// 0–100.
    pub jag: f32,
    /// 0–100 %.
    pub opacity: f32,
    /// Flame lines (complexity) 1–100.
    pub flame_lines: f32,
    /// 0–100: how unevenly the lines start along the flame.
    pub flame_bottom_alignment: f32,
    pub flame_style: FlameStyle,
    pub flame_shape: FlameShape,
    pub randomize_shapes: bool,
    /// 0–100: arrangement seed.
    pub seed: u32,
    /// 0 (draft) … 4 (fine): segments per flame line.
    pub quality: u32,
}

impl Default for FlameSpec {
    fn default() -> Self {
        FlameSpec {
            flame_type: FlameType::OneFlameAlongPath,
            length: 150.0,
            randomize_length: false,
            width: 40.0,
            angle: 0.0,
            interval: 60.0,
            adjust_interval_for_loops: true,
            color: None,
            turbulent: 25.0,
            jag: 25.0,
            opacity: 75.0,
            flame_lines: 20.0,
            flame_bottom_alignment: 20.0,
            flame_style: FlameStyle::Normal,
            flame_shape: FlameShape::Parallel,
            randomize_shapes: false,
            seed: 0,
            quality: 2,
        }
    }
}

/// Natural flame colour at position `t` along the flame (0 = base, 1 = tip).
fn flame_colour(t: f32, custom: Option<[f32; 4]>) -> [f32; 4] {
    let a = (1.0 - t).powf(0.7) * 0.9 + 0.1 * (1.0 - t);
    match custom {
        Some(c) => {
            // Hot near-white core → the colour → a darker rim.
            let core = [c[0] * 0.4 + 0.6, c[1] * 0.4 + 0.6, c[2] * 0.4 + 0.6];
            let rim = [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6];
            let k = t.clamp(0.0, 1.0);
            let rgb = if k < 0.35 {
                let u = k / 0.35;
                [
                    core[0] + (c[0] - core[0]) * u,
                    core[1] + (c[1] - core[1]) * u,
                    core[2] + (c[2] - core[2]) * u,
                ]
            } else {
                let u = (k - 0.35) / 0.65;
                [
                    c[0] + (rim[0] - c[0]) * u,
                    c[1] + (rim[1] - c[1]) * u,
                    c[2] + (rim[2] - c[2]) * u,
                ]
            };
            [rgb[0], rgb[1], rgb[2], a * c[3]]
        }
        None => {
            const STOPS: [(f32, [f32; 3]); 5] = [
                (0.0, [1.0, 0.98, 0.85]),
                (0.2, [1.0, 0.86, 0.35]),
                (0.45, [1.0, 0.55, 0.08]),
                (0.75, [0.9, 0.25, 0.02]),
                (1.0, [0.6, 0.08, 0.0]),
            ];
            let k = t.clamp(0.0, 1.0);
            let mut rgb = STOPS[4].1;
            for w in STOPS.windows(2) {
                if k <= w[1].0 {
                    let u = (k - w[0].0) / (w[1].0 - w[0].0);
                    rgb = [
                        w[0].1[0] + (w[1].1[0] - w[0].1[0]) * u,
                        w[0].1[1] + (w[1].1[1] - w[0].1[1]) * u,
                        w[0].1[2] + (w[1].1[2] - w[0].1[2]) * u,
                    ];
                    break;
                }
            }
            [rgb[0], rgb[1], rgb[2], a]
        }
    }
}

/// One flame: its spine (base → tip), how wide it is, and its random stream.
fn draw_flame(out: &mut Vec<Prim>, spine: &Spine, width: f32, spec: &FlameSpec, rng: &mut Rng) {
    let len = spine.len();
    if len < 1.0 {
        return;
    }
    let lines = (spec.flame_lines.clamp(1.0, 100.0) * 0.6 + 3.0).round() as usize;
    let segs = [6usize, 10, 16, 24, 36][spec.quality.min(4) as usize]
        .max((len / 12.0) as usize)
        .min(160);
    let (mut turb, mut jag) = (
        spec.turbulent.clamp(0.0, 100.0) / 100.0,
        spec.jag.clamp(0.0, 100.0) / 100.0,
    );
    match spec.flame_style {
        FlameStyle::Violent => {
            turb = (turb * 1.8 + 0.2).min(2.0);
            jag = (jag * 1.8 + 0.15).min(2.0);
        }
        FlameStyle::Flat => {
            turb *= 0.35;
            jag *= 0.35;
        }
        FlameStyle::Normal => {}
    }
    let shape = if spec.randomize_shapes {
        [
            FlameShape::Parallel,
            FlameShape::ToCenter,
            FlameShape::Spread,
            FlameShape::Oval,
            FlameShape::Pointed,
        ][(rng.next_u64() % 5) as usize]
    } else {
        spec.flame_shape
    };
    let opacity = spec.opacity.clamp(0.0, 100.0) / 100.0;
    let half = width.max(1.0) / 2.0;
    let seed = rng.next_u64();
    // The whole flame sways; each line adds its own flicker on top.
    let sway = |u: f32| turb * half * 1.6 * fbm1(u * 1.7, seed ^ 0x5A5A) * u.powf(1.4);
    // A soft additive body glow along the spine.
    for k in 0..6 {
        let u = k as f32 / 6.0 * 0.75 + 0.05;
        let ((px, py), (dx, dy)) = spine.at(u * len);
        let o = sway(u);
        let c = flame_colour(u, spec.color);
        let r = half * (1.25 - u) + 2.0;
        out.push(
            Prim::ellipse(
                (px - dy * o, py + dx * o),
                r,
                r * 1.2,
                dy.atan2(dx),
                [c[0], c[1], c[2], 0.16 * opacity],
                [c[0], c[1] * 0.6, c[2] * 0.3, 0.0],
            )
            .with_soft(r.max(2.0))
            .additive(),
        );
    }
    let line_r = (half / lines as f32 * 2.2).clamp(0.8, 10.0);
    for li in 0..lines {
        let lane = if lines == 1 {
            0.0
        } else {
            li as f32 / (lines - 1) as f32 * 2.0 - 1.0
        };
        let lseed = seed ^ (li as u64).wrapping_mul(0x51_7CC1_B727_220A);
        let start = (spec.flame_bottom_alignment.clamp(0.0, 100.0) / 100.0) * rng.f() * 0.4;
        // Outer lines are shorter, giving the flame its tongue shape.
        let reach = (1.0 - 0.55 * lane.abs().powf(1.5)) * rng.range(0.7, 1.0);
        let end = (start + reach * (1.0 - start)).clamp(start + 0.05, 1.0);
        let lr = line_r * rng.range(0.6, 1.3);
        let mut prev: Option<((f32, f32), f32, f32)> = None;
        for k in 0..=segs {
            let u = start + (end - start) * k as f32 / segs as f32;
            let s = u * len;
            let ((px, py), (dx, dy)) = spine.at(s);
            let (nx, ny) = (-dy, dx);
            let spread = match shape {
                FlameShape::Parallel => 1.0 - 0.6 * u,
                FlameShape::ToCenter => 1.0 - u,
                FlameShape::Spread => 1.0 + u,
                FlameShape::Oval => (std::f32::consts::PI * (0.15 + 0.85 * u)).sin() * 1.3,
                FlameShape::Pointed => (1.0 - u).powi(2),
            };
            let flick =
                turb * half * 0.5 * fbm1(s / (len * 0.2 + 6.0) + li as f32 * 0.37, lseed) * u
                    + jag * half * 0.25 * noise1(s / 5.0, lseed ^ 0x77) * u;
            let off = lane * half * spread + sway(u) + flick;
            let p = (px + nx * off, py + ny * off);
            let r = lr * (1.0 - u * 0.75);
            // Lines fade out towards their own tip.
            let fade_t = ((u - start) / (end - start).max(1e-3)).clamp(0.0, 1.0);
            if let Some((q, rq, uq)) = prev {
                // Fade in over the first tenth (a soft base) and out towards the line's tip.
                let f = |c: [f32; 4], uu: f32, t: f32| {
                    [
                        c[0],
                        c[1],
                        c[2],
                        c[3] * opacity
                            * (1.0 - t.powi(3))
                            * ((uu - start) / 0.08).clamp(0.0, 1.0).sqrt(),
                    ]
                };
                out.push(
                    Prim::capsule(
                        q,
                        p,
                        rq,
                        r,
                        f(flame_colour(uq, spec.color), uq, fade_t),
                        f(flame_colour(u, spec.color), u, fade_t),
                    )
                    .with_soft(1.0 + lr * 0.6)
                    .lighten(),
                );
            }
            prev = Some((p, r, u));
        }
    }
}

/// Builds flame primitives. `paths` are flattened polylines in document pixels.
pub fn flame(spec: &FlameSpec, paths: &[Vec<(f32, f32)>]) -> Vec<Prim> {
    let mut out = Vec::new();
    let mut rng = Rng::new(spec.seed, 0xF1A3);
    let length = spec.length.clamp(1.0, 1000.0);
    let width = spec.width.clamp(1.0, 1000.0);
    let interval = spec.interval.clamp(1.0, 1000.0);
    let pick_len = |rng: &mut Rng, var: f32| {
        if spec.randomize_length || var > 0.0 {
            length * rng.range(1.0 - var.max(0.4), 1.0 + var.max(0.4) * 0.5)
        } else {
            length
        }
    };
    for path in paths.iter().filter(|p| !p.is_empty()) {
        let spine = Spine::new(path.clone());
        let total = spine.len();
        match spec.flame_type {
            FlameType::OneFlameAlongPath if path.len() >= 2 && total >= 1.0 => {
                draw_flame(&mut out, &spine, width, spec, &mut rng)
            }
            FlameType::CandleLight | FlameType::OneFlameAlongPath => {
                // A candle: a short teardrop rising straight up from the start.
                let base = path[0];
                let h = length.min(400.0);
                let candle = Spine::new(vec![base, (base.0, base.1 - h)]);
                let c = FlameSpec {
                    flame_shape: FlameShape::Oval,
                    turbulent: spec.turbulent * 0.3,
                    jag: spec.jag * 0.2,
                    ..spec.clone()
                };
                draw_flame(&mut out, &candle, (width * 0.6).max(4.0), &c, &mut rng);
                let core = flame_colour(0.05, spec.color);
                out.push(
                    Prim::ellipse(
                        (base.0, base.1 - h * 0.28),
                        (width * 0.12).max(1.5),
                        h * 0.22,
                        0.0,
                        [
                            core[0],
                            core[1],
                            core[2],
                            0.9 * spec.opacity.clamp(0.0, 100.0) / 100.0,
                        ],
                        [core[0], core[1], core[2], 0.0],
                    )
                    .with_soft(2.0)
                    .additive(),
                );
            }
            _ => {
                let closed = path.len() > 2 && {
                    let (a, b) = (path[0], path[path.len() - 1]);
                    (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0
                };
                let count = if spec.adjust_interval_for_loops && closed {
                    let c = (total / interval).round();
                    if c.is_finite() && c > 1.0 {
                        (c as usize).min(2048)
                    } else {
                        1
                    }
                } else {
                    let c = (total / interval).floor();
                    if c.is_finite() && c >= 0.0 {
                        (c as usize).saturating_add(1).min(2048)
                    } else {
                        1
                    }
                };
                let step = if spec.adjust_interval_for_loops && closed {
                    total / count as f32
                } else {
                    interval
                };
                for k in 0..count {
                    if out.len() >= 65536 {
                        break;
                    }
                    let s = k as f32 * step;
                    let ((x, y), (dx, dy)) = spine.at(s);
                    let dir = match spec.flame_type {
                        FlameType::MultipleFlamesPathDirections => (dx, dy),
                        FlameType::MultipleFlamesOneDirection => {
                            let a = spec.angle.to_radians();
                            (a.sin(), -a.cos())
                        }
                        // Left of the path direction (up for a left-to-right path).
                        _ => (dy, -dx),
                    };
                    let var = if spec.flame_type == FlameType::MultipleFlamesVariousLength {
                        0.7
                    } else {
                        0.0
                    };
                    let l = pick_len(&mut rng, var);
                    let fl = Spine::new(vec![(x, y), (x + dir.0 * l, y + dir.1 * l)]);
                    draw_flame(&mut out, &fl, width, spec, &mut rng);
                }
            }
        }
    }
    out
}

/// The path Flame uses when the document has none: a gentle arc across the lower part of the canvas.
pub fn default_flame_path(canvas: Rect) -> Vec<(f32, f32)> {
    let (w, h) = (canvas.width() as f32, canvas.height() as f32);
    (0..=32)
        .map(|k| {
            let t = k as f32 / 32.0;
            (
                canvas.x0 as f32 + w * (0.15 + 0.7 * t),
                canvas.y0 as f32 + h * (0.8 - 0.08 * (std::f32::consts::PI * t).sin()),
            )
        })
        .collect()
}
