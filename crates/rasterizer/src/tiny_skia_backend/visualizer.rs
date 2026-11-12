//! Audio spectrum, waveform, and radial visualizer rendering.

use super::shapes::build_rounded_rect;
use super::RenderResources;
use crate::backend::RasterError;
use crate::scene::Color;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub(crate) fn render_audio_visualizer(
    pixmap: &mut Pixmap,
    resources: &RenderResources<'_>,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    style: &crate::scene::VisualizerStyle,
    time: f64,
    transform: Transform,
) -> Result<(), RasterError> {
    resources.security.validate_path(src)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;

    match style {
        crate::scene::VisualizerStyle::Bars {
            count,
            gap,
            radius,
            mirror,
        } => {
            let n_bars = (*count).max(1);
            let bins = resources
                .audios
                .get_spectrum_with_policy(src, time, n_bars, resources.security)
                .unwrap_or_else(|_| vec![0.0; n_bars]);
            let total_gap = gap * (n_bars - 1) as f32;
            let bar_width = ((w - total_gap) / n_bars as f32).max(1.0);

            for (i, &mag) in bins.iter().enumerate() {
                let bx = x + i as f32 * (bar_width + gap);
                let bar_h = (mag * h).max(2.0);

                let (by, bh) = if *mirror {
                    let half_h = bar_h * 0.5;
                    let cy = y + h * 0.5;
                    (cy - half_h, bar_h)
                } else {
                    (y + h - bar_h, bar_h)
                };

                if let Some(rect) = Rect::from_xywh(bx, by, bar_width, bh) {
                    if *radius > 0.0 {
                        let path = build_rounded_rect(bx, by, bar_width, bh, *radius);
                        pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    } else {
                        let path = PathBuilder::from_rect(rect);
                        pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    }
                }
            }
        }
        crate::scene::VisualizerStyle::Wave {
            stroke_width,
            filled,
        } => {
            let n_points = ((w / 4.0) as usize).clamp(16, 256);
            let window_secs = 0.05; // 50ms window
            let points = resources
                .audios
                .get_waveform_slice_with_policy(
                    src,
                    time,
                    window_secs,
                    n_points,
                    resources.security,
                )
                .unwrap_or_else(|_| vec![0.0; n_points]);

            if points.len() < 2 {
                return Ok(());
            }

            let mid_y = y + h * 0.5;
            let half_h = h * 0.45;
            let step_x = w / (points.len() - 1) as f32;

            let mut pb = PathBuilder::new();
            pb.move_to(x, mid_y + points[0] * half_h);

            for (i, &val) in points.iter().enumerate().skip(1) {
                let px = x + i as f32 * step_x;
                let py = mid_y + val * half_h;
                pb.line_to(px, py);
            }

            if *filled {
                let mut fill_pb = pb.clone();
                fill_pb.line_to(x + w, y + h);
                fill_pb.line_to(x, y + h);
                fill_pb.close();
                if let Some(fill_path) = fill_pb.finish() {
                    let mut fill_paint = paint.clone();
                    fill_paint.set_color_rgba8(
                        color.r,
                        color.g,
                        color.b,
                        (color.a as f32 * 0.35) as u8,
                    );
                    pixmap.fill_path(&fill_path, &fill_paint, FillRule::Winding, transform, None);
                }
            }

            if let Some(stroke_path) = pb.finish() {
                let stroke = Stroke {
                    width: *stroke_width,
                    line_cap: tiny_skia::LineCap::Round,
                    line_join: tiny_skia::LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&stroke_path, &paint, &stroke, transform, None);
            }
        }
        crate::scene::VisualizerStyle::Radial {
            radius,
            bar_count,
            bar_length,
        } => {
            let n_bars = (*bar_count).max(8);
            let bins = resources
                .audios
                .get_spectrum_with_policy(src, time, n_bars, resources.security)
                .unwrap_or_else(|_| vec![0.0; n_bars]);

            let cx = x + w * 0.5;
            let cy = y + h * 0.5;
            let r_base = *radius;

            let stroke = Stroke {
                width: (2.0 * std::f32::consts::PI * r_base / n_bars as f32 * 0.6).clamp(1.5, 6.0),
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            };

            for (i, &mag) in bins.iter().enumerate() {
                let angle = (i as f32 / n_bars as f32) * 2.0 * std::f32::consts::PI
                    - std::f32::consts::FRAC_PI_2;
                let cos_a = angle.cos();
                let sin_a = angle.sin();

                let len = (mag * bar_length).max(3.0);
                let x0 = cx + cos_a * r_base;
                let y0 = cy + sin_a * r_base;
                let x1 = cx + cos_a * (r_base + len);
                let y1 = cy + sin_a * (r_base + len);

                let mut pb = PathBuilder::new();
                pb.move_to(x0, y0);
                pb.line_to(x1, y1);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            }
        }
    }

    Ok(())
}
