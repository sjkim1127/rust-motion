//! Whole-area application of the box-pass blurs, avoiding the per-tile re-read of their wide halo.

use super::*;

/// [`apply_tiled_with`] for the blurs run as box passes ([`blur::box_widths`]): the whole area at
/// once (in column bands of about [`RESULT_BUDGET`] bytes of floats), so the wide halo of a large
/// radius is read once rather than around every tile. Pixels beyond `extent` repeat its edge, as
/// [`Image::read_clamped`] does for tiles; without one the surface is read (transparent where empty).
pub(super) fn apply_box_blur(
    mut out: Surface,
    surface: &Surface,
    area: Rect,
    extent: Option<Rect>,
    selection: Option<&Surface>,
    boxes: &[usize],
    ctl: &rust_motion_raster::Interrupt,
) -> Option<Surface> {
    let fmt = surface.format();
    let n = fmt.channels();
    let reach = i32::try_from(boxes.iter().map(|w| w / 2).sum::<usize>()).unwrap_or(i32::MAX);
    let grown = area.inflate(reach);
    let src = extent
        .map(|e| grown.intersect(&e))
        .filter(|r| !r.is_empty())
        .unwrap_or(grown);
    let read = |r: Rect, buf: &mut Vec<f32>| surface.read_region_into(r, buf);
    // Per output column: the horizontal results for every source row, plus the final samples.
    let column_bytes =
        (src.height() as usize + area.height() as usize) * n * std::mem::size_of::<f32>();
    let band = i32::try_from(RESULT_BUDGET / column_bytes.max(1))
        .unwrap_or(i32::MAX)
        .max(blur::STRIP)
        / blur::STRIP
        * blur::STRIP;
    let bands: Vec<Rect> = (area.x0..area.x1)
        .step_by(band as usize)
        .map(|x| Rect::new(x, area.y0, x.saturating_add(band).min(area.x1), area.y1))
        .collect();
    for (i, b) in bands.iter().enumerate() {
        let strips = blur::box_blur(src, &read, *b, n, fmt.alpha, boxes, ctl)?;
        // Each strip lies in one tile column: the columns' tiles are written in parallel.
        let mut columns: Vec<(Surface, Vec<blur::Strip>)> = Vec::new();
        for (t, data) in strips {
            match columns.last_mut() {
                Some((_, v))
                    if v.first().is_some_and(|(r, _)| {
                        r.x0.div_euclid(rust_motion_geom::TILE_SIZE)
                            == t.x0.div_euclid(rust_motion_geom::TILE_SIZE)
                    }) =>
                {
                    v.push((t, data))
                }
                _ => columns.push((
                    Surface::with_default(fmt, &surface.default_pixel()),
                    vec![(t, data)],
                )),
            }
        }
        for (s, v) in &mut columns {
            let r = v.iter().fold(
                Rect::EMPTY,
                |acc, (t, _)| if acc.is_empty() { *t } else { acc.union(t) },
            );
            s.put_tiles(out.take_tiles(r));
        }
        let write = |(s, v): &mut (Surface, Vec<blur::Strip>)| {
            for (t, data) in v.iter_mut() {
                if let Some(sel) = selection {
                    mix_selection(data, *t, sel, &Image::read(surface, *t), fmt.alpha);
                }
                s.write_region(*t, data);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            columns.par_iter_mut().for_each(write);
        }
        #[cfg(target_arch = "wasm32")]
        columns.iter_mut().for_each(write);
        for (s, _) in columns {
            out.put_tiles(s.tiles().map(|(c, t)| (*c, t.clone())));
        }
        ctl.progress(0.98 * (i + 1) as f32 / bands.len() as f32);
    }
    out.prune();
    ctl.progress(1.0);
    Some(out)
}
