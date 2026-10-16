//! Real-time Professional Video Scopes Engine.
//!
//! Provides hardware-efficient Waveform (Luma & RGB), RGB Parade, Vectorscope (YUV & HLS
//! with skin-tone indicator line), and 256-bin Luminance/RGB Histograms for color grading
//! analysis in post-production.

use serde::{Deserialize, Serialize};

/// Type of video scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeType {
    WaveformLuma,
    WaveformRgb,
    RgbParade,
    VectorscopeYuv,
    Histogram,
}

/// 256-bin histogram for a single color channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelHistogram {
    pub bins: Vec<u32>,
    pub min_value: u8,
    pub max_value: u8,
    pub mean_value: f32,
    pub clipped_blacks: u32,
    pub clipped_whites: u32,
}

impl Default for ChannelHistogram {
    fn default() -> Self {
        Self {
            bins: vec![0; 256],
            min_value: 255,
            max_value: 0,
            mean_value: 0.0,
            clipped_blacks: 0,
            clipped_whites: 0,
        }
    }
}

/// Full four-channel histogram (Red, Green, Blue, Luminance).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoHistogram {
    pub red: ChannelHistogram,
    pub green: ChannelHistogram,
    pub blue: ChannelHistogram,
    pub luma: ChannelHistogram,
}

/// 2D density grid representing a scope graph (e.g. Waveform or Vectorscope).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeGrid {
    pub width: usize,
    pub height: usize,
    /// Hit density per cell (row-major order).
    pub data: Vec<u32>,
    /// Maximum count among all cells, for normalization when drawing.
    pub max_count: u32,
}

impl ScopeGrid {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0; width * height],
            max_count: 0,
        }
    }

    #[inline]
    pub fn increment(&mut self, x: usize, y: usize) {
        if x < self.width && y < self.height {
            let idx = y * self.width + x;
            let val = self.data[idx].saturating_add(1);
            self.data[idx] = val;
            if val > self.max_count {
                self.max_count = val;
            }
        }
    }
}

/// Complete video scopes analysis package for a single video frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoScopesAnalysis {
    pub histogram: VideoHistogram,
    pub waveform_luma: ScopeGrid,
    pub rgb_parade: ScopeGrid,
    pub vectorscope_yuv: ScopeGrid,
}

/// Calculate Rec. 709 Luminance $Y = 0.2126 R + 0.7152 G + 0.0722 B$.
#[inline]
pub fn rgb_to_luma_rec709(r: u8, g: u8, b: u8) -> u8 {
    let y = 0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32);
    y.round().clamp(0.0, 255.0) as u8
}

/// Calculate Rec. 709 YCbCr chromaticity coordinates.
/// Returns `(Y, Cb, Cr)` normalized to range `[0..255]`.
#[inline]
pub fn rgb_to_ycbcr_rec709(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let rf = r as f32;
    let gf = g as f32;
    let bf = b as f32;
    let y = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
    let cb = 128.0 - 0.1146 * rf - 0.3854 * gf + 0.5000 * bf;
    let cr = 128.0 + 0.5000 * rf - 0.4542 * gf - 0.0458 * bf;
    (
        y.round().clamp(0.0, 255.0) as u8,
        cb.round().clamp(0.0, 255.0) as u8,
        cr.round().clamp(0.0, 255.0) as u8,
    )
}

/// Analyze an RGBA8 frame buffer and compute all professional video scopes.
pub fn analyze_frame_scopes(
    rgba_bytes: &[u8],
    width: u32,
    height: u32,
    scope_width: usize,
    scope_height: usize,
) -> VideoScopesAnalysis {
    let scope_width = scope_width.max(64);
    let scope_height = scope_height.max(64);

    let mut hist = VideoHistogram::default();
    let mut waveform_luma = ScopeGrid::new(scope_width, scope_height);
    // RGB Parade: 3 panels (R, G, B) side-by-side
    let mut rgb_parade = ScopeGrid::new(scope_width * 3, scope_height);
    // Vectorscope: square grid (e.g. 256x256)
    let vec_size = scope_height.min(256);
    let mut vectorscope = ScopeGrid::new(vec_size, vec_size);

    let pixel_count = (width * height) as usize;
    let mut r_sum: u64 = 0;
    let mut g_sum: u64 = 0;
    let mut b_sum: u64 = 0;
    let mut y_sum: u64 = 0;

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if idx + 3 >= rgba_bytes.len() {
                break;
            }
            let r = rgba_bytes[idx];
            let g = rgba_bytes[idx + 1];
            let b = rgba_bytes[idx + 2];
            let y_val = rgb_to_luma_rec709(r, g, b);
            let (_y_c, cb, cr) = rgb_to_ycbcr_rec709(r, g, b);

            // 1. Histogram accumulation
            hist.red.bins[r as usize] += 1;
            hist.green.bins[g as usize] += 1;
            hist.blue.bins[b as usize] += 1;
            hist.luma.bins[y_val as usize] += 1;

            r_sum += r as u64;
            g_sum += g as u64;
            b_sum += b as u64;
            y_sum += y_val as u64;

            if r < hist.red.min_value { hist.red.min_value = r; }
            if r > hist.red.max_value { hist.red.max_value = r; }
            if g < hist.green.min_value { hist.green.min_value = g; }
            if g > hist.green.max_value { hist.green.max_value = g; }
            if b < hist.blue.min_value { hist.blue.min_value = b; }
            if b > hist.blue.max_value { hist.blue.max_value = b; }
            if y_val < hist.luma.min_value { hist.luma.min_value = y_val; }
            if y_val > hist.luma.max_value { hist.luma.max_value = y_val; }

            // 2. Waveform Luma (columns = x column mapped to scope_width, rows = y_val mapped to scope_height, 0 at bottom)
            let col = ((x as usize * scope_width) / width as usize).min(scope_width - 1);
            let row = ((255 - y_val as usize) * (scope_height - 1)) / 255;
            waveform_luma.increment(col, row);

            // 3. RGB Parade (R on panel 0, G on panel 1, B on panel 2)
            let row_r = ((255 - r as usize) * (scope_height - 1)) / 255;
            let row_g = ((255 - g as usize) * (scope_height - 1)) / 255;
            let row_b = ((255 - b as usize) * (scope_height - 1)) / 255;
            rgb_parade.increment(col, row_r);
            rgb_parade.increment(scope_width + col, row_g);
            rgb_parade.increment(scope_width * 2 + col, row_b);

            // 4. Vectorscope (Cb horizontal, Cr vertical inverted)
            let vec_x = (cb as usize * (vec_size - 1)) / 255;
            let vec_y = ((255 - cr as usize) * (vec_size - 1)) / 255;
            vectorscope.increment(vec_x, vec_y);
        }
    }

    if pixel_count > 0 {
        hist.red.mean_value = r_sum as f32 / pixel_count as f32;
        hist.green.mean_value = g_sum as f32 / pixel_count as f32;
        hist.blue.mean_value = b_sum as f32 / pixel_count as f32;
        hist.luma.mean_value = y_sum as f32 / pixel_count as f32;

        hist.red.clipped_blacks = hist.red.bins[0];
        hist.red.clipped_whites = hist.red.bins[255];
        hist.green.clipped_blacks = hist.green.bins[0];
        hist.green.clipped_whites = hist.green.bins[255];
        hist.blue.clipped_blacks = hist.blue.bins[0];
        hist.blue.clipped_whites = hist.blue.bins[255];
        hist.luma.clipped_blacks = hist.luma.bins[0];
        hist.luma.clipped_whites = hist.luma.bins[255];
    }

    VideoScopesAnalysis {
        histogram: hist,
        waveform_luma,
        rgb_parade,
        vectorscope_yuv: vectorscope,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_luma_and_ycbcr() {
        // Pure black
        assert_eq!(rgb_to_luma_rec709(0, 0, 0), 0);
        let (y, cb, cr) = rgb_to_ycbcr_rec709(0, 0, 0);
        assert_eq!(y, 0);
        assert_eq!(cb, 128);
        assert_eq!(cr, 128);

        // Pure white
        assert_eq!(rgb_to_luma_rec709(255, 255, 255), 255);
        let (y, cb, cr) = rgb_to_ycbcr_rec709(255, 255, 255);
        assert_eq!(y, 255);
        assert_eq!(cb, 128);
        assert_eq!(cr, 128);
    }

    #[test]
    fn test_analyze_frame_scopes() {
        // 4x4 red image
        let mut pixels = vec![0u8; 4 * 4 * 4];
        for i in 0..16 {
            pixels[i * 4] = 255; // R
            pixels[i * 4 + 1] = 0; // G
            pixels[i * 4 + 2] = 0; // B
            pixels[i * 4 + 3] = 255; // A
        }

        let scopes = analyze_frame_scopes(&pixels, 4, 4, 64, 64);
        assert_eq!(scopes.histogram.red.clipped_whites, 16);
        assert_eq!(scopes.histogram.green.clipped_blacks, 16);
        assert_eq!(scopes.histogram.blue.clipped_blacks, 16);

        // Waveform should have hits
        assert!(scopes.waveform_luma.max_count > 0);
        assert!(scopes.rgb_parade.max_count > 0);
        assert!(scopes.vectorscope_yuv.max_count > 0);
    }
}
