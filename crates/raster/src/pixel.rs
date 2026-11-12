//! Color conversion and pixel encoding/decoding utilities.

use rust_motion_color::{read_sample, write_sample, ColorMode, PixelFormat};

/// Encode normalised floats into one pixel's bytes.
pub fn encode_pixel(format: &PixelFormat, px: &[f32], out: &mut [u8]) {
    for (i, v) in px.iter().enumerate().take(format.channels()) {
        write_sample(out, format.sample, i, *v);
    }
}

pub fn decode_pixel(format: &PixelFormat, bytes: &[u8]) -> Vec<f32> {
    (0..format.channels())
        .map(|i| read_sample(bytes, format.sample, i))
        .collect()
}

/// Convert one pixel between formats via sRGB RGBA (CMYK through the built-in CMYK profile;
/// the engine's mode conversions use the document's profiles instead).
pub fn convert_pixel(from: &PixelFormat, to: &PixelFormat, px: &[f32]) -> Vec<f32> {
    if from.mode == to.mode {
        let n = from.mode.color_channels();
        let mut out: Vec<f32> = px[..n].to_vec();
        if to.alpha {
            out.push(if from.alpha { px[n] } else { 1.0 });
        }
        return out;
    }
    let rgba = to_rgba(from, px);
    from_rgba(to, rgba)
}

pub fn to_rgba(format: &PixelFormat, px: &[f32]) -> [f32; 4] {
    let n = format.mode.color_channels();
    let a = if format.alpha { px[n] } else { 1.0 };
    let rgb = match format.mode {
        ColorMode::Grayscale | ColorMode::Bitmap | ColorMode::Duotone => [px[0]; 3],
        ColorMode::Cmyk => rust_motion_color::convert::cmyk_to_rgb([px[0], px[1], px[2], px[3]]),
        ColorMode::Lab => rust_motion_color::convert::lab_to_srgb([
            px[0] * 100.0,
            px[1] * 255.0 - 128.0,
            px[2] * 255.0 - 128.0,
        ]),
        _ => [px[0], px[1], px[2]],
    };
    [rgb[0], rgb[1], rgb[2], a]
}

pub fn from_rgba(format: &PixelFormat, rgba: [f32; 4]) -> Vec<f32> {
    let rgb = [rgba[0], rgba[1], rgba[2]];
    let mut out: Vec<f32> = match format.mode {
        ColorMode::Grayscale | ColorMode::Bitmap | ColorMode::Duotone => {
            vec![rust_motion_color::convert::rgb_to_gray(rgb)]
        }
        ColorMode::Cmyk => rust_motion_color::convert::rgb_to_cmyk(rgb).to_vec(),
        ColorMode::Lab => {
            let l = rust_motion_color::convert::srgb_to_lab(rgb);
            vec![l[0] / 100.0, (l[1] + 128.0) / 255.0, (l[2] + 128.0) / 255.0]
        }
        _ => rgb.to_vec(),
    };
    if format.alpha {
        out.push(rgba[3]);
    }
    out
}

/// [`from_rgba`] without allocating: writes `format.channels()` values into
/// `out` and returns how many were written.
#[inline]
pub fn from_rgba_into(format: &PixelFormat, rgba: [f32; 4], out: &mut [f32]) -> usize {
    let rgb = [rgba[0], rgba[1], rgba[2]];
    let n = match format.mode {
        ColorMode::Grayscale | ColorMode::Bitmap | ColorMode::Duotone => {
            out[0] = rust_motion_color::convert::rgb_to_gray(rgb);
            1
        }
        ColorMode::Cmyk => {
            out[..4].copy_from_slice(&rust_motion_color::convert::rgb_to_cmyk(rgb));
            4
        }
        ColorMode::Lab => {
            let l = rust_motion_color::convert::srgb_to_lab(rgb);
            out[..3].copy_from_slice(&[
                l[0] / 100.0,
                (l[1] + 128.0) / 255.0,
                (l[2] + 128.0) / 255.0,
            ]);
            3
        }
        _ => {
            out[..3].copy_from_slice(&rgb);
            3
        }
    };
    if format.alpha {
        out[n] = rgba[3];
        n + 1
    } else {
        n
    }
}
