//! Mesh construction: outline extraction, triangulation and band ordering.

use super::*;

/// Most outline points the mesh takes; a busier outline is simplified more coarsely.
const MAX_OUTLINE_POINTS: usize = 20_000;

/// Builds the mesh over the covered part of `src` inside `bounds`.
///
/// The outline of the opaque region grown by `expansion` is traced, simplified (corners kept)
/// and split into sides no longer than the density's spacing; a triangular lattice at that
/// spacing fills the inside, at least half a step from the outline. The mesh is the
/// Delaunay triangulation of those points with the outline's sides recovered (split until
/// each is an edge), minus the triangles outside the outline.
pub fn build_mesh(
    src: &Surface,
    bounds: Rect,
    density: PuppetDensity,
    expansion: f64,
) -> PuppetMesh {
    if bounds.is_empty() {
        return PuppetMesh::default();
    }
    let e = if expansion.is_finite() {
        expansion.clamp(-200.0, 200.0)
    } else {
        0.0
    };
    // The density counts steps across the region the mesh covers (grown by the expansion).
    let long = f64::from(bounds.width().max(bounds.height())) + 2.0 * e.max(0.0);
    let spacing = (long / density.cells()).max(2.0);
    let field = Field::new(src, bounds, e);
    let loops = field.outline();
    let empty = PuppetMesh {
        spacing,
        ..Default::default()
    };
    if loops.is_empty() {
        return empty;
    }
    let mut tol = (0.5 * field.g).max(0.5);
    let rings = loop {
        let rings: Vec<Vec<[f64; 2]>> = loops
            .iter()
            .map(|l| densify(&simplify(l, tol), spacing))
            .collect();
        if rings.iter().map(Vec::len).sum::<usize>() <= MAX_OUTLINE_POINTS || tol > spacing {
            break rings;
        }
        tol *= 2.0;
    };
    let (lo, hi) = (
        [field.x0, field.y0],
        [
            field.x0 + field.bw as f64 * field.g,
            field.y0 + field.bh as f64 * field.g,
        ],
    );
    let mut dl = Delaunay::new(lo, hi);
    let mut segs: Vec<(usize, usize)> = Vec::new();
    for ring in &rings {
        let idx: Vec<usize> = ring.iter().filter_map(|&p| dl.insert(p)).collect();
        for (k, &a) in idx.iter().enumerate() {
            let b = idx[(k + 1) % idx.len()];
            if a != b {
                segs.push((a, b));
            }
        }
    }
    // The inside: a triangular lattice, kept half a step clear of the outline.
    let rise = spacing * 3f64.sqrt() / 2.0;
    let rows = ((hi[1] - lo[1]) / rise).ceil() as usize;
    let cols = ((hi[0] - lo[0]) / spacing).ceil() as usize;
    for j in 0..=rows {
        for i in 0..=cols {
            let p = [
                lo[0] + (i as f64 + 0.5 * (j % 2) as f64) * spacing,
                lo[1] + j as f64 * rise,
            ];
            if field.value(p) < -0.5 * spacing {
                dl.insert(p);
            }
        }
    }
    // Recover the outline's sides: split a side the triangulation lacks at its midpoint.
    let mut budget = segs.len() * 2;
    for _ in 0..8 {
        let edges = dl.edges();
        let mut next = Vec::with_capacity(segs.len());
        let mut split = false;
        for &(a, b) in &segs {
            if budget == 0 || edges.contains(&(a.min(b), a.max(b))) {
                next.push((a, b));
                continue;
            }
            match dl.insert(lerp(dl.pts[a], dl.pts[b], 0.5)) {
                Some(c) if c != a && c != b => {
                    next.push((a, c));
                    next.push((c, b));
                    budget -= 1;
                    split = true;
                }
                _ => next.push((a, b)),
            }
        }
        segs = next;
        if !split {
            break;
        }
    }
    let inside = Inside::new(&dl.pts, &segs, spacing);
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for t in dl
        .tris
        .iter()
        .filter(|t| t.alive && t.v.iter().all(|&v| v >= 3))
    {
        let [a, b, c] = t.v.map(|v| dl.pts[v]);
        if orient(a, b, c) > 1e-9
            && inside.contains([(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0])
        {
            tris.push(t.v);
        }
    }
    if tris.is_empty() {
        return empty;
    }
    // Keep the vertices in use, numbered for a narrow band in the solver's matrix.
    let pos = &dl.pts;
    let order = band_order(pos, &tris);
    let mut remap = vec![usize::MAX; pos.len()];
    for (new, &old) in order.iter().enumerate() {
        remap[old] = new;
    }
    PuppetMesh {
        verts: order.iter().map(|&i| pos[i]).collect(),
        tris: tris.iter().map(|t| t.map(|i| remap[i])).collect(),
        spacing,
    }
}

/// The vertices `tris` use, in reverse Cuthill–McKee order (breadth first from the topmost
/// vertex of each piece, lower degrees first), which keeps the solver's banded factor small.
fn band_order(pos: &[[f64; 2]], tris: &[[usize; 3]]) -> Vec<usize> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); pos.len()];
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            adj[a].push(b);
            adj[b].push(a);
        }
    }
    for a in &mut adj {
        a.sort_unstable();
        a.dedup();
    }
    let mut starts: Vec<usize> = (0..pos.len()).filter(|&i| !adj[i].is_empty()).collect();
    starts.sort_by(|&a, &b| {
        pos[a][1]
            .total_cmp(&pos[b][1])
            .then(pos[a][0].total_cmp(&pos[b][0]))
    });
    let mut seen = vec![false; pos.len()];
    let mut order = Vec::with_capacity(starts.len());
    for s in starts {
        if seen[s] {
            continue;
        }
        seen[s] = true;
        let first = order.len();
        order.push(s);
        let mut i = first;
        while let Some(&v) = order.get(i) {
            i += 1;
            let mut next: Vec<usize> = adj[v].iter().copied().filter(|&w| !seen[w]).collect();
            next.sort_by_key(|&w| adj[w].len());
            for w in next {
                seen[w] = true;
                order.push(w);
            }
        }
    }
    order.reverse();
    order
}
