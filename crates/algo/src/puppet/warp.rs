//! Public entry points: deforming a mesh and rendering the warped layer or its mask.

use super::*;

/// Number of local/global rounds used by the command.
pub const ITERATIONS: usize = 40;

/// The deformed mesh of a puppet warp: (solver, deformed vertices, draw order).
pub fn deform(
    src: &Surface,
    bounds: Rect,
    w: &PuppetWarp,
    iterations: usize,
) -> (PuppetSolver, Vec<[f64; 2]>, Vec<usize>) {
    let mesh = build_mesh(src, bounds, w.density, w.expansion);
    let pin_src: Vec<[f64; 2]> = w.pins.iter().map(|p| p.src).collect();
    let fixed: Vec<bool> = w.pins.iter().map(|p| p.rotate.is_some()).collect();
    let solver = PuppetSolver::new(mesh, &pin_src, &fixed);
    let dst: Vec<[f64; 2]> = w.pins.iter().map(|p| p.dst).collect();
    let rot: Vec<Option<f64>> = w.pins.iter().map(|p| p.rotate).collect();
    let (v, _) = solver.solve(&dst, &rot, w.mode, iterations, None);
    let depths: Vec<i32> = w.pins.iter().map(|p| p.depth).collect();
    let order = solver.draw_order(&depths, &pin_src);
    (solver, v, order)
}

/// Rasterizes the mesh deformation of `src` (inside `bounds`): the deformed triangles only, so
/// pixels outside the mesh are dropped.
pub fn render(
    src: &Surface,
    bounds: Rect,
    mesh: &PuppetMesh,
    deformed: &[[f64; 2]],
    order: &[usize],
    interp: Interp,
) -> Surface {
    let verts: Vec<([f64; 2], [f64; 2])> = deformed
        .iter()
        .zip(&mesh.verts)
        .map(|(d, s)| (*d, *s))
        .collect();
    let tris: Vec<[usize; 3]> = order.iter().map(|&t| mesh.tris[t]).collect();
    crate::warp::warp_triangles(src, bounds, &verts, &tris, interp)
}

/// The largest area (px) a puppet warp's result may span: 2³⁰, about 16000 tiles. A pin dragged
/// absurdly far stretches the mesh over far more, and the renderer allocates bookkeeping for
/// every tile of it before drawing (#1019).
pub const MAX_WARP_PIXELS: f64 = (1u64 << 30) as f64;

/// Applies a puppet warp to `src` (content inside `bounds`). `None` when the deformed mesh would
/// span more than [`MAX_WARP_PIXELS`].
pub fn puppet_warp(src: &Surface, bounds: Rect, w: &PuppetWarp, interp: Interp) -> Option<Surface> {
    puppet_warp_with_mask(src, bounds, w, interp, None).map(|(out, _)| out)
}

/// [`puppet_warp`], also moving `mask` (a one-channel layer mask linked to `src`) with the same
/// mesh, as Photoshop moves a linked mask with its layer (#4910). The mask is `None` in the
/// result when none was given.
pub fn puppet_warp_with_mask(
    src: &Surface,
    bounds: Rect,
    w: &PuppetWarp,
    interp: Interp,
    mask: Option<&Surface>,
) -> Option<(Surface, Option<Surface>)> {
    let (solver, v, order) = deform(src, bounds, w, ITERATIONS);
    // No triangles (an expansion that shrinks every pixel away): nothing to warp. Rendering the
    // empty mesh would drop every pixel of the layer.
    if solver.mesh.tris.is_empty() {
        return Some((src.clone(), mask.cloned()));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &v {
        (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
    }
    let area = (x1 - x0 + 1.0) * (y1 - y0 + 1.0);
    if area.is_nan() || area > MAX_WARP_PIXELS {
        return None;
    }
    let out = render(src, bounds, &solver.mesh, &v, &order, interp);
    Some((
        out,
        mask.map(|m| render_mask(m, &solver.mesh, &v, &order, interp)),
    ))
}

/// Moves a one-channel layer mask with a deformed mesh: the mask under the mesh is lifted,
/// deformed like the layer's pixels and laid over the rest. The area the mesh leaves takes the
/// mask's default value (as [`crate::warp::warp_mesh_gray`] leaves it); outside the mesh the
/// mask keeps its values.
pub fn render_mask(
    mask: &Surface,
    mesh: &PuppetMesh,
    deformed: &[[f64; 2]],
    order: &[usize],
    interp: Interp,
) -> Surface {
    let mut out = mask.clone();
    if mesh.tris.is_empty() || deformed.len() != mesh.verts.len() || mask.format().channels() != 1 {
        return out;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &mesh.verts {
        (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
    }
    if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
        return out;
    }
    // The mesh's source area, a pixel wider so its edge samples stay inside.
    let lim = f64::from(1 << 20);
    let src = Rect::new(
        x0.max(-lim).floor() as i32 - 1,
        y0.max(-lim).floor() as i32 - 1,
        x1.min(lim).ceil() as i32 + 1,
        y1.min(lim).ceil() as i32 + 1,
    );
    if src.is_empty() {
        return out;
    }
    let default = mask.default_pixel().first().copied().unwrap_or(0.0);
    let fmt = mask.format();
    let mut tmp = Surface::new(rust_motion_color::PixelFormat::new(
        fmt.mode, fmt.sample, true,
    ));
    tmp.write_region(
        src,
        &mask
            .read_region(src)
            .iter()
            .flat_map(|g| [*g, 1.0])
            .collect::<Vec<f32>>(),
    );
    let tris: Vec<[usize; 3]> = order
        .iter()
        .filter_map(|&t| mesh.tris.get(t).copied())
        .collect();
    // Lift: where the undeformed mesh covers the mask, the mask takes its default value.
    let still: Vec<([f64; 2], [f64; 2])> = mesh.verts.iter().map(|s| (*s, *s)).collect();
    let lifted = crate::warp::warp_triangles(&tmp, src, &still, &tris, Interp::Nearest);
    let b = lifted.content_bounds();
    if !b.is_empty() {
        let under = out.read_region(b);
        let flat: Vec<f32> = lifted
            .read_region(b)
            .as_chunks::<2>()
            .0
            .iter()
            .zip(&under)
            .map(|(p, u)| default * p[1] + u * (1.0 - p[1]))
            .collect();
        out.write_region(b, &flat);
    }
    // Lay the deformed mask over what is left.
    let moved: Vec<([f64; 2], [f64; 2])> = deformed
        .iter()
        .zip(&mesh.verts)
        .map(|(d, s)| (*d, *s))
        .collect();
    let warped = crate::warp::warp_triangles(&tmp, src, &moved, &tris, interp);
    let b = warped.content_bounds();
    if !b.is_empty() {
        let under = out.read_region(b);
        let flat: Vec<f32> = warped
            .read_region(b)
            .as_chunks::<2>()
            .0
            .iter()
            .zip(&under)
            .map(|(p, u)| p[0] * p[1] + u * (1.0 - p[1]))
            .collect();
        out.write_region(b, &flat);
    }
    out.prune();
    out
}
