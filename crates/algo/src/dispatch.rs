//! Filter dispatch: the [`Ctx`] a filter runs in and the [`kernel`] that routes a [`FilterParams`] to its implementation.

use super::*;

/// Context a filter runs in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ctx {
    /// Reference area: distortions are centred on it and sample within it
    /// (Photoshop uses the selection bounds, else the canvas).
    pub bounds: Rect,
    /// Colour model of the samples (the last channel is alpha if `alpha`).
    pub mode: ColorMode,
    /// Whether the last channel is alpha.
    pub alpha: bool,
}

/// Runs the filter kernel for `out` from `src` (which covers `out` grown by
/// the halo, or the bounds for global filters). Returns interleaved samples.
pub fn kernel(params: &FilterParams, src: &Image, out: Rect, ctx: &Ctx) -> Vec<f32> {
    match params {
        FilterParams::GaussianBlur { radius } => blur::gaussian(src, out, ctx, *radius),
        FilterParams::BoxBlur { radius } => blur::boxed(src, out, ctx, *radius),
        FilterParams::MotionBlur { angle, distance } => {
            blur::motion(src, out, ctx, *angle, *distance)
        }
        FilterParams::RadialBlur {
            amount,
            method,
            quality,
            center_x,
            center_y,
        } => blur::radial(
            src,
            out,
            ctx,
            *amount,
            *method,
            *quality,
            (*center_x, *center_y),
        ),
        FilterParams::SurfaceBlur { radius, threshold } => {
            blur::surface(src, out, ctx, *radius, *threshold)
        }
        FilterParams::UnsharpMask {
            amount,
            radius,
            threshold,
        } => sharpen::unsharp(src, out, ctx, *amount, *radius, *threshold),
        FilterParams::SmartSharpen {
            amount,
            radius,
            reduce_noise,
        } => sharpen::smart(src, out, ctx, *amount, *radius, *reduce_noise),
        FilterParams::HighPass { radius } => sharpen::high_pass(src, out, ctx, *radius),
        FilterParams::AddNoise {
            amount,
            distribution,
            monochromatic,
            seed,
        } => noise::add(src, out, ctx, *amount, *distribution, *monochromatic, *seed),
        FilterParams::Median { radius } => noise::median(src, out, *radius, None),
        FilterParams::DustAndScratches { radius, threshold } => {
            noise::median(src, out, *radius, Some(*threshold))
        }
        FilterParams::Minimum { radius, preserve } => {
            other::min_max_preserve(src, out, *radius, false, *preserve)
        }
        FilterParams::Maximum { radius, preserve } => {
            other::min_max_preserve(src, out, *radius, true, *preserve)
        }
        FilterParams::Offset {
            horizontal,
            vertical,
            undefined,
        } => other::offset(src, out, ctx, *horizontal, *vertical, *undefined),
        FilterParams::Mosaic { cell_size } => stylize::mosaic(src, out, ctx, *cell_size),
        FilterParams::Emboss {
            angle,
            height,
            amount,
        } => stylize::emboss(src, out, ctx, *angle, *height, *amount),
        FilterParams::FindEdges => stylize::find_edges(src, out, ctx),
        FilterParams::Solarize => stylize::solarize(src, out, ctx),
        FilterParams::Invert => stylize::per_pixel(src, out, ctx, |v| 1.0 - v),
        FilterParams::Desaturate => stylize::desaturate(src, out, ctx),
        FilterParams::Twirl { angle } => distort::twirl(src, out, ctx, *angle),
        FilterParams::Pinch { amount } => distort::pinch(src, out, ctx, *amount),
        FilterParams::Spherize { amount, mode } => distort::spherize(src, out, ctx, *amount, *mode),
        FilterParams::Wave {
            generators,
            wavelength_min,
            wavelength_max,
            amplitude_min,
            amplitude_max,
            wave_type,
            undefined,
            seed,
        } => distort::wave(
            src,
            out,
            ctx,
            distort::WaveSpec {
                generators: *generators,
                wavelength: (*wavelength_min, *wavelength_max),
                amplitude: (*amplitude_min, *amplitude_max),
                wave_type: *wave_type,
                undefined: *undefined,
                seed: *seed,
            },
        ),
        FilterParams::Ripple { amount, size } => distort::ripple(src, out, ctx, *amount, *size),
        FilterParams::PolarCoordinates { mode } => distort::polar(src, out, ctx, *mode),
        FilterParams::ColorHalftone { max_radius, angles } => {
            pixelate::color_halftone(src, out, ctx, *max_radius, *angles)
        }
        FilterParams::Crystallize { cell_size, seed } => {
            pixelate::crystallize(src, out, ctx, *cell_size, *seed)
        }
        FilterParams::Facet => pixelate::facet(src, out, ctx),
        FilterParams::Fragment => pixelate::fragment(src, out, ctx),
        FilterParams::Mezzotint { kind, seed } => pixelate::mezzotint(src, out, ctx, *kind, *seed),
        FilterParams::Pointillize {
            cell_size,
            seed,
            background,
        } => pixelate::pointillize(src, out, ctx, *cell_size, *seed, *background),
        FilterParams::Diffuse { mode, seed } => stylize2::diffuse(src, out, ctx, *mode, *seed),
        FilterParams::Extrude {
            kind,
            size,
            depth,
            level_based,
            solid_front,
            mask_incomplete,
            seed,
        } => stylize2::extrude(
            src,
            out,
            ctx,
            &stylize2::ExtrudeSpec {
                kind: *kind,
                size: *size,
                depth: *depth,
                level_based: *level_based,
                solid_front: *solid_front,
                mask_incomplete: *mask_incomplete,
                seed: *seed,
            },
        ),
        FilterParams::OilPaint {
            stylization,
            cleanliness,
            scale,
            bristle_detail,
            lighting,
            angle,
            shine,
        } => oil::oil_paint(
            src,
            out,
            ctx,
            &oil::OilSpec {
                stylization: *stylization,
                cleanliness: *cleanliness,
                scale: *scale,
                bristle_detail: *bristle_detail,
                lighting: *lighting,
                angle: *angle,
                shine: *shine,
            },
        ),
        FilterParams::Tiles {
            count,
            max_offset,
            fill,
            foreground,
            background,
            seed,
        } => stylize2::tiles(
            src,
            out,
            ctx,
            *count,
            *max_offset,
            *fill,
            (*foreground, *background),
            *seed,
        ),
        FilterParams::TraceContour { level, upper } => {
            stylize2::trace_contour(src, out, ctx, *level, *upper)
        }
        FilterParams::Wind {
            method,
            from_right,
            seed,
        } => stylize2::wind(src, out, ctx, *method, *from_right, *seed),
        FilterParams::Displace {
            horizontal,
            vertical,
            stretch,
            undefined,
            map,
        } => distort2::displace(
            src,
            out,
            ctx,
            (*horizontal, *vertical),
            *stretch,
            *undefined,
            map.as_deref(),
        ),
        FilterParams::Shear { points, undefined } => {
            distort2::shear(src, out, ctx, points, *undefined)
        }
        FilterParams::ZigZag {
            amount,
            ridges,
            style,
        } => distort2::zigzag(src, out, ctx, *amount, *ridges, *style),
        FilterParams::Fibers {
            variance,
            strength,
            seed,
            foreground,
            background,
        } => render::fibers(
            src,
            out,
            ctx,
            *variance,
            *strength,
            *seed,
            (*foreground, *background),
        ),
        FilterParams::LensFlare {
            brightness,
            center_x,
            center_y,
            lens,
        } => render::lens_flare(src, out, ctx, *brightness, (*center_x, *center_y), *lens),
        FilterParams::LightingEffects {
            lights,
            gloss,
            metallic,
            exposure,
            ambience,
            texture,
            height,
            white_is_high,
        } => render::lighting(
            src,
            out,
            ctx,
            &render::LightingSpec {
                lights,
                gloss: *gloss,
                metallic: *metallic,
                exposure: *exposure,
                ambience: *ambience,
                texture: *texture,
                height: *height,
                white_is_high: *white_is_high,
            },
        ),
        FilterParams::Relight {
            angle,
            elevation,
            intensity,
            ambient,
            warmth,
            softness,
        } => relight::relight(
            src,
            out,
            ctx,
            relight::Params {
                angle: *angle,
                elevation: *elevation,
                intensity: *intensity,
                ambient: *ambient,
                warmth: *warmth,
                softness: *softness,
            },
        ),
        FilterParams::ReduceNoise {
            strength,
            preserve_details,
            reduce_color_noise,
            sharpen_details,
            remove_jpeg_artifact,
        } => denoise::reduce_noise(
            src,
            out,
            ctx,
            &denoise::DenoiseSpec {
                strength: *strength,
                preserve: *preserve_details,
                color: *reduce_color_noise,
                sharpen: *sharpen_details,
                jpeg: *remove_jpeg_artifact,
            },
        ),
        FilterParams::SmartBlur {
            radius,
            threshold,
            quality,
            mode,
        } => blur2::smart_blur(src, out, ctx, *radius, *threshold, *quality, *mode),
        FilterParams::LensBlur {
            radius,
            blades,
            curvature,
            rotation,
            depth,
            focal_distance,
            invert_depth,
            brightness,
            threshold,
            noise,
            distribution,
            monochromatic,
            seed,
            depth_map,
        } => blur2::lens_blur(
            src,
            out,
            ctx,
            &blur2::LensSpec {
                radius: *radius,
                blades: *blades,
                curvature: *curvature,
                rotation: *rotation,
                depth: *depth,
                focal: *focal_distance,
                invert: *invert_depth,
                brightness: *brightness,
                threshold: *threshold,
                noise: *noise,
                distribution: *distribution,
                mono: *monochromatic,
                seed: *seed,
                depth_map: depth_map.as_deref(),
            },
        ),
        FilterParams::ShapeBlur { radius, shape } => {
            blur2::shape_blur(src, out, ctx, *radius, *shape)
        }
        FilterParams::TiltShift {
            blur,
            center_x,
            center_y,
            angle,
            focus,
            transition,
        } => gallery::tilt_shift(
            src,
            out,
            ctx,
            *blur,
            (*center_x, *center_y),
            *angle,
            *focus,
            *transition,
        ),
        FilterParams::IrisBlur { pins } => gallery::iris(src, out, ctx, pins),
        FilterParams::FieldBlur { pins } => gallery::field(src, out, ctx, pins),
        FilterParams::SpinBlur { pins } => gallery::spin(src, out, ctx, pins),
        FilterParams::PathBlur { paths } => gallery::path(src, out, ctx, paths),
        FilterParams::Custom {
            kernel,
            scale,
            offset,
        } => other2::custom(src, out, ctx, kernel, *scale, *offset),
        FilterParams::HsbHsl { input, output } => other2::hsb_hsl(src, out, ctx, *input, *output),
        FilterParams::ColorToAlpha {
            color,
            transparency_threshold,
            opacity_threshold,
        } => color_to_alpha::color_to_alpha(
            src,
            out,
            ctx,
            *color,
            *transparency_threshold,
            *opacity_threshold,
        ),
        FilterParams::DeInterlace {
            eliminate_even,
            interpolate,
        } => video::deinterlace(src, out, ctx, *eliminate_even, *interpolate),
        FilterParams::NtscColors => video::ntsc(src, out, ctx),
        FilterParams::FilterGallery { effects } => artistic_fx::run(effects, src, out, ctx),
    }
}
