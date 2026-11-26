//! Tiled application of a filter to a surface: tile sizing, cancellation, progress and selection mixing per tile.

use super::*;

/// Output tile size used by [`apply`].
pub const TILE: i32 = 256;

/// The area a filter writes for a layer whose pixels cover `content`:
/// global filters and renders that ignore the layer's pixels (Fibers) write the reference
/// bounds; others the content grown by the halo (blurs spread into transparent areas).
/// Clipped to the selection's bounds when there is one.
pub fn output_area(
    params: &FilterParams,
    content: Rect,
    bounds: Rect,
    selection_bounds: Option<Rect>,
) -> Rect {
    let mut area = match params.halo_for(bounds) {
        Halo::Bounds => bounds,
        Halo::Radius(_) if params.renders_from_nothing() => bounds,
        Halo::Radius(r) => {
            if content.is_empty() {
                Rect::EMPTY
            } else {
                content.inflate(r)
            }
        }
    };
    if let Some(sb) = selection_bounds {
        area = area.intersect(&sb);
    }
    area
}

/// Applies a filter to `area` of `surface`, mixing with the original by the
/// selection coverage (channel 0 of `selection`). Returns the new surface.
pub fn apply(
    surface: &Surface,
    params: &FilterParams,
    area: Rect,
    bounds: Rect,
    selection: Option<&Surface>,
) -> Surface {
    apply_tiled(
        surface,
        params,
        area,
        bounds,
        selection,
        auto_tile(params, bounds),
        None,
    )
}

/// [`apply`] for a layer in a document: neighbourhood filters repeat the edge pixels of `extent`
/// (the canvas plus any off-canvas pixels the layer has) instead of reading transparency beyond
/// it, and the output is clipped to `extent`, as Photoshop does at the canvas edge.
pub fn apply_in(
    surface: &Surface,
    params: &FilterParams,
    area: Rect,
    bounds: Rect,
    selection: Option<&Surface>,
    extent: Rect,
) -> Surface {
    apply_tiled(
        surface,
        params,
        area.intersect(&extent),
        bounds,
        selection,
        auto_tile(params, bounds),
        Some(extent),
    )
}

/// [`apply_in`] that can be cancelled (checked before each tile) and reports progress per tile
/// group. `None` when cancelled, or when a checked filter rejects geometry/allocation;
/// the input surface is never modified.
pub fn apply_in_with(
    surface: &Surface,
    params: &FilterParams,
    area: Rect,
    bounds: Rect,
    selection: Option<&Surface>,
    extent: Rect,
    ctl: &rust_motion_raster::Interrupt,
) -> Option<Surface> {
    apply_tiled_with(
        surface,
        params,
        area.intersect(&extent),
        bounds,
        selection,
        auto_tile(params, bounds),
        Some(extent),
        ctl,
    )
}

/// Results do not depend on the tiling, so wide-halo filters use bigger tiles to keep the
/// re-read margin (and its cost) below ~2× the tile area.
pub(crate) fn auto_tile(params: &FilterParams, bounds: Rect) -> i32 {
    match params.halo_for(bounds) {
        Halo::Radius(r) => TILE.max((2 * r + 63) / 64 * 64).min(2048),
        Halo::Bounds => TILE,
    }
}

/// [`apply`] / [`apply_in`] with an explicit tile size (tests use it to check tile independence).
pub fn apply_tiled(
    surface: &Surface,
    params: &FilterParams,
    area: Rect,
    bounds: Rect,
    selection: Option<&Surface>,
    tile: i32,
    extent: Option<Rect>,
) -> Surface {
    // Preserve the input if a checked filter allocation cannot be completed.
    apply_tiled_with(
        surface,
        params,
        area,
        bounds,
        selection,
        tile,
        extent,
        &rust_motion_raster::Interrupt::NONE,
    )
    .unwrap_or_else(|| surface.clone())
}

/// [`apply_tiled`] with cancellation (checked before each tile, so a cancel takes effect within
/// one tile's work) and progress (after each group of tiles). `None` when cancelled,
/// or when a checked filter rejects geometry or cannot allocate a tile.
#[allow(clippy::too_many_arguments)]
pub fn apply_tiled_with(
    surface: &Surface,
    params: &FilterParams,
    area: Rect,
    bounds: Rect,
    selection: Option<&Surface>,
    tile: i32,
    extent: Option<Rect>,
    ctl: &rust_motion_raster::Interrupt,
) -> Option<Surface> {
    apply_tiled_with_impl(
        surface, params, area, bounds, selection, tile, extent, ctl, true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_tiled_with_impl(
    surface: &Surface,
    params: &FilterParams,
    area: Rect,
    bounds: Rect,
    selection: Option<&Surface>,
    tile: i32,
    extent: Option<Rect>,
    ctl: &rust_motion_raster::Interrupt,
    prepare_fast_paths: bool,
) -> Option<Surface> {
    if ctl.cancelled() {
        return None;
    }
    if let FilterParams::UnsharpMask {
        amount,
        radius,
        threshold,
    } = *params
    {
        if !sharpen::exact::valid_params(amount, radius, threshold)
            || !sharpen::exact::valid_area(area)
            || !sharpen::exact::valid_area(bounds)
            || extent.is_some_and(|e| !sharpen::exact::valid_area(e))
        {
            return None;
        }
    }
    let mut out = surface.clone();
    if area.is_empty() {
        return Some(out);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if prepare_fast_paths {
        if let Some(result) = sharpen::fft::apply(surface, params, area, selection, extent, ctl) {
            return result;
        }
    }
    if let Some(boxes) = blur::box_widths(params) {
        return apply_box_blur(out, surface, area, extent, selection, &boxes, ctl);
    }
    if let Some(result) =
        blur::motion_apply::apply(surface, params, area, bounds, selection, extent, tile, ctl)
    {
        return result;
    }
    // The public tile override must not create a zero-step loop or an
    // unbounded scratch buffer for these checked filters. Automatic tiles are unchanged.
    let tile = if matches!(
        params,
        FilterParams::ShapeBlur { .. } | FilterParams::UnsharpMask { .. }
    ) {
        tile.clamp(1, 2048)
    } else {
        tile
    };
    let fmt = surface.format();
    let ctx = Ctx {
        bounds,
        mode: fmt.mode,
        alpha: fmt.alpha,
    };
    // Prepare the immutable sample transforms once for all output tiles. If the
    // optional table allocation fails, use the existing sampler; only a real
    // cancellation returns None. Source reads keep their upstream semantics.
    let radial = match params {
        FilterParams::RadialBlur {
            amount,
            method,
            quality,
            center_x,
            center_y,
        } if prepare_fast_paths => blur::radial::Plan::for_area(
            bounds,
            *amount,
            *method,
            *quality,
            (*center_x, *center_y),
            area,
            ctl,
        ),
        _ => None,
    };
    if ctl.cancelled() {
        return None;
    }
    let halo = params.halo_for(bounds);
    if matches!(params, FilterParams::UnsharpMask { .. }) {
        let Halo::Radius(reach) = halo else {
            return None;
        };
        let (w, h) = (
            area.width().min(tile as u32) as usize,
            area.height().min(tile as u32) as usize,
        );
        let read_samples = w
            .checked_add(2 * reach as usize)?
            .checked_mul(h.checked_add(2 * reach as usize)?)?
            .checked_mul(fmt.channels())?;
        let tile_count = u64::from(area.width())
            .div_ceil(tile as u64)
            .checked_mul(u64::from(area.height()).div_ceil(tile as u64))?;
        // Reject hostile source-window sizes before Image::read allocates them.
        // Ordinary filter parameters and automatic tiles fit these bounds.
        if read_samples > 512 * 1024 * 1024
            || read_samples.checked_mul(std::mem::size_of::<f32>())? > isize::MAX as usize
            || tile_count > 1_048_576
        {
            return None;
        }
    }
    let shared = (halo == Halo::Bounds).then(|| Image::read(surface, bounds.union(&area)));
    // Shape rasterization depends only on params, not on a tile's pixels.
    let shape = match params {
        FilterParams::ShapeBlur { radius, shape } => {
            if ctl.cancelled() {
                return None;
            }
            Some(shape_blur::Prepared::new(*radius, *shape))
        }
        _ => None,
    };
    let mut tiles = Vec::new();
    let mut y = area.y0;
    while y < area.y1 {
        let mut x = area.x0;
        while x < area.x1 {
            tiles.push(Rect::new(
                x,
                y,
                x.saturating_add(tile).min(area.x1),
                y.saturating_add(tile).min(area.y1),
            ));
            x = x.saturating_add(tile);
        }
        y = y.saturating_add(tile);
    }
    // Tiles finished so far, for progress reported per tile (from any worker thread).
    let finished = std::sync::atomic::AtomicUsize::new(0);
    let total = tiles.len().max(1);
    let run = |t: &Rect| -> (Rect, Vec<f32>) {
        // Cancelled: skip the remaining tiles of the group (the result is discarded).
        if ctl.cancelled() {
            return (*t, Vec::new());
        }
        let tick = || {
            let n = finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            // The last few percent are the writes after each group.
            ctl.progress(0.98 * n as f32 / total as f32);
        };
        let owned;
        let src = match (&shared, halo) {
            (Some(s), _) => s,
            (None, Halo::Radius(r)) => {
                owned = match extent {
                    Some(e) => Image::read_clamped(surface, t.inflate(r), e),
                    None => Image::read(surface, t.inflate(r)),
                };
                &owned
            }
            // `shared` is always read for global filters; read the same image if it weren't.
            (None, Halo::Bounds) => {
                owned = Image::read(surface, bounds.union(&area));
                &owned
            }
        };
        let Some(mut data) = (match (&shape, &radial) {
            (Some(shape), _) => shape.checked_run(src, *t, &ctx),
            (None, Some(plan)) => match plan.filter(src, *t, &ctx, ctl) {
                Some(data) => Some(data),
                None if ctl.cancelled() => None,
                None => Some(kernel(params, src, *t, &ctx)),
            },
            (None, None) => match *params {
                FilterParams::UnsharpMask {
                    amount,
                    radius,
                    threshold,
                } => sharpen::exact::run(src, *t, &ctx, amount, radius, threshold, ctl),
                _ => Some(kernel(params, src, *t, &ctx)),
            },
        }) else {
            return (*t, Vec::new());
        };
        if let Some(sel) = selection {
            mix_selection(&mut data, *t, sel, src, fmt.alpha);
        }
        tick();
        (*t, data)
    };
    // Tiles are filtered in groups of about RESULT_BUDGET bytes (at least one per core) and each
    // group is written before the next starts, so a huge layer never holds all its results as
    // floats at once.
    #[cfg(not(target_arch = "wasm32"))]
    let threads = rayon::current_num_threads();
    #[cfg(target_arch = "wasm32")]
    let threads = 1;
    let per_tile = (tile.max(1) as usize).pow(2) * fmt.channels() * std::mem::size_of::<f32>();
    let group = (RESULT_BUDGET / per_tile.max(1)).max(threads).max(1);
    for chunk in tiles.chunks(group) {
        if ctl.cancelled() {
            return None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let results: Vec<(Rect, Vec<f32>)> = {
            use rayon::prelude::*;
            chunk.par_iter().map(run).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let results: Vec<(Rect, Vec<f32>)> = chunk.iter().map(run).collect();
        if ctl.cancelled() {
            return None;
        }
        // Checked filters can reject malformed geometry or an allocation failure.
        // Do not hand an incomplete tile to Surface::write_region.
        if (shape.is_some() || matches!(params, FilterParams::UnsharpMask { .. }))
            && results.iter().any(|(t, data)| {
                data.len() != t.width() as usize * t.height() as usize * fmt.channels()
            })
        {
            return None;
        }
        for (t, data) in results {
            out.write_region(t, &data);
        }
    }
    out.prune();
    ctl.progress(1.0);
    Some(out)
}

/// Bytes of float filter results held at once by [`apply_tiled`].
pub(crate) const RESULT_BUDGET: usize = 256 << 20;

/// The current upstream sampler through the identical source/selection/tile pipeline.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_radial_reference(
    surface: &Surface,
    params: &FilterParams,
    area: Rect,
    bounds: Rect,
    selection: Option<&Surface>,
    extent: Rect,
    ctl: &rust_motion_raster::Interrupt,
) -> Option<Surface> {
    apply_tiled_with_impl(
        surface,
        params,
        area.intersect(&extent),
        bounds,
        selection,
        auto_tile(params, bounds),
        Some(extent),
        ctl,
        false,
    )
}
