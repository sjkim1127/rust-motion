//! Color blending, opacity scaling, and custom PSD compositing.

use crate::scene::{BlendMode, Color};
use rayon::prelude::*;
use tiny_skia::{BlendMode as SkBlendMode, Pixmap};

pub(crate) fn apply_opacity(color: Color, opacity: f32) -> tiny_skia::Color {
    let a = (color.a as f32 * opacity.clamp(0.0, 1.0)) as u8;
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, a)
}

pub(crate) fn tiny_skia_blend_mode(mode: BlendMode) -> Option<SkBlendMode> {
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => Some(SkBlendMode::SourceOver),
        BlendMode::Multiply => Some(SkBlendMode::Multiply),
        BlendMode::Screen => Some(SkBlendMode::Screen),
        BlendMode::Overlay => Some(SkBlendMode::Overlay),
        BlendMode::Darken => Some(SkBlendMode::Darken),
        BlendMode::Lighten => Some(SkBlendMode::Lighten),
        BlendMode::ColorDodge => Some(SkBlendMode::ColorDodge),
        BlendMode::ColorBurn => Some(SkBlendMode::ColorBurn),
        BlendMode::HardLight => Some(SkBlendMode::HardLight),
        BlendMode::SoftLight => Some(SkBlendMode::SoftLight),
        BlendMode::Difference => Some(SkBlendMode::Difference),
        BlendMode::Exclusion => Some(SkBlendMode::Exclusion),
        _ => None,
    }
}

pub(crate) fn composite_custom_psd_blend(
    backdrop: &mut Pixmap,
    source: &Pixmap,
    mode: rust_motion_color::BlendMode,
    opacity: f32,
    offset_x: i32,
    offset_y: i32,
) {
    if opacity <= 0.0 {
        return;
    }
    let bw = backdrop.width() as i32;
    let bh = backdrop.height() as i32;
    let sw = source.width() as i32;
    let sh = source.height() as i32;

    let x_min = offset_x.max(0);
    let y_min = offset_y.max(0);
    let x_max = (offset_x + sw).min(bw);
    let y_max = (offset_y + sh).min(bh);

    if x_min >= x_max || y_min >= y_max {
        return;
    }

    let b_data = backdrop.data_mut();
    let s_data = source.data();

    for y in y_min..y_max {
        let sy = y - offset_y;
        let b_row_start = (y as usize * bw as usize) * 4;
        let s_row_start = (sy as usize * sw as usize) * 4;
        for x in x_min..x_max {
            let sx = x - offset_x;
            let b_idx = b_row_start + x as usize * 4;
            let s_idx = s_row_start + sx as usize * 4;

            let sa = s_data[s_idx + 3] as f32 / 255.0;
            if sa <= 0.0 {
                continue;
            }
            let s_rgba = [
                (s_data[s_idx] as f32 / 255.0) / sa,
                (s_data[s_idx + 1] as f32 / 255.0) / sa,
                (s_data[s_idx + 2] as f32 / 255.0) / sa,
                sa,
            ];

            let ba = b_data[b_idx + 3] as f32 / 255.0;
            let b_rgba = if ba > 0.0 {
                [
                    (b_data[b_idx] as f32 / 255.0) / ba,
                    (b_data[b_idx + 1] as f32 / 255.0) / ba,
                    (b_data[b_idx + 2] as f32 / 255.0) / ba,
                    ba,
                ]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };

            let out_straight = rust_motion_color::blend::composite(mode, b_rgba, s_rgba, opacity);
            let oa = out_straight[3].clamp(0.0, 1.0);
            if oa <= 0.0 {
                b_data[b_idx] = 0;
                b_data[b_idx + 1] = 0;
                b_data[b_idx + 2] = 0;
                b_data[b_idx + 3] = 0;
            } else {
                b_data[b_idx] = (out_straight[0].clamp(0.0, 1.0) * oa * 255.0 + 0.5) as u8;
                b_data[b_idx + 1] = (out_straight[1].clamp(0.0, 1.0) * oa * 255.0 + 0.5) as u8;
                b_data[b_idx + 2] = (out_straight[2].clamp(0.0, 1.0) * oa * 255.0 + 0.5) as u8;
                b_data[b_idx + 3] = (oa * 255.0 + 0.5) as u8;
            }
        }
    }
}

pub(crate) fn apply_adjustment_to_pixmap(
    pixmap: &mut Pixmap,
    adj: &rust_motion_color::adjust::Adjustment,
) {
    pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
        let a = pixel[3] as f32 / 255.0;
        if a <= 0.0 {
            return;
        }
        let r = (pixel[0] as f32 / 255.0) / a;
        let g = (pixel[1] as f32 / 255.0) / a;
        let b = (pixel[2] as f32 / 255.0) / a;
        let [or, og, ob] = rust_motion_color::adjust::apply_to_pixel(adj, [r, g, b]);
        pixel[0] = (or.clamp(0.0, 1.0) * a * 255.0 + 0.5) as u8;
        pixel[1] = (og.clamp(0.0, 1.0) * a * 255.0 + 0.5) as u8;
        pixel[2] = (ob.clamp(0.0, 1.0) * a * 255.0 + 0.5) as u8;
    });
}
