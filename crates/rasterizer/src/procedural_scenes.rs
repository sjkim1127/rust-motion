//! Pure-Rust Procedural Infinite-Resolution Scene-Linear Rec. 2020 HDR Landscape Generator (#129).
//!
//! Evaluates continuous mathematical landscapes at arbitrary resolution (`width x height`)
//! with temporal animation (`time_sec`) and deterministic `seed`:
//! - `AlpineLake`: snow-capped fBm mountain ridges, Rayleigh/Mie atmospheric scattering, and rippled lake reflections
//! - `Dunes`: wind-swept desert sand dunes with analytic surface normals and golden-hour solar lighting
//! - `OceanSunset`: super-bright HDR solar disc ($> 4.0$ scene-linear radiance), atmospheric gradient, and animated specular ocean waves
//! - `Aurora`: night sky starfield with volumetric green/violet aurora borealis curtains in Rec. 2020 linear space

use crate::hdr_merge::HdrRadiance;
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;

/// Procedural natural HDR landscape preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProceduralSceneType {
    #[default]
    AlpineLake,
    Dunes,
    OceanSunset,
    Aurora,
}

#[inline]
fn hash2d(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = seed
        .wrapping_add((ix as u32).wrapping_mul(374761393))
        .wrapping_add((iy as u32).wrapping_mul(668265263));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    (h as f32) * (1.0 / u32::MAX as f32)
}

#[inline]
fn value_noise2d(x: f32, y: f32, seed: u32) -> f32 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);

    let n00 = hash2d(x0, y0, seed);
    let n10 = hash2d(x0 + 1, y0, seed);
    let n01 = hash2d(x0, y0 + 1, seed);
    let n11 = hash2d(x0 + 1, y0 + 1, seed);

    let top = n00 + (n10 - n00) * ux;
    let bot = n01 + (n11 - n01) * ux;
    top + (bot - top) * uy
}

/// Multi-octave Fractional Brownian Motion (fBm) in `[0.0, 1.0]`.
#[inline]
fn fbm2d(mut x: f32, mut y: f32, octaves: usize, seed: u32) -> f32 {
    let mut sum = 0.0f32;
    let mut amp = 0.5f32;
    let mut norm = 0.0f32;
    for i in 0..octaves {
        sum += amp * value_noise2d(x, y, seed.wrapping_add(i as u32 * 1013));
        norm += amp;
        x = x * 2.03 + 13.7;
        y = y * 2.03 + 7.1;
        amp *= 0.5;
    }
    if norm > 0.0 {
        sum / norm
    } else {
        0.0
    }
}

/// Evaluate a continuous procedural scene in scene-linear Rec. 2020 HDR radiance (`HdrRadiance`).
/// Solar discs, specular wave glints, and aurora cores exceed `1.0` (up to `8.0+` scene-linear units).
pub fn render_procedural_hdr_scene(
    scene_type: ProceduralSceneType,
    width: u32,
    height: u32,
    seed: u32,
    time_sec: f32,
) -> HdrRadiance {
    let w = width.max(1);
    let h = height.max(1);
    let mut hdr = HdrRadiance::new(w, h);
    let aspect = w as f32 / h as f32;

    for y in 0..h {
        let v = y as f32 / (h.max(2) - 1) as f32; // 0.0 at top, 1.0 at bottom
        for x in 0..w {
            let u = x as f32 / (w.max(2) - 1) as f32;
            let ux = (u - 0.5) * aspect;

            let rgb_rec2020 = match scene_type {
                ProceduralSceneType::OceanSunset => {
                    let horizon = 0.56;
                    let sun_x = 0.08 * (time_sec * 0.15).sin();
                    let sun_y = horizon - 0.10 - 0.03 * (time_sec * 0.1).cos();

                    if v <= horizon {
                        // Sky + HDR Solar Disc
                        let sky_t = (v / horizon).clamp(0.0, 1.0);
                        let mut r = 0.08 + 1.45 * sky_t.powf(1.5);
                        let mut g = 0.06 + 0.55 * sky_t.powf(1.8);
                        let mut b = 0.28 * (1.0 - sky_t * 0.6) + 0.08;

                        let dx = ux - sun_x;
                        let dy = v - sun_y;
                        let dist2 = dx * dx + dy * dy;
                        // Super-bright HDR sun core (> 6.0 scene-linear radiance) + Mie halo
                        let sun_core = (-dist2 / 0.0012).exp() * 6.5;
                        let sun_halo = (-dist2 / 0.035).exp() * 1.4;
                        r += sun_core * 1.2 + sun_halo * 1.1;
                        g += sun_core * 0.95 + sun_halo * 0.55;
                        b += sun_core * 0.55 + sun_halo * 0.15;
                        [r, g, b]
                    } else {
                        // Animated Ocean Waves + HDR Solar Glitter Path
                        let water_t = ((v - horizon) / (1.0 - horizon)).clamp(0.001, 1.0);
                        let wave_freq = 18.0 / (water_t + 0.08);
                        let wave = fbm2d(
                            ux * 6.0 + time_sec * 0.4,
                            water_t * wave_freq - time_sec * 1.2,
                            4,
                            seed,
                        );
                        let dx = (ux - sun_x) + (wave - 0.5) * 0.12;
                        let glitter = (-dx * dx / (0.004 + water_t * 0.02)).exp()
                            * (wave.powi(3) * 4.5);

                        let base_r = 0.12 * (1.0 - water_t * 0.5) + glitter * 1.3;
                        let base_g = 0.08 * (1.0 - water_t * 0.4) + glitter * 0.75;
                        let base_b = 0.16 * (1.0 - water_t * 0.3) + glitter * 0.25;
                        [base_r, base_g, base_b]
                    }
                }
                ProceduralSceneType::AlpineLake => {
                    let lake_line = 0.62;
                    let sample_y = if v <= lake_line {
                        v
                    } else {
                        // Mirror reflection in the alpine lake with animated water ripples
                        let depth = v - lake_line;
                        let ripple = (depth * 90.0 - time_sec * 3.0).sin() * 0.006 * depth;
                        (lake_line - depth + ripple).clamp(0.0, lake_line)
                    };

                    let ridge_h = 0.32
                        + 0.22
                            * fbm2d(
                                ux * 2.4 + 1.7,
                                0.5,
                                5,
                                seed.wrapping_add(77),
                            );
                    let mut col = if sample_y < ridge_h {
                        // Crisp alpine sky with golden cloud wisps
                        let sky_t = (sample_y / lake_line).clamp(0.0, 1.0);
                        let cloud = fbm2d(ux * 3.0 + time_sec * 0.05, sample_y * 6.0, 4, seed);
                        let c_glow = (cloud - 0.45).max(0.0) * 1.8;
                        [
                            0.12 + 0.55 * sky_t + c_glow * 1.2,
                            0.28 + 0.52 * sky_t + c_glow * 0.9,
                            0.75 + 0.35 * sky_t + c_glow * 0.7,
                        ]
                    } else {
                        // Mountain slope with snowline
                        let elev = (ridge_h - sample_y).abs();
                        let tex = fbm2d(ux * 12.0, sample_y * 12.0, 4, seed ^ 0xA5A5);
                        let snow = ((0.12 - elev) * 10.0 + tex * 0.6).clamp(0.0, 1.0);
                        let rock = [0.09, 0.10, 0.13];
                        let snow_hdr = [1.35, 1.40, 1.55]; // Bright sunlit snow (> 1.0 HDR)
                        [
                            rock[0] * (1.0 - snow) + snow_hdr[0] * snow,
                            rock[1] * (1.0 - snow) + snow_hdr[1] * snow,
                            rock[2] * (1.0 - snow) + snow_hdr[2] * snow,
                        ]
                    };

                    if v > lake_line {
                        // Fresnel darkening & teal glacial water tint on lake reflection
                        col[0] *= 0.68;
                        col[1] = col[1] * 0.75 + 0.03;
                        col[2] = col[2] * 0.82 + 0.05;
                    }
                    col
                }
                ProceduralSceneType::Dunes => {
                    let horizon = 0.42;
                    if v < horizon {
                        let t = (v / horizon).clamp(0.0, 1.0);
                        let sun_glow = (-((ux - 0.25).powi(2) + (v - 0.28).powi(2)) / 0.025).exp();
                        [
                            0.45 + 0.75 * t + sun_glow * 2.8,
                            0.25 + 0.45 * t + sun_glow * 1.6,
                            0.18 + 0.20 * t + sun_glow * 0.6,
                        ]
                    } else {
                        let d_y = (v - horizon) / (1.0 - horizon);
                        let wind = time_sec * 0.12;
                        let h0 = fbm2d(ux * 3.2 + wind, d_y * 4.5, 4, seed);
                        let h_dx = fbm2d(ux * 3.2 + 0.01 + wind, d_y * 4.5, 4, seed);
                        let slope = ((h_dx - h0) * 80.0).clamp(-1.0, 1.0);
                        let lit = (0.55 + 0.45 * slope).clamp(0.12, 1.35);
                        let ripple = ((ux * 45.0 + d_y * 20.0 + h0 * 6.0).sin() * 0.04 + 1.0) * lit;
                        [0.88 * ripple, 0.56 * ripple, 0.24 * ripple]
                    }
                }
                ProceduralSceneType::Aurora => {
                    let horizon = 0.75;
                    if v < horizon {
                        // Deep night sky + stars + HDR Rec. 2020 Aurora curtains
                        let curtain_phase = ux * 4.5
                            + (v * 3.0 + time_sec * 0.4).sin() * 0.6
                            + fbm2d(ux * 3.0, v * 2.0 + time_sec * 0.2, 3, seed) * 1.4;
                        let ribbon = (curtain_phase.sin() * 0.5 + 0.5).powf(4.0);
                        let vertical_env =
                            ((v / horizon) * (1.0 - v / horizon) * 4.0).clamp(0.0, 1.0);
                        let aurora_intensity = ribbon * vertical_env * 3.2; // HDR > 1.0 in core

                        // Oxygen 557.7nm green lower curtain + Nitrogen violet upper fringe
                        let alt_mix = (1.0 - v / horizon).clamp(0.0, 1.0);
                        let ar = aurora_intensity * (0.15 + 0.65 * alt_mix);
                        let ag = aurora_intensity * (1.15 - 0.45 * alt_mix);
                        let ab = aurora_intensity * (0.45 + 0.70 * alt_mix);

                        [0.01 + ar, 0.015 + ag, 0.04 + ab]
                    } else {
                        // Silhouette borealis forest / frozen tundra reflecting aurora
                        let refl = fbm2d(ux * 8.0, (v - horizon) * 12.0, 3, seed);
                        [0.01 + refl * 0.04, 0.04 + refl * 0.14, 0.03 + refl * 0.08]
                    }
                }
            };

            hdr.data[(y * w + x) as usize] = rgb_rec2020;
        }
    }

    hdr
}

/// Render a procedural HDR scene and tonemap it directly to an 8-bit sRGB `Pixmap`.
pub fn render_procedural_scene_pixmap(
    scene_type: ProceduralSceneType,
    width: u32,
    height: u32,
    seed: u32,
    time_sec: f32,
    exposure_bias: f32,
) -> Pixmap {
    let hdr = render_procedural_hdr_scene(scene_type, width, height, seed, time_sec);
    hdr.tonemap("aces", exposure_bias)
}
