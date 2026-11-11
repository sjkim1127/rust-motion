//! TouchDesigner-inspired procedural TOP (Texture Operator) generator for real-time WebAssembly synthesis.

use rust_motion_noise::{fbm_2d, noise2d, noise3d, FbmOptions};
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

/// Procedural Generative Canvas for TouchDesigner-style live visual synthesis.
#[wasm_bindgen]
pub struct GenerativeCanvas {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    width: u32,
    height: u32,
    pixel_buffer: Vec<u8>,
}

#[wasm_bindgen]
impl GenerativeCanvas {
    /// Create a new generative canvas attached to the specified DOM canvas ID.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_id: &str, width: u32, height: u32) -> Result<GenerativeCanvas, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no global `window`"))?;
        let document = window
            .document()
            .ok_or_else(|| JsValue::from_str("no `document` on window"))?;
        let element = document
            .get_element_by_id(canvas_id)
            .ok_or_else(|| JsValue::from_str(&format!("canvas '#{canvas_id}' not found")))?;

        let canvas: HtmlCanvasElement = element
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| JsValue::from_str("element is not an HtmlCanvasElement"))?;

        canvas.set_width(width);
        canvas.set_height(height);

        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("failed to get 2d context"))?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsValue::from_str("failed to cast context"))?;

        let buffer_size = (width * height * 4) as usize;
        let pixel_buffer = vec![0u8; buffer_size];

        Ok(Self {
            canvas,
            ctx,
            width,
            height,
            pixel_buffer,
        })
    }

    /// Resize canvas dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.pixel_buffer.resize((width * height * 4) as usize, 0);
    }

    /// Render a TouchDesigner-inspired Noise TOP in real time.
    ///
    /// Computes procedural multi-octave fBm Simplex noise with animated time offset and color gradient.
    pub fn render_noise(
        &mut self,
        time: f64,
        scale: f64,
        octaves: usize,
        r1: u8,
        g1: u8,
        b1: u8,
        r2: u8,
        g2: u8,
        b2: u8,
    ) -> Result<(), JsValue> {
        let w = self.width;
        let h = self.height;
        let fbm_opts = FbmOptions {
            octaves: octaves.clamp(1, 6),
            lacunarity: 2.0,
            persistence: 0.5,
        };

        let aspect = w as f64 / h.max(1) as f64;
        let s = if scale <= 0.0 { 3.0 } else { scale };

        for y in 0..h {
            let ny = (y as f64 / h as f64) * s;
            for x in 0..w {
                let nx = (x as f64 / w as f64) * s * aspect;

                // 2D fractal noise combined with 3D time evolution
                let n_val = fbm_2d("generative", nx + time * 0.1, ny + time * 0.05, &fbm_opts);
                let t_val = noise3d("generative", nx * 0.5, ny * 0.5, time * 0.2);
                let combined = ((n_val + t_val * 0.5) * 0.5 + 0.5).clamp(0.0, 1.0);

                let r = (r1 as f64 * (1.0 - combined) + r2 as f64 * combined) as u8;
                let g = (g1 as f64 * (1.0 - combined) + g2 as f64 * combined) as u8;
                let b = (b1 as f64 * (1.0 - combined) + b2 as f64 * combined) as u8;

                let idx = ((y * w + x) * 4) as usize;
                if idx + 3 < self.pixel_buffer.len() {
                    self.pixel_buffer[idx] = r;
                    self.pixel_buffer[idx + 1] = g;
                    self.pixel_buffer[idx + 2] = b;
                    self.pixel_buffer[idx + 3] = 255;
                }
            }
        }

        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(&self.pixel_buffer),
            w,
            h,
        )?;
        self.ctx.put_image_data(&image_data, 0.0, 0.0)?;
        Ok(())
    }

    /// Render dynamic audio-reactive spectrum waves.
    pub fn render_audio_wave(
        &mut self,
        time: f64,
        bass: f64,
        mid: f64,
        treble: f64,
    ) -> Result<(), JsValue> {
        let w = self.width;
        let h = self.height;

        // Clear to dark backdrop
        for pixel in self.pixel_buffer.chunks_exact_mut(4) {
            pixel[0] = 10;
            pixel[1] = 12;
            pixel[2] = 20;
            pixel[3] = 255;
        }

        let cy = h as f64 / 2.0;
        let num_waves = 5;

        for wave in 0..num_waves {
            let wave_offset = wave as f64 * 0.8;
            let freq = 0.01 + (wave as f64 * 0.005);
            let amp = (h as f64 * 0.15) * (1.0 + bass * 0.8 + (wave as f64 * treble * 0.2));

            for x in 0..w {
                let x_f = x as f64;
                let y_f = cy
                    + amp
                        * (x_f * freq + time * 2.0 + wave_offset).sin()
                        * (x_f * 0.003 + time).cos()
                    + noise2d("wave", x_f * 0.005, time * 0.5) * mid * 30.0;

                let y_int = y_f.round() as i32;
                for thickness in -2..=2 {
                    let py = y_int + thickness;
                    if py >= 0 && (py as u32) < h {
                        let idx = ((py as u32 * w + x) * 4) as usize;
                        if idx + 3 < self.pixel_buffer.len() {
                            let glow = 1.0 - (thickness.abs() as f64 / 3.0);
                            self.pixel_buffer[idx] =
                                (self.pixel_buffer[idx] as f64 + 40.0 * glow * bass).min(255.0)
                                    as u8;
                            self.pixel_buffer[idx + 1] = (self.pixel_buffer[idx + 1] as f64
                                + 180.0 * glow * (1.0 - wave as f64 * 0.15))
                                .min(255.0)
                                as u8;
                            self.pixel_buffer[idx + 2] = (self.pixel_buffer[idx + 2] as f64
                                + 240.0 * glow * (wave as f64 * 0.2 + 0.5))
                                .min(255.0)
                                as u8;
                        }
                    }
                }
            }
        }

        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(&self.pixel_buffer),
            w,
            h,
        )?;
        self.ctx.put_image_data(&image_data, 0.0, 0.0)?;
        Ok(())
    }
}
