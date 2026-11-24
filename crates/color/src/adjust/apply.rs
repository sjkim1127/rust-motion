//! Evaluation entry points: applying an [`Adjustment`] to a buffer, pixel or slice.

use super::*;

pub fn adjustment_quantum(depth: SampleType) -> Option<f32> {
    match depth {
        SampleType::U8 => Some(255.0),
        SampleType::U16 => Some(32768.0),
        SampleType::F32 => None,
    }
}

pub fn apply_to_rgba_slice(adj: &Adjustment, pixels: &mut [[f32; 4]]) {
    let mut buf = Buffer {
        rect: Rect::new(0, 0, pixels.len() as i32, 1),
        px: pixels.to_vec(),
    };
    apply(adj, &mut buf);
    pixels.copy_from_slice(&buf.px);
}

pub fn apply_to_pixel(adj: &Adjustment, rgb: [f32; 3]) -> [f32; 3] {
    let mut buf = Buffer {
        rect: Rect::new(0, 0, 1, 1),
        px: vec![[rgb[0], rgb[1], rgb[2], 1.0]],
    };
    apply(adj, &mut buf);
    [buf.px[0][0], buf.px[0][1], buf.px[0][2]]
}

pub fn apply_to_rgba8(adj: &Adjustment, rgba: &mut [u8]) {
    for chunk in rgba.as_chunks_mut::<4>().0 {
        let r = chunk[0] as f32 / 255.0;
        let g = chunk[1] as f32 / 255.0;
        let b = chunk[2] as f32 / 255.0;
        let [or, og, ob] = apply_to_pixel(adj, [r, g, b]);
        chunk[0] = (or.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        chunk[1] = (og.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        chunk[2] = (ob.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
}

/// Applies an adjustment assuming an sRGB document.
pub fn apply(adj: &Adjustment, buf: &mut Buffer) {
    apply_with(adj, buf, Transfer::Srgb);
}

/// Applies an adjustment with the document's tone transfer.
pub fn apply_with(adj: &Adjustment, buf: &mut Buffer, transfer: Transfer) {
    apply_depth(adj, buf, transfer, None);
}

/// Applies an adjustment with the document's tone transfer, in a document of `depth` (`None`:
/// unrounded). Integer depths work on whole levels like Photoshop's (see [`levels_q`] and
/// `adjustment_quantum`), 32-bit documents get its float Levels (see [`levels_float`]).
pub fn apply_depth(
    adj: &Adjustment,
    buf: &mut Buffer,
    transfer: Transfer,
    depth: Option<SampleType>,
) {
    match adj {
        Adjustment::Invert => map_rgb(buf, |c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]]),
        Adjustment::Threshold { level } => map_rgb(buf, |c| {
            [if threshold_white(c, *level, depth) {
                1.0
            } else {
                0.0
            }; 3]
        }),
        Adjustment::Posterize { levels } => {
            map_rgb(buf, |c| c.map(|v| posterize_in(v, *levels, depth)))
        }
        Adjustment::BrightnessContrast {
            brightness,
            contrast,
            legacy: true,
        } => {
            // Legacy: contrast scales around 128/255, with brightness added before it when
            // contrast is raised and after it when contrast is lowered. Measured on Photoshop
            // 27.11, brightness +40 with contrast +30 maps 128 to 185 (brightness first), while
            // the corpus's brightness -23 with contrast -26 (psd-tools) needs contrast first;
            // contrast 80 keeps 128 at 128 (a 0.5 pivot moved it to 130).
            if depth == Some(SampleType::F32) {
                let f = legacy_brightness_contrast_hdr(*brightness, *contrast);
                map_rgb(buf, |px| px.map(&f));
            } else {
                let f = legacy_brightness_contrast(*brightness, *contrast);
                map_rgb(buf, |px| px.map(&f))
            }
        }
        Adjustment::BrightnessContrast {
            brightness,
            contrast,
            ..
        } => {
            // Modern (CS3+) Brightness/Contrast, reverse-engineered from Photoshop ground truth.
            // Unlike the legacy linear scale, both are smooth curves that pin pure black and white
            // (see `modern_brightness` and `modern_contrast`), applied brightness then contrast
            // through one lookup table.
            let table = modern_lut(*brightness, *contrast);
            map_rgb(buf, |px| px.map(|v| lut(&table, v)))
        }
        Adjustment::Exposure {
            exposure,
            offset,
            gamma,
        } => {
            // In linear light: (lin·2^exposure + offset)^(1/gamma), then back
            // through the document tone curve (fitted on the corpus).
            let m = 2f32.powf(*exposure);
            let g = gamma.max(0.01);
            let transfer = transfer.for_exposure();
            let hdr = depth == Some(SampleType::F32);
            map_rgb(buf, |c| {
                c.map(|v| {
                    if hdr {
                        // Float documents store linear samples. Photoshop's 32-bit merged
                        // Exposure oracle also retains negative results with signed gamma.
                        let lin = v * m + offset;
                        return if g == 1.0 {
                            lin
                        } else {
                            lin.signum() * lin.abs().powf(1.0 / g)
                        };
                    }
                    let lin = (transfer.decode(v) * m + offset).max(0.0);
                    let lin = if g == 1.0 { lin } else { lin.powf(1.0 / g) };
                    transfer.encode(lin).clamp(0.0, 1.0)
                })
            })
        }
        Adjustment::Levels {
            master,
            per_channel,
            space: ToneSpace::Rgb,
            ..
        } if depth == Some(SampleType::F32) => {
            // A 0..1 lookup table cannot represent signed, unbounded float samples.
            let master_identity = *master == LevelsChannel::default();
            let channel_identity = per_channel
                .each_ref()
                .map(|ch| *ch == LevelsChannel::default());
            if !master_identity || channel_identity.iter().any(|&v| !v) {
                map_rgb(buf, |c| {
                    std::array::from_fn(|i| {
                        let v = if channel_identity[i] {
                            c[i]
                        } else {
                            levels_float(&per_channel[i], c[i])
                        };
                        if master_identity {
                            v
                        } else {
                            levels_float(master, v)
                        }
                    })
                });
            }
        }
        Adjustment::Levels { space, .. } | Adjustment::Curves { space, .. } => {
            let luts = tone_luts_depth(adj, depth);
            let identity = float_tone_identity(adj, depth);
            match space {
                ToneSpace::Rgb => map_rgb(buf, |c| {
                    std::array::from_fn(|i| {
                        if identity[i] {
                            c[i]
                        } else {
                            lut(&luts[i], c[i])
                        }
                    })
                }),
                ToneSpace::Cmyk | ToneSpace::Lab => {
                    map_rgb(buf, |c| tone_in_space(*space, &luts, c))
                }
            }
        }
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
            ranges,
        } => {
            let table = (!*colorize && ranges.iter().any(|r| !r.is_neutral()))
                .then(|| hue_range_tables(ranges));
            map_rgb(buf, |c| {
                let (dh, ds, dl) = match &table {
                    Some(t) => {
                        // Range edits follow the pixel's original hue, in full however dull the colour
                        // (Photoshop's merged images, #4089). Greys stay grey: no hue to shift, no
                        // saturation to scale, and range Lightness moves towards a channel they share.
                        let h0 = rgb_to_hsl(c).0;
                        (lut(&t[0], h0), lut(&t[1], h0), lut(&t[2], h0))
                    }
                    None => (0.0, 0.0, 0.0),
                };
                let out = hue_saturation(
                    c,
                    *hue + dh,
                    (*saturation / 100.0 + ds).clamp(-1.0, 1.0),
                    (*lightness / 100.0).clamp(-1.0, 1.0),
                    *colorize,
                );
                range_lightness(out, dl)
            })
        }
        Adjustment::Vibrance {
            vibrance,
            saturation,
        } => {
            let (v, s) = (*vibrance / 100.0, *saturation / 100.0);
            let hi = if depth == Some(SampleType::F32) {
                f32::INFINITY
            } else {
                1.0
            };
            map_rgb(buf, |c| vibrance_px(c, v, s, transfer, hi))
        }
        Adjustment::ChannelMixer { matrix, monochrome } if depth == Some(SampleType::F32) => {
            // 32-bit: each source percentage is cut to 1/1024 (toward zero), the constant counts
            // 255/512 of its value, and nothing clips (photoshop corpus rgb32 channel-mixer.psd:
            // pure red, green, blue, black and white all exact).
            let cut = |v: f32| (v * 1024.0).trunc() / 1024.0;
            let m = matrix.map(|row| {
                [
                    cut(row[0]),
                    cut(row[1]),
                    cut(row[2]),
                    row[3] * 255.0 / 512.0,
                ]
            });
            map_rgb(buf, |c| {
                let mix = |row: &[f32; 4]| row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3];
                if *monochrome {
                    [mix(&m[0]); 3]
                } else {
                    [mix(&m[0]), mix(&m[1]), mix(&m[2])]
                }
            })
        }
        Adjustment::ChannelMixer { matrix, monochrome } => map_rgb(buf, |c| {
            let mix = |row: &[f32; 4]| {
                (row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3]).clamp(0.0, 1.0)
            };
            if *monochrome {
                [mix(&matrix[0]); 3]
            } else {
                [mix(&matrix[0]), mix(&matrix[1]), mix(&matrix[2])]
            }
        }),
        Adjustment::PhotoFilter {
            color,
            density,
            preserve_luminosity,
        } => {
            let linear_doc = depth == Some(SampleType::F32);
            let m = photo_filter_matrix(*color, *density, *preserve_luminosity && linear_doc);
            let set_lum = *preserve_luminosity && !linear_doc;
            map_rgb(buf, |c| {
                // 32-bit samples are linear already, and may go negative.
                let f = if linear_doc {
                    mat3_mul(&m, c)
                } else {
                    mat3_mul(&m, c.map(|v| transfer.decode(v))).map(|v| transfer.encode(v))
                };
                let f = if set_lum {
                    crate::blend::set_lum(f, crate::blend::lum(c))
                } else {
                    f
                };
                // 32-bit results aren't clipped (photoshop corpus rgb32 photo-filter.psd: white
                // filters to 1.19 red).
                if linear_doc {
                    f
                } else {
                    f.map(|v| v.clamp(0.0, 1.0))
                }
            })
        }
        Adjustment::BlackWhite { weights, tint } => {
            let hdr = depth == Some(SampleType::F32);
            map_rgb(buf, |c| {
                let g = black_white_gray(c, weights);
                match tint {
                    // Photoshop tints like the Color blend mode: the tint's hue and saturation at
                    // the grey's luminosity (SetLum + ClipColor), measured in Photoshop 27.11. In
                    // 32-bit documents nothing clips at 1, as with the blend mode there (photoshop
                    // corpus rgb32 black-white-tint.psd: white tints to 1.05, 1.00, 0.87).
                    Some(t) if hdr => crate::blend::blend_rgb_within(
                        crate::blend::BlendMode::Color,
                        [g; 3],
                        *t,
                        f32::MAX,
                    ),
                    Some(t) => crate::blend::set_lum(*t, g).map(|v| v.clamp(0.0, 1.0)),
                    None => [g; 3],
                }
            })
        }
        Adjustment::GradientMap {
            stops,
            reverse,
            dither,
        } => {
            let (w, x0, y0) = (buf.rect.width().max(1) as usize, buf.rect.x0, buf.rect.y0);
            for (i, p) in buf.px.iter_mut().enumerate() {
                if p[3] <= 0.0 {
                    continue;
                }
                // Driven by the W3C luminosity (0.3, 0.59, 0.11), not the Rec. 601 grey: photoshop
                // corpus rgb8 and rgb16 gradient-map.psd match within a level (2 levels off with
                // 0.299, 0.587, 0.114).
                let mut t = crate::blend::lum([p[0], p[1], p[2]]);
                if *reverse {
                    t = 1.0 - t;
                }
                let mut o = gradient(stops, t);
                if *dither {
                    let d = bayer4(x0 + (i % w) as i32, y0 + (i / w) as i32) / 255.0;
                    o = o.map(|v| (v + d).clamp(0.0, 1.0));
                }
                p[..3].copy_from_slice(&o);
            }
        }
        Adjustment::ColorBalance {
            shadows,
            midtones,
            highlights,
            preserve_luminosity,
        } => map_rgb(buf, |c| {
            std::array::from_fn(|ch| {
                color_balance_channel(
                    [*shadows, *midtones, *highlights],
                    *preserve_luminosity,
                    ch,
                    c[ch],
                )
            })
        }),
        Adjustment::SelectiveColor {
            relative,
            adjustments,
        } => map_rgb(buf, |c| selective_color(c, *relative, adjustments)),
        Adjustment::ColorLookup {
            lut: Some(table),
            size,
            tetrahedral,
            dither,
            ..
        } if lut3d_len(*size).is_some_and(|len| table.len() >= len) => {
            let (n, w, x0, y0) = (
                *size as usize,
                buf.rect.width().max(1) as usize,
                buf.rect.x0,
                buf.rect.y0,
            );
            for (i, p) in buf.px.iter_mut().enumerate() {
                if p[3] <= 0.0 {
                    continue;
                }
                let mut o = lut3d_sample(table, n, [p[0], p[1], p[2]], *tetrahedral);
                if *dither {
                    let d = bayer4(x0 + (i % w) as i32, y0 + (i / w) as i32) / 255.0;
                    o = o.map(|v| (v + d).clamp(0.0, 1.0));
                }
                p[..3].copy_from_slice(&o);
            }
        }
        // Not evaluated: identity (still round-trips through PSD).
        Adjustment::ColorLookup { .. } | Adjustment::Unsupported { .. } => {}
    }
}

/// Whether Threshold at `level` turns colour `c` white in a document of `depth` (`None`: unknown,
/// treated as 8-bit). Photoshop splits on the luminosity 0.30 R + 0.59 G + 0.11 B (the blend
/// modes' `lum`), not the Rec. 601 grey, of the document's whole sample values:
/// - 8 bits: white when that luminosity, rounded to a level with halves down, reaches the level;
/// - 16 bits: white when it reaches the level converted to 16 bits (128 → 16448);
/// - 32 bits: white when it reaches the level.
///
/// Exact on every pixel of psd-tools threshold_rgb.psd, threshold_16bits_rgb.psd and
/// threshold_16bits_grayscale.psd and of the photoshop corpus's rgb16 threshold-128.psd, where the
/// Rec. 601 grey split 28 and 100 pixels of the first two the other way. The sums of
/// `30 R + 59 G + 11 B` over whole samples are exact in f32 (at most 3 276 800).
pub fn threshold_white(c: [f32; 3], level: f32, depth: Option<SampleType>) -> bool {
    let t = (level * 255.0).round();
    let sum = |q: f32| {
        let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * q).round());
        30.0 * r + 59.0 * g + 11.0 * b
    };
    match depth {
        None | Some(SampleType::U8) => sum(255.0) > 100.0 * t - 50.0,
        Some(SampleType::U16) => sum(32768.0) >= 100.0 * (t * 32768.0 / 255.0).round(),
        Some(SampleType::F32) => (0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]) * 255.0 >= t,
    }
}

/// Applies an adjustment in a document of colour `mode`: [`apply_depth`], except that in Lab
/// and CMYK documents the adjustments Photoshop evaluates on the document's own channels do so
/// (see [`lab_channel_adjustment`] and [`cmyk_channel_adjustment`]).
pub fn apply_in(
    adj: &Adjustment,
    buf: &mut Buffer,
    transfer: Transfer,
    depth: Option<SampleType>,
    mode: ColorMode,
) {
    if mode == ColorMode::Lab && lab_channel_adjustment(adj) {
        apply_lab(adj, buf, depth);
    } else if mode == ColorMode::Cmyk && cmyk_channel_adjustment(adj) {
        use crate::convert::{cmyk_to_rgb, rgb_to_cmyk};
        if let Adjustment::BrightnessContrast {
            brightness,
            contrast,
            legacy,
        } = adj
        {
            let table = ink_brightness_contrast_lut(*brightness, *contrast, *legacy);
            map_rgb(buf, |c| {
                cmyk_to_rgb(rgb_to_cmyk(c).map(|v| ink_through(&table, v)))
            });
        } else if cmy_adjustment(adj) {
            let mut inks: Vec<[f32; 4]> = buf
                .px
                .iter()
                .map(|p| rgb_to_cmyk([p[0], p[1], p[2]]))
                .collect();
            apply_cmy(adj, &mut inks);
            for (p, ink) in buf.px.iter_mut().zip(&inks) {
                if p[3] > 0.0 {
                    let [r, g, b] = cmyk_to_rgb(*ink);
                    (p[0], p[1], p[2]) = (r, g, b);
                }
            }
        } else {
            map_rgb(buf, |c| {
                let mut ink = rgb_to_cmyk(c);
                stored_channels(adj, mode, depth, &mut ink);
                cmyk_to_rgb(ink)
            });
        }
    } else {
        apply_depth(adj, buf, transfer, depth);
    }
}

pub(super) fn map_rgb(buf: &mut Buffer, f: impl Fn([f32; 3]) -> [f32; 3]) {
    for p in &mut buf.px {
        if p[3] <= 0.0 {
            continue;
        }
        let r = f([p[0], p[1], p[2]]);
        p[0] = r[0];
        p[1] = r[1];
        p[2] = r[2];
    }
}
