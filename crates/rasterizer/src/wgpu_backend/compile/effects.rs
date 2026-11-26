//! Layer effect lowering: tint, duotone, colour grading and vignette.

pub(crate) fn gpu_layer_effects(
    filters: &[crate::scene::SceneFilter],
    layer_opacity: f32,
) -> Option<(f32, f32, f32, f32, f32, f32, f32)> {
    if !layer_opacity.is_finite() {
        return None;
    }
    if filters
        .iter()
        .any(|filter| matches!(filter, crate::scene::SceneFilter::ColorGrading { .. }))
        && filters.iter().any(|filter| {
            matches!(
                filter,
                crate::scene::SceneFilter::Brightness { .. }
                    | crate::scene::SceneFilter::Grayscale { .. }
                    | crate::scene::SceneFilter::Contrast { .. }
                    | crate::scene::SceneFilter::Saturation { .. }
                    | crate::scene::SceneFilter::Invert { .. }
                    | crate::scene::SceneFilter::HueRotate { .. }
                    | crate::scene::SceneFilter::Tint { .. }
                    | crate::scene::SceneFilter::Duotone { .. }
            )
        })
    {
        // ColorGrading has its own gamma/contrast/saturation/tint order in
        // TinySkia. Do not claim GPU support for a chain whose CPU order
        // cannot be represented by the packed instance fields.
        return None;
    }
    filters.iter().try_fold(
        (layer_opacity, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0),
        |(opacity, brightness, grayscale, contrast, saturation, invert, hue), filter| match filter {
            crate::scene::SceneFilter::Opacity { amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                Some((
                    opacity * amount,
                    brightness,
                    grayscale,
                    contrast,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Brightness { amount }
                if amount.is_finite() && (0.0..=10.0).contains(amount) =>
            {
                Some((
                    opacity,
                    brightness * amount,
                    grayscale,
                    contrast,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Grayscale { amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                Some((
                    opacity,
                    brightness,
                    grayscale + amount - grayscale * amount,
                    contrast,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Contrast { factor }
                if factor.is_finite() && *factor >= 0.0 =>
            {
                Some((
                    opacity,
                    brightness,
                    grayscale,
                    contrast * factor,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Saturation { factor }
                if factor.is_finite() && *factor >= 0.0 =>
            {
                Some((
                    opacity,
                    brightness,
                    grayscale,
                    contrast,
                    saturation * factor,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Vignette {
                offset,
                darkness,
                roundness,
            } if offset.is_finite()
                && darkness.is_finite()
                && roundness.is_finite()
                && (0.0..=2.0).contains(offset)
                && (0.0..=1.0).contains(darkness)
                && (0.0..=1.0).contains(roundness) =>
            {
                Some((
                    opacity, brightness, grayscale, contrast, saturation, invert, hue,
                ))
            }
            crate::scene::SceneFilter::Invert { amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                // Applying two invert filters sequentially composes their
                // affine color transforms: I_b(I_a(x)) has amount
                // a + b - 2ab. Preserve that composition instead of
                // silently dropping every preceding invert filter.
                Some((
                    opacity,
                    brightness,
                    grayscale,
                    contrast,
                    saturation,
                    invert + *amount - 2.0 * invert * *amount,
                    hue,
                ))
            }
            crate::scene::SceneFilter::HueRotate { degrees } if degrees.is_finite() => Some((
                opacity,
                brightness,
                grayscale,
                contrast,
                saturation,
                invert,
                hue + *degrees,
            )),
            crate::scene::SceneFilter::Tint { color, amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                Some((
                    opacity, brightness, grayscale, contrast, saturation, invert, hue,
                ))
            }
            crate::scene::SceneFilter::Duotone { .. } => Some((
                opacity, brightness, grayscale, contrast, saturation, invert, hue,
            )),
            crate::scene::SceneFilter::ColorGrading {
                contrast: grading_contrast,
                saturation: grading_saturation,
                gamma,
                ..
            } if grading_contrast.is_finite()
                && *grading_contrast >= 0.0
                && grading_saturation.is_finite()
                && *grading_saturation >= 0.0
                && gamma.is_finite()
                && *gamma > 0.0 =>
            {
                Some((
                    opacity, brightness, grayscale, contrast, saturation, invert, hue,
                ))
            }
            _ => None,
        },
    )
}

pub(super) fn gpu_tint(filters: &[crate::scene::SceneFilter]) -> Option<[f32; 4]> {
    let mut tint = None;
    for filter in filters {
        if let crate::scene::SceneFilter::Tint { color, amount } = filter {
            if !amount.is_finite() || !(0.0..=1.0).contains(amount) {
                return None;
            }
            tint = Some([
                f32::from(color[0]) / 255.0,
                f32::from(color[1]) / 255.0,
                f32::from(color[2]) / 255.0,
                f32::from(color[3]) / 255.0 * *amount,
            ]);
        }
    }
    tint
}

pub(super) fn gpu_duotone(filters: &[crate::scene::SceneFilter]) -> Option<([f32; 4], [f32; 4])> {
    filters.iter().find_map(|filter| {
        if let crate::scene::SceneFilter::Duotone { primary, secondary } = filter {
            Some((
                [
                    f32::from(primary[0]) / 255.0,
                    f32::from(primary[1]) / 255.0,
                    f32::from(primary[2]) / 255.0,
                    1.0,
                ],
                [
                    f32::from(secondary[0]) / 255.0,
                    f32::from(secondary[1]) / 255.0,
                    f32::from(secondary[2]) / 255.0,
                    0.0,
                ],
            ))
        } else {
            None
        }
    })
}

pub(super) fn gpu_color_grading(
    filters: &[crate::scene::SceneFilter],
) -> Option<([f32; 4], [f32; 4])> {
    filters.iter().find_map(|filter| {
        if let crate::scene::SceneFilter::ColorGrading {
            contrast,
            saturation,
            gamma,
            tint,
        } = filter
        {
            if !contrast.is_finite()
                || *contrast < 0.0
                || !saturation.is_finite()
                || *saturation < 0.0
                || !gamma.is_finite()
                || *gamma <= 0.0
            {
                return None;
            }
            let tint = tint.map_or([0.0; 4], |color| {
                [
                    f32::from(color[0]) / 255.0,
                    f32::from(color[1]) / 255.0,
                    f32::from(color[2]) / 255.0,
                    f32::from(color[3]) / 255.0,
                ]
            });
            Some(([*contrast, *saturation, 1.0 / *gamma, 1.0], tint))
        } else {
            None
        }
    })
}

pub(super) fn gpu_vignette(filters: &[crate::scene::SceneFilter]) -> Option<[f32; 4]> {
    let mut result = None;
    for filter in filters {
        if let crate::scene::SceneFilter::Vignette {
            offset,
            darkness,
            roundness,
        } = filter
        {
            if !offset.is_finite()
                || !darkness.is_finite()
                || !roundness.is_finite()
                || !(0.0..=2.0).contains(offset)
                || !(0.0..=1.0).contains(darkness)
                || !(0.0..=1.0).contains(roundness)
            {
                return None;
            }
            result = Some([*offset, *darkness, *roundness, 1.0]);
        }
    }
    result
}
