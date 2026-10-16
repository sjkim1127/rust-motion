//! After Effects Shape Layer Modifiers: Trim Paths, Repeater, Wiggle Paths, and Round Corners.
//!
//! Provides geometric path modifications that operate directly on SVG instructions,
//! matching the procedural behavior of Adobe After Effects vector shape operators.

use serde::{Deserialize, Serialize};

use crate::length::get_instructions_length;
use crate::parser::parse_path;
use crate::types::{Instruction, Point};

// ─── Trim Paths ───────────────────────────────────────────────────────────────

/// Mode of Trim Paths operation across multiple subpaths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrimMode {
    /// Each subpath is trimmed simultaneously with the same start/end/offset.
    #[default]
    Simultaneously,
    /// All subpaths are treated as a single continuous path and trimmed sequentially.
    Individually,
}

/// Splits a flat list of instructions into subpaths based on `MoveTo` commands.
pub fn split_subpaths_insts(instructions: &[Instruction]) -> Vec<Vec<Instruction>> {
    let mut subpaths = Vec::new();
    let mut current = Vec::new();

    for inst in instructions {
        if matches!(inst, Instruction::MoveTo { .. }) && !current.is_empty() {
            subpaths.push(current);
            current = Vec::new();
        }
        current.push(inst.clone());
    }

    if !current.is_empty() {
        subpaths.push(current);
    }

    subpaths
}

/// Trims an SVG path using After Effects Trim Paths semantics.
///
/// * `start`: Fractional start position (0.0 to 1.0, or 0% to 100%).
/// * `end`: Fractional end position (0.0 to 1.0, or 0% to 100%).
/// * `offset`: Fractional rotational offset (0.0 to 1.0, where 1.0 = 360°).
/// * `mode`: `Simultaneously` or `Individually`.
pub fn trim_paths(
    instructions: &[Instruction],
    start: f64,
    end: f64,
    offset: f64,
    mode: TrimMode,
) -> Vec<Instruction> {
    if instructions.is_empty() {
        return Vec::new();
    }

    let subpaths = split_subpaths_insts(instructions);
    if subpaths.is_empty() {
        return Vec::new();
    }

    match mode {
        TrimMode::Simultaneously => {
            let mut result = Vec::new();
            for sub in &subpaths {
                let trimmed = trim_single_subpath(sub, start, end, offset);
                result.extend(trimmed);
            }
            result
        }
        TrimMode::Individually => {
            trim_subpaths_individually(&subpaths, start, end, offset)
        }
    }
}

/// Convenience helper to trim an SVG path string directly.
pub fn trim_path_str(
    path_d: &str,
    start: f64,
    end: f64,
    offset: f64,
    mode: TrimMode,
) -> Vec<Instruction> {
    match parse_path(path_d) {
        Ok(insts) => trim_paths(&insts, start, end, offset, mode),
        Err(_) => Vec::new(),
    }
}

fn trim_single_subpath(
    instructions: &[Instruction],
    start: f64,
    end: f64,
    offset: f64,
) -> Vec<Instruction> {
    let total_len = get_instructions_length(instructions);
    if total_len <= 1e-6 {
        return Vec::new();
    }

    // Normalize start/end with offset into [0, 1)
    let s_raw = start + offset;
    let e_raw = end + offset;

    let s = s_raw.rem_euclid(1.0);
    let e = e_raw.rem_euclid(1.0);
    if (end - start).abs() >= 1.0 {
        // Full loop
        return instructions.to_vec();
    }
    if (end - start).abs() <= 1e-7 {
        // Zero length
        return Vec::new();
    }

    // If s <= e, we take one segment [s, e]
    // If s > e, wrap-around occurs: two segments [s, 1.0] and [0.0, e]
    if s <= e {
        slice_subpath(instructions, s * total_len, e * total_len)
    } else {
        let mut part1 = slice_subpath(instructions, s * total_len, total_len);
        let part2 = slice_subpath(instructions, 0.0, e * total_len);
        part1.extend(part2);
        part1
    }
}

fn trim_subpaths_individually(
    subpaths: &[Vec<Instruction>],
    start: f64,
    end: f64,
    offset: f64,
) -> Vec<Instruction> {
    let lengths: Vec<f64> = subpaths.iter().map(|s| get_instructions_length(s)).collect();
    let total_len: f64 = lengths.iter().sum();
    if total_len <= 1e-6 {
        return Vec::new();
    }

    let s = (start + offset).rem_euclid(1.0);
    let e = (end + offset).rem_euclid(1.0);

    if (end - start).abs() >= 1.0 {
        return subpaths.concat();
    }
    if (end - start).abs() <= 1e-7 {
        return Vec::new();
    }

    let intervals = if s <= e {
        vec![(s * total_len, e * total_len)]
    } else {
        vec![(s * total_len, total_len), (0.0, e * total_len)]
    };

    let mut result = Vec::new();
    for (t_start, t_end) in intervals {
        let mut cursor = 0.0;
        for (i, sub) in subpaths.iter().enumerate() {
            let sub_len = lengths[i];
            let sub_start = cursor;
            let sub_end = cursor + sub_len;

            // Check overlap between [sub_start, sub_end] and [t_start, t_end]
            let overlap_start = t_start.max(sub_start);
            let overlap_end = t_end.min(sub_end);

            if overlap_start < overlap_end {
                let local_start = overlap_start - sub_start;
                let local_end = overlap_end - sub_start;
                let sliced = slice_subpath(sub, local_start, local_end);
                result.extend(sliced);
            }
            cursor += sub_len;
        }
    }
    result
}

/// Slices a single subpath from length `from_len` to `to_len`.
fn slice_subpath(instructions: &[Instruction], from_len: f64, to_len: f64) -> Vec<Instruction> {
    if from_len >= to_len || instructions.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut current_x = 0.0;
    let mut current_y = 0.0;
    let mut start_x = 0.0;
    let mut start_y = 0.0;
    let mut cursor = 0.0;
    let mut has_moved = false;

    for inst in instructions {
        match *inst {
            Instruction::MoveTo { x, y } => {
                current_x = x;
                current_y = y;
                start_x = x;
                start_y = y;
            }
            Instruction::LineTo { x, y } => {
                let dx = x - current_x;
                let dy = y - current_y;
                let seg_len = (dx * dx + dy * dy).sqrt();
                let seg_start = cursor;
                let seg_end = cursor + seg_len;

                let overlap_start = from_len.max(seg_start);
                let overlap_end = to_len.min(seg_end);

                if overlap_start < overlap_end && seg_len > 1e-9 {
                    let t0 = (overlap_start - seg_start) / seg_len;
                    let t1 = (overlap_end - seg_start) / seg_len;

                    let p0_x = current_x + dx * t0;
                    let p0_y = current_y + dy * t0;
                    let p1_x = current_x + dx * t1;
                    let p1_y = current_y + dy * t1;

                    if !has_moved {
                        result.push(Instruction::MoveTo { x: p0_x, y: p0_y });
                        has_moved = true;
                    }
                    result.push(Instruction::LineTo { x: p1_x, y: p1_y });
                }

                current_x = x;
                current_y = y;
                cursor = seg_end;
            }
            Instruction::ClosePath => {
                let dx = start_x - current_x;
                let dy = start_y - current_y;
                let seg_len = (dx * dx + dy * dy).sqrt();
                let seg_start = cursor;
                let seg_end = cursor + seg_len;

                let overlap_start = from_len.max(seg_start);
                let overlap_end = to_len.min(seg_end);

                if overlap_start < overlap_end && seg_len > 1e-9 {
                    let t0 = (overlap_start - seg_start) / seg_len;
                    let t1 = (overlap_end - seg_start) / seg_len;

                    let p0_x = current_x + dx * t0;
                    let p0_y = current_y + dy * t0;
                    let p1_x = current_x + dx * t1;
                    let p1_y = current_y + dy * t1;

                    if !has_moved {
                        result.push(Instruction::MoveTo { x: p0_x, y: p0_y });
                        has_moved = true;
                    }
                    result.push(Instruction::LineTo { x: p1_x, y: p1_y });
                }

                current_x = start_x;
                current_y = start_y;
                cursor = seg_end;
            }
            Instruction::CubicCurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let seg_len = crate::length::cubic_bezier_length(
                    (current_x, current_y),
                    (x1, y1),
                    (x2, y2),
                    (x, y),
                );
                let seg_start = cursor;
                let seg_end = cursor + seg_len;

                let overlap_start = from_len.max(seg_start);
                let overlap_end = to_len.min(seg_end);

                if overlap_start < overlap_end && seg_len > 1e-9 {
                    let local_start = overlap_start - seg_start;
                    let local_end = overlap_end - seg_start;

                    let t0 = cubic_t_at_length(
                        (current_x, current_y),
                        (x1, y1),
                        (x2, y2),
                        (x, y),
                        local_start,
                        seg_len,
                    );
                    let t1 = cubic_t_at_length(
                        (current_x, current_y),
                        (x1, y1),
                        (x2, y2),
                        (x, y),
                        local_end,
                        seg_len,
                    );

                    let (sp0, sp1, sp2, sp3) = slice_cubic_bezier(
                        (current_x, current_y),
                        (x1, y1),
                        (x2, y2),
                        (x, y),
                        t0,
                        t1,
                    );

                    if !has_moved {
                        result.push(Instruction::MoveTo {
                            x: sp0.0,
                            y: sp0.1,
                        });
                        has_moved = true;
                    }
                    result.push(Instruction::CubicCurveTo {
                        x1: sp1.0,
                        y1: sp1.1,
                        x2: sp2.0,
                        y2: sp2.1,
                        x: sp3.0,
                        y: sp3.1,
                    });
                }

                current_x = x;
                current_y = y;
                cursor = seg_end;
            }
            Instruction::QuadCurveTo { x1, y1, x, y } => {
                // Convert quadratic to cubic for uniform de Casteljau slicing
                let c_x1 = current_x + 2.0 / 3.0 * (x1 - current_x);
                let c_y1 = current_y + 2.0 / 3.0 * (y1 - current_y);
                let c_x2 = x + 2.0 / 3.0 * (x1 - x);
                let c_y2 = y + 2.0 / 3.0 * (y1 - y);

                let seg_len = crate::length::cubic_bezier_length(
                    (current_x, current_y),
                    (c_x1, c_y1),
                    (c_x2, c_y2),
                    (x, y),
                );
                let seg_start = cursor;
                let seg_end = cursor + seg_len;

                let overlap_start = from_len.max(seg_start);
                let overlap_end = to_len.min(seg_end);

                if overlap_start < overlap_end && seg_len > 1e-9 {
                    let local_start = overlap_start - seg_start;
                    let local_end = overlap_end - seg_start;

                    let t0 = cubic_t_at_length(
                        (current_x, current_y),
                        (c_x1, c_y1),
                        (c_x2, c_y2),
                        (x, y),
                        local_start,
                        seg_len,
                    );
                    let t1 = cubic_t_at_length(
                        (current_x, current_y),
                        (c_x1, c_y1),
                        (c_x2, c_y2),
                        (x, y),
                        local_end,
                        seg_len,
                    );

                    let (sp0, sp1, sp2, sp3) = slice_cubic_bezier(
                        (current_x, current_y),
                        (c_x1, c_y1),
                        (c_x2, c_y2),
                        (x, y),
                        t0,
                        t1,
                    );

                    if !has_moved {
                        result.push(Instruction::MoveTo {
                            x: sp0.0,
                            y: sp0.1,
                        });
                        has_moved = true;
                    }
                    result.push(Instruction::CubicCurveTo {
                        x1: sp1.0,
                        y1: sp1.1,
                        x2: sp2.0,
                        y2: sp2.1,
                        x: sp3.0,
                        y: sp3.1,
                    });
                }

                current_x = x;
                current_y = y;
                cursor = seg_end;
            }
            Instruction::ArcTo { .. } => {
                // Approximate arc as line for slicing if encountered
                let x = inst_end_x(inst);
                let y = inst_end_y(inst);
                current_x = x;
                current_y = y;
            }
        }
    }

    result
}

#[inline]
fn inst_end_x(inst: &Instruction) -> f64 {
    match *inst {
        Instruction::MoveTo { x, .. }
        | Instruction::LineTo { x, .. }
        | Instruction::CubicCurveTo { x, .. }
        | Instruction::QuadCurveTo { x, .. }
        | Instruction::ArcTo { x, .. } => x,
        Instruction::ClosePath => 0.0,
    }
}

#[inline]
fn inst_end_y(inst: &Instruction) -> f64 {
    match *inst {
        Instruction::MoveTo { y, .. }
        | Instruction::LineTo { y, .. }
        | Instruction::CubicCurveTo { y, .. }
        | Instruction::QuadCurveTo { y, .. }
        | Instruction::ArcTo { y, .. } => y,
        Instruction::ClosePath => 0.0,
    }
}

fn cubic_t_at_length(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    target_len: f64,
    total_len: f64,
) -> f64 {
    if target_len <= 1e-9 {
        return 0.0;
    }
    if target_len >= total_len - 1e-9 {
        return 1.0;
    }

    const STEPS: usize = 32;
    let mut prev = p0;
    let mut cum_len = 0.0;

    let eval = |t: f64| -> (f64, f64) {
        let it = 1.0 - t;
        let x = it * it * it * p0.0 + 3.0 * it * it * t * p1.0 + 3.0 * it * t * t * p2.0 + t * t * t * p3.0;
        let y = it * it * it * p0.1 + 3.0 * it * it * t * p1.1 + 3.0 * it * t * t * p2.1 + t * t * t * p3.1;
        (x, y)
    };

    for i in 1..=STEPS {
        let t = i as f64 / STEPS as f64;
        let curr = eval(t);
        let dx = curr.0 - prev.0;
        let dy = curr.1 - prev.1;
        let seg = (dx * dx + dy * dy).sqrt();

        if cum_len + seg >= target_len {
            let frac = if seg > 1e-9 {
                (target_len - cum_len) / seg
            } else {
                0.0
            };
            let prev_t = (i - 1) as f64 / STEPS as f64;
            return (prev_t + frac * (1.0 / STEPS as f64)).clamp(0.0, 1.0);
        }

        cum_len += seg;
        prev = curr;
    }

    1.0
}

/// Slices a cubic Bézier curve between parameter values `t0` and `t1` (in [0, 1])
/// using two de Casteljau splits.
fn slice_cubic_bezier(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    t0: f64,
    t1: f64,
) -> ((f64, f64), (f64, f64), (f64, f64), (f64, f64)) {
    let t0 = t0.clamp(0.0, 1.0);
    let t1 = t1.clamp(0.0, 1.0);

    // Step 1: Split at t0, keep right half [t0, 1]
    let right = if t0 > 1e-9 {
        let (_, r) = split_cubic_at(p0, p1, p2, p3, t0);
        r
    } else {
        (p0, p1, p2, p3)
    };

    // Step 2: Split right half at adjusted parameter t_adj = (t1 - t0) / (1 - t0)
    let denom = 1.0 - t0;
    let t_adj = if denom > 1e-9 {
        ((t1 - t0) / denom).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let (sliced, _) = split_cubic_at(right.0, right.1, right.2, right.3, t_adj);
    sliced
}

#[inline]
fn split_cubic_at(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    t: f64,
) -> (
    ((f64, f64), (f64, f64), (f64, f64), (f64, f64)),
    ((f64, f64), (f64, f64), (f64, f64), (f64, f64)),
) {
    let lerp = |a: (f64, f64), b: (f64, f64), t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);

    let p01 = lerp(p0, p1, t);
    let p12 = lerp(p1, p2, t);
    let p23 = lerp(p2, p3, t);

    let p012 = lerp(p01, p12, t);
    let p123 = lerp(p12, p23, t);

    let p0123 = lerp(p012, p123, t);

    ((p0, p01, p012, p0123), (p0123, p123, p23, p3))
}

// ─── Repeater ─────────────────────────────────────────────────────────────────

/// Configuration for the After Effects Repeater modifier.
#[derive(Debug, Clone, PartialEq)]
#[derive(Serialize, Deserialize)]
pub struct RepeaterConfig {
    /// Number of copies to create.
    pub copies: usize,
    /// Offset of the copies.
    pub offset: f64,
    /// Delta position added per copy.
    pub position: Point,
    /// Scale factor applied per copy (e.g. `Point::new(1.0, 1.0)` for no scale change).
    pub scale: Point,
    /// Rotation in degrees added per copy.
    pub rotation: f64,
    /// Opacity of the first copy (0.0 to 1.0).
    pub start_opacity: f64,
    /// Opacity of the last copy (0.0 to 1.0).
    pub end_opacity: f64,
    /// Anchor point around which transformation occurs.
    pub anchor_point: Point,
}

impl Default for RepeaterConfig {
    fn default() -> Self {
        Self {
            copies: 3,
            offset: 0.0,
            position: Point::new(100.0, 0.0),
            scale: Point::new(1.0, 1.0),
            rotation: 0.0,
            start_opacity: 1.0,
            end_opacity: 1.0,
            anchor_point: Point::new(0.0, 0.0),
        }
    }
}

/// An individual generated copy from the Repeater modifier.
#[derive(Debug, Clone, PartialEq)]
#[derive(Serialize, Deserialize)]
pub struct RepeaterInstance {
    /// Copy index (0 to `copies - 1`).
    pub index: usize,
    /// Transformed path instructions for this copy.
    pub instructions: Vec<Instruction>,
    /// Calculated opacity for this copy.
    pub opacity: f64,
}

/// Generates repeated, transformed copies of input path instructions.
pub fn repeat_paths(instructions: &[Instruction], config: &RepeaterConfig) -> Vec<RepeaterInstance> {
    if config.copies == 0 || instructions.is_empty() {
        return Vec::new();
    }

    let mut instances = Vec::with_capacity(config.copies);
    let copies_f = config.copies as f64;

    for i in 0..config.copies {
        let t = (i as f64 + config.offset).max(0.0);

        // Compute cumulative transformation for this instance
        let tx = config.position.x * t;
        let ty = config.position.y * t;
        let sx = config.scale.x.powf(t);
        let sy = config.scale.y.powf(t);
        let rot = config.rotation * t;

        // Transform path: Translate(-anchor) -> Scale -> Rotate -> Translate(+anchor + delta)
        let mut insts = instructions.to_vec();
        insts = translate_insts(&insts, -config.anchor_point.x, -config.anchor_point.y);
        insts = scale_insts(&insts, sx, sy);
        insts = rotate_insts(&insts, rot);
        insts = translate_insts(
            &insts,
            config.anchor_point.x + tx,
            config.anchor_point.y + ty,
        );

        // Interpolate opacity between start and end
        let alpha = if config.copies > 1 {
            i as f64 / (copies_f - 1.0)
        } else {
            0.0
        };
        let opacity = config.start_opacity + (config.end_opacity - config.start_opacity) * alpha;

        instances.push(RepeaterInstance {
            index: i,
            instructions: insts,
            opacity: opacity.clamp(0.0, 1.0),
        });
    }

    instances
}

fn translate_insts(insts: &[Instruction], dx: f64, dy: f64) -> Vec<Instruction> {
    insts
        .iter()
        .map(|inst| match *inst {
            Instruction::MoveTo { x, y } => Instruction::MoveTo {
                x: x + dx,
                y: y + dy,
            },
            Instruction::LineTo { x, y } => Instruction::LineTo {
                x: x + dx,
                y: y + dy,
            },
            Instruction::CubicCurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => Instruction::CubicCurveTo {
                x1: x1 + dx,
                y1: y1 + dy,
                x2: x2 + dx,
                y2: y2 + dy,
                x: x + dx,
                y: y + dy,
            },
            Instruction::QuadCurveTo { x1, y1, x, y } => Instruction::QuadCurveTo {
                x1: x1 + dx,
                y1: y1 + dy,
                x: x + dx,
                y: y + dy,
            },
            Instruction::ArcTo {
                rx,
                ry,
                x_axis_rotation,
                large_arc_flag,
                sweep_flag,
                x,
                y,
            } => Instruction::ArcTo {
                rx,
                ry,
                x_axis_rotation,
                large_arc_flag,
                sweep_flag,
                x: x + dx,
                y: y + dy,
            },
            Instruction::ClosePath => Instruction::ClosePath,
        })
        .collect()
}

fn scale_insts(insts: &[Instruction], sx: f64, sy: f64) -> Vec<Instruction> {
    insts
        .iter()
        .map(|inst| match *inst {
            Instruction::MoveTo { x, y } => Instruction::MoveTo {
                x: x * sx,
                y: y * sy,
            },
            Instruction::LineTo { x, y } => Instruction::LineTo {
                x: x * sx,
                y: y * sy,
            },
            Instruction::CubicCurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => Instruction::CubicCurveTo {
                x1: x1 * sx,
                y1: y1 * sy,
                x2: x2 * sx,
                y2: y2 * sy,
                x: x * sx,
                y: y * sy,
            },
            Instruction::QuadCurveTo { x1, y1, x, y } => Instruction::QuadCurveTo {
                x1: x1 * sx,
                y1: y1 * sy,
                x: x * sx,
                y: y * sy,
            },
            Instruction::ArcTo {
                rx,
                ry,
                x_axis_rotation,
                large_arc_flag,
                sweep_flag,
                x,
                y,
            } => Instruction::ArcTo {
                rx: rx * sx,
                ry: ry * sy,
                x_axis_rotation,
                large_arc_flag,
                sweep_flag,
                x: x * sx,
                y: y * sy,
            },
            Instruction::ClosePath => Instruction::ClosePath,
        })
        .collect()
}

fn rotate_insts(insts: &[Instruction], angle_deg: f64) -> Vec<Instruction> {
    let rad = angle_deg.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();

    let rot_pt = |x: f64, y: f64| (x * cos - y * sin, x * sin + y * cos);

    insts
        .iter()
        .map(|inst| match *inst {
            Instruction::MoveTo { x, y } => {
                let (rx, ry) = rot_pt(x, y);
                Instruction::MoveTo { x: rx, y: ry }
            }
            Instruction::LineTo { x, y } => {
                let (rx, ry) = rot_pt(x, y);
                Instruction::LineTo { x: rx, y: ry }
            }
            Instruction::CubicCurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (rx1, ry1) = rot_pt(x1, y1);
                let (rx2, ry2) = rot_pt(x2, y2);
                let (rx, ry) = rot_pt(x, y);
                Instruction::CubicCurveTo {
                    x1: rx1,
                    y1: ry1,
                    x2: rx2,
                    y2: ry2,
                    x: rx,
                    y: ry,
                }
            }
            Instruction::QuadCurveTo { x1, y1, x, y } => {
                let (rx1, ry1) = rot_pt(x1, y1);
                let (rx, ry) = rot_pt(x, y);
                Instruction::QuadCurveTo {
                    x1: rx1,
                    y1: ry1,
                    x: rx,
                    y: ry,
                }
            }
            Instruction::ArcTo {
                rx,
                ry,
                x_axis_rotation,
                large_arc_flag,
                sweep_flag,
                x,
                y,
            } => {
                let (rx_pt, ry_pt) = rot_pt(x, y);
                Instruction::ArcTo {
                    rx,
                    ry,
                    x_axis_rotation: x_axis_rotation + angle_deg,
                    large_arc_flag,
                    sweep_flag,
                    x: rx_pt,
                    y: ry_pt,
                }
            }
            Instruction::ClosePath => Instruction::ClosePath,
        })
        .collect()
}

// ─── Wiggle Paths ─────────────────────────────────────────────────────────────

/// Wiggle Paths point connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[derive(Serialize, Deserialize)]
pub enum WigglePointType {
    /// Sharp corner vertices.
    #[default]
    Corner,
    /// Smooth bezier tangents.
    Smooth,
}

/// Displaces path vertices with procedural noise.
pub fn wiggle_paths(
    instructions: &[Instruction],
    size: f64,
    detail: f64,
    phase: f64,
    point_type: WigglePointType,
) -> Vec<Instruction> {
    if instructions.is_empty() || size <= 1e-6 {
        return instructions.to_vec();
    }

    let mut result = Vec::with_capacity(instructions.len());

    let noise = |seed: f64| -> f64 {
        // High-quality sine-hash coherent noise generator
        let n = (seed * 12.9898 + phase * 78.233).sin() * 43758.5453;
        (n - n.floor()) * 2.0 - 1.0
    };

    let mut idx = 0.0;
    for inst in instructions {
        let dx = noise(idx * detail) * size;
        let dy = noise(idx * detail + 100.0) * size;
        idx += 1.0;

        match *inst {
            Instruction::MoveTo { x, y } => {
                result.push(Instruction::MoveTo {
                    x: x + dx,
                    y: y + dy,
                });
            }
            Instruction::LineTo { x, y } => match point_type {
                WigglePointType::Corner => {
                    result.push(Instruction::LineTo {
                        x: x + dx,
                        y: y + dy,
                    });
                }
                WigglePointType::Smooth => {
                    let cp_dx = noise(idx * detail + 200.0) * size * 0.5;
                    let cp_dy = noise(idx * detail + 300.0) * size * 0.5;
                    result.push(Instruction::QuadCurveTo {
                        x1: x + cp_dx,
                        y1: y + cp_dy,
                        x: x + dx,
                        y: y + dy,
                    });
                }
            },
            Instruction::CubicCurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                result.push(Instruction::CubicCurveTo {
                    x1: x1 + noise(idx) * size,
                    y1: y1 + noise(idx + 1.0) * size,
                    x2: x2 + noise(idx + 2.0) * size,
                    y2: y2 + noise(idx + 3.0) * size,
                    x: x + dx,
                    y: y + dy,
                });
                idx += 4.0;
            }
            Instruction::QuadCurveTo { x1, y1, x, y } => {
                result.push(Instruction::QuadCurveTo {
                    x1: x1 + noise(idx) * size,
                    y1: y1 + noise(idx + 1.0) * size,
                    x: x + dx,
                    y: y + dy,
                });
                idx += 2.0;
            }
            Instruction::ArcTo { .. } | Instruction::ClosePath => {
                result.push(inst.clone());
            }
        }
    }

    result
}

// ─── Round Corners ────────────────────────────────────────────────────────────

/// Procedurally rounds sharp corners of polygon/line paths by a fillet radius `r`.
pub fn round_corners(instructions: &[Instruction], radius: f64) -> Vec<Instruction> {
    if instructions.len() < 3 || radius <= 1e-6 {
        return instructions.to_vec();
    }

    let mut result = Vec::with_capacity(instructions.len() * 2);

    // Extract vertices for line-based segments
    let mut pts: Vec<Point> = Vec::new();
    let mut is_closed = false;

    for inst in instructions {
        match *inst {
            Instruction::MoveTo { x, y } | Instruction::LineTo { x, y } => {
                pts.push(Point::new(x, y));
            }
            Instruction::ClosePath => {
                is_closed = true;
            }
            _ => {
                // If path contains complex curves, preserve without fillet
                return instructions.to_vec();
            }
        }
    }

    let n = pts.len();
    if n < 3 {
        return instructions.to_vec();
    }

    if !is_closed {
        result.push(Instruction::MoveTo {
            x: pts[0].x,
            y: pts[0].y,
        });

        for i in 1..n - 1 {
            let prev = pts[i - 1];
            let curr = pts[i];
            let next = pts[i + 1];

            let v1 = Point::new(prev.x - curr.x, prev.y - curr.y);
            let v2 = Point::new(next.x - curr.x, next.y - curr.y);
            let d1 = (v1.x * v1.x + v1.y * v1.y).sqrt();
            let d2 = (v2.x * v2.x + v2.y * v2.y).sqrt();

            if d1 < 1e-6 || d2 < 1e-6 {
                result.push(Instruction::LineTo {
                    x: curr.x,
                    y: curr.y,
                });
                continue;
            }

            let r = radius.min(d1 * 0.5).min(d2 * 0.5);
            let p_start = Point::new(curr.x + (v1.x / d1) * r, curr.y + (v1.y / d1) * r);
            let p_end = Point::new(curr.x + (v2.x / d2) * r, curr.y + (v2.y / d2) * r);

            result.push(Instruction::LineTo {
                x: p_start.x,
                y: p_start.y,
            });
            result.push(Instruction::QuadCurveTo {
                x1: curr.x,
                y1: curr.y,
                x: p_end.x,
                y: p_end.y,
            });
        }

        result.push(Instruction::LineTo {
            x: pts[n - 1].x,
            y: pts[n - 1].y,
        });
    } else {
        // Closed polygon: compute fillet points for vertex 0
        let prev0 = pts[n - 1];
        let curr0 = pts[0];
        let next0 = pts[1];

        let v1_0 = Point::new(prev0.x - curr0.x, prev0.y - curr0.y);
        let v2_0 = Point::new(next0.x - curr0.x, next0.y - curr0.y);
        let d1_0 = (v1_0.x * v1_0.x + v1_0.y * v1_0.y).sqrt();
        let d2_0 = (v2_0.x * v2_0.x + v2_0.y * v2_0.y).sqrt();
        let r0 = radius.min(d1_0 * 0.5).min(d2_0 * 0.5);

        let p_start0 = Point::new(curr0.x + (v1_0.x / d1_0) * r0, curr0.y + (v1_0.y / d1_0) * r0);
        let p_end0 = Point::new(curr0.x + (v2_0.x / d2_0) * r0, curr0.y + (v2_0.y / d2_0) * r0);

        // Start at fillet end of vertex 0
        result.push(Instruction::MoveTo {
            x: p_end0.x,
            y: p_end0.y,
        });

        // Round vertices 1..n-1
        for i in 1..n {
            let prev = pts[i - 1];
            let curr = pts[i];
            let next = pts[(i + 1) % n];

            let v1 = Point::new(prev.x - curr.x, prev.y - curr.y);
            let v2 = Point::new(next.x - curr.x, next.y - curr.y);
            let d1 = (v1.x * v1.x + v1.y * v1.y).sqrt();
            let d2 = (v2.x * v2.x + v2.y * v2.y).sqrt();

            if d1 < 1e-6 || d2 < 1e-6 {
                result.push(Instruction::LineTo {
                    x: curr.x,
                    y: curr.y,
                });
                continue;
            }

            let r = radius.min(d1 * 0.5).min(d2 * 0.5);
            let p_start = Point::new(curr.x + (v1.x / d1) * r, curr.y + (v1.y / d1) * r);
            let p_end = Point::new(curr.x + (v2.x / d2) * r, curr.y + (v2.y / d2) * r);

            result.push(Instruction::LineTo {
                x: p_start.x,
                y: p_start.y,
            });
            result.push(Instruction::QuadCurveTo {
                x1: curr.x,
                y1: curr.y,
                x: p_end.x,
                y: p_end.y,
            });
        }

        // Finish vertex 0 fillet
        result.push(Instruction::LineTo {
            x: p_start0.x,
            y: p_start0.y,
        });
        result.push(Instruction::QuadCurveTo {
            x1: curr0.x,
            y1: curr0.y,
            x: p_end0.x,
            y: p_end0.y,
        });
        result.push(Instruction::ClosePath);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_path;

    #[test]
    fn test_trim_paths_line_half() {
        let path = "M 0 0 L 100 0";
        let insts = parse_path(path).unwrap();

        let trimmed = trim_paths(&insts, 0.0, 0.5, 0.0, TrimMode::Simultaneously);
        let len = get_instructions_length(&trimmed);
        assert!((len - 50.0).abs() < 1e-3, "Expected 50.0, got {len}");
    }

    #[test]
    fn test_trim_paths_offset_wrap_around() {
        let path = "M 0 0 L 100 0";
        let insts = parse_path(path).unwrap();

        // 50% length with 80% offset: should wrap around [0.8, 1.0] and [0.0, 0.3] -> total 50.0
        let trimmed = trim_paths(&insts, 0.0, 0.5, 0.8, TrimMode::Simultaneously);
        let len = get_instructions_length(&trimmed);
        assert!((len - 50.0).abs() < 1e-3, "Expected 50.0, got {len}");
    }

    #[test]
    fn test_repeater_copies_and_transforms() {
        let path = "M 0 0 L 50 50";
        let insts = parse_path(path).unwrap();

        let cfg = RepeaterConfig {
            copies: 4,
            offset: 0.0,
            position: Point::new(100.0, 0.0),
            scale: Point::new(1.0, 1.0),
            rotation: 0.0,
            start_opacity: 1.0,
            end_opacity: 0.4,
            anchor_point: Point::new(0.0, 0.0),
        };

        let copies = repeat_paths(&insts, &cfg);
        assert_eq!(copies.len(), 4);
        assert_eq!(copies[0].opacity, 1.0);
        assert_eq!(copies[3].opacity, 0.4);

        // Check translation of last copy: 3 * 100 = 300
        let last_inst = &copies[3].instructions;
        if let Instruction::MoveTo { x, y } = last_inst[0] {
            assert!((x - 300.0).abs() < 1e-3);
            assert!((y - 0.0).abs() < 1e-3);
        } else {
            panic!("Expected MoveTo");
        }
    }

    #[test]
    fn test_round_corners_on_rect() {
        let path = "M 0 0 L 100 0 L 100 100 L 0 100 Z";
        let insts = parse_path(path).unwrap();

        let rounded = round_corners(&insts, 10.0);
        // The original has 4 LineTo/MoveTo instructions. Rounded should contain QuadCurveTo fillets.
        let has_curves = rounded.iter().any(|i| matches!(i, Instruction::QuadCurveTo { .. }));
        assert!(has_curves, "Rounded path should contain curves");
    }

    #[test]
    fn test_wiggle_paths_deterministic_and_displaced() {
        let path = "M 0 0 L 100 0";
        let insts = parse_path(path).unwrap();

        let wiggled1 = wiggle_paths(&insts, 10.0, 1.0, 0.0, WigglePointType::Corner);
        let wiggled2 = wiggle_paths(&insts, 10.0, 1.0, 0.0, WigglePointType::Corner);
        assert_eq!(wiggled1, wiggled2, "Same phase must produce deterministic output");

        let wiggled_next_frame = wiggle_paths(&insts, 10.0, 1.0, 1.0, WigglePointType::Corner);
        assert_ne!(wiggled1, wiggled_next_frame, "Different phase must evolve temporally");
    }
}
