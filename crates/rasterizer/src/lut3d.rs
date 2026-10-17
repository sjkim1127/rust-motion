//! 3D Look-Up Table (LUT) parser and high-precision interpolation engine.
//!
//! Supports:
//! - `.cube` Adobe/Resolve 3D LUT format with `LUT_3D_SIZE`, `DOMAIN_MIN`, `DOMAIN_MAX`, and comments
//! - `.3dl` Autodesk Flame / Lustre 3D LUT format
//! - Both Trilinear and Tetrahedral (industry standard 6-tetrahedra decomposition) interpolation
//! - Built-in cinema film emulation LUTs: Kodak Portra 400, Fujifilm Provia 100, Cinematic Teal & Orange

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use tiny_skia::Pixmap;

/// A 3D colour lookup table.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut3d {
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub data: Vec<[f32; 3]>,
    pub title: Option<String>,
}

impl Lut3d {
    /// Create a new 3D LUT with given size and data points (ordered red fastest, then green, then blue).
    pub fn new(size: usize, data: Vec<[f32; 3]>, title: Option<String>) -> Result<Self, String> {
        if !(2..=256).contains(&size) {
            return Err(format!("LUT size {size} out of supported range (2..=256)"));
        }
        let expected = size * size * size;
        if data.len() != expected {
            return Err(format!("expected {expected} entries, found {}", data.len()));
        }
        Ok(Self {
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            data,
            title,
        })
    }

    /// Parse a `.cube` file.
    pub fn parse_cube(text: &str) -> Result<Lut3d, String> {
        let (mut size, mut title) = (0usize, None);
        let (mut min, mut max) = ([0.0f32; 3], [1.0f32; 3]);
        let mut data = Vec::new();

        let parse_three = |rest: &str| -> Option<[f32; 3]> {
            let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() == 3 {
                Some([v[0], v[1], v[2]])
            } else {
                None
            }
        };

        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            if let Some(r) = l.strip_prefix("TITLE") {
                title = Some(r.trim().trim_matches('"').to_string());
            } else if let Some(r) = l.strip_prefix("LUT_3D_SIZE") {
                size = r.trim().parse().map_err(|_| format!("bad LUT_3D_SIZE `{}`", r.trim()))?;
                if !(2..=256).contains(&size) {
                    return Err(format!("LUT_3D_SIZE {size} out of range (2..=256)"));
                }
            } else if l.starts_with("LUT_1D_SIZE") {
                return Err("1D LUTs are not supported (only 3D .cube files)".into());
            } else if let Some(r) = l.strip_prefix("DOMAIN_MIN") {
                min = parse_three(r).ok_or("bad DOMAIN_MIN")?;
            } else if let Some(r) = l.strip_prefix("DOMAIN_MAX") {
                max = parse_three(r).ok_or("bad DOMAIN_MAX")?;
            } else if l.starts_with(|c: char| c.is_ascii_alphabetic()) {
                // Ignore unrecognized header directives
            } else {
                data.push(parse_three(l).ok_or_else(|| format!("bad line `{l}`"))?);
            }
        }

        if size == 0 {
            return Err("not a 3D .cube LUT (no LUT_3D_SIZE keyword)".into());
        }
        let expected = size * size * size;
        if data.len() != expected {
            return Err(format!("expected {expected} entries, found {}", data.len()));
        }

        Ok(Lut3d {
            size,
            data,
            domain_min: min,
            domain_max: max,
            title,
        })
    }

    /// Parse an Autodesk Flame / Lustre `.3dl` file.
    pub fn parse_3dl(text: &str) -> Result<Lut3d, String> {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
        let mut size = 0usize;
        let mut data = Vec::new();

        // 3dl files either start with "3DMESH" or a list of mesh indices
        for line in lines.by_ref() {
            if line.starts_with("3DMESH") {
                continue;
            }
            if line.starts_with("Mesh") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    if let Ok(s) = parts[1].parse::<usize>() {
                        size = s;
                        break;
                    }
                }
            }
            let nums: Vec<f32> = line.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if nums.len() > 1 && size == 0 {
                // Input mesh coordinate line (e.g. 0 64 128 192 ...)
                size = nums.len();
                break;
            }
        }

        if size == 0 {
            return Err("invalid .3dl file: failed to determine grid size".into());
        }

        // Determine max output scale (1023 or 4095 or 1.0)
        let mut max_val: f32 = 1.0;
        let mut raw_triplets = Vec::new();
        for line in lines {
            let nums: Vec<f32> = line.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if nums.len() >= 3 {
                max_val = max_val.max(nums[0]).max(nums[1]).max(nums[2]);
                raw_triplets.push([nums[0], nums[1], nums[2]]);
            }
        }

        let scale = if max_val > 256.0 {
            if max_val > 1024.0 { 4095.0 } else { 1023.0 }
        } else if max_val > 1.0 {
            255.0
        } else {
            1.0
        };

        for [r, g, b] in raw_triplets {
            data.push([r / scale, g / scale, b / scale]);
        }

        let expected = size * size * size;
        if data.len() != expected {
            return Err(format!("expected {expected} entries in .3dl, found {}", data.len()));
        }

        Ok(Lut3d {
            size,
            data,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            title: None,
        })
    }

    /// Look up RGB with trilinear interpolation.
    pub fn apply_trilinear(&self, c: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let s = (n - 1) as f32;
        let mut idx = [0usize; 3];
        let mut frac = [0.0f32; 3];

        for k in 0..3 {
            let span = (self.domain_max[k] - self.domain_min[k]).max(1e-6);
            let x = (((c[k] - self.domain_min[k]) / span).clamp(0.0, 1.0)) * s;
            let i = (x.floor() as usize).min(n - 2);
            idx[k] = i;
            frac[k] = x - i as f32;
        }

        let at = |r: usize, g: usize, b: usize| self.data[r + n * (g + n * b)];
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ];

        let (r, g, b) = (idx[0], idx[1], idx[2]);
        let c00 = lerp(at(r, g, b), at(r + 1, g, b), frac[0]);
        let c10 = lerp(at(r, g + 1, b), at(r + 1, g + 1, b), frac[0]);
        let c01 = lerp(at(r, g, b + 1), at(r + 1, g, b + 1), frac[0]);
        let c11 = lerp(at(r, g + 1, b + 1), at(r + 1, g + 1, b + 1), frac[0]);

        lerp(lerp(c00, c10, frac[1]), lerp(c01, c11, frac[1]), frac[2])
    }

    /// Look up RGB with tetrahedral interpolation.
    ///
    /// Tetrahedral interpolation partitions each subcube into 6 tetrahedra, providing
    /// continuous piece-wise linear interpolation without diagonal artifacts.
    pub fn apply_tetrahedral(&self, c: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let s = (n - 1) as f32;
        let mut idx = [0usize; 3];
        let mut frac = [0.0f32; 3];

        for k in 0..3 {
            let span = (self.domain_max[k] - self.domain_min[k]).max(1e-6);
            let x = (((c[k] - self.domain_min[k]) / span).clamp(0.0, 1.0)) * s;
            let i = (x.floor() as usize).min(n - 2);
            idx[k] = i;
            frac[k] = x - i as f32;
        }

        let at = |r: usize, g: usize, b: usize| self.data[r + n * (g + n * b)];
        let (r, g, b) = (idx[0], idx[1], idx[2]);

        let p000 = at(r, g, b);
        let p100 = at(r + 1, g, b);
        let p010 = at(r, g + 1, b);
        let p001 = at(r, g, b + 1);
        let p110 = at(r + 1, g + 1, b);
        let p101 = at(r + 1, g, b + 1);
        let p011 = at(r, g + 1, b + 1);
        let p111 = at(r + 1, g + 1, b + 1);

        let fx = frac[0];
        let fy = frac[1];
        let fz = frac[2];

        let mut out = [0.0f32; 3];
        for k in 0..3 {
            let v = if fx >= fy {
                if fy >= fz {
                    p000[k] + fx * (p100[k] - p000[k]) + fy * (p110[k] - p100[k]) + fz * (p111[k] - p110[k])
                } else if fx >= fz {
                    p000[k] + fx * (p100[k] - p000[k]) + fz * (p101[k] - p100[k]) + fy * (p111[k] - p101[k])
                } else {
                    p000[k] + fz * (p001[k] - p000[k]) + fx * (p101[k] - p001[k]) + fy * (p111[k] - p101[k])
                }
            } else if fx >= fz {
                p000[k] + fy * (p010[k] - p000[k]) + fx * (p110[k] - p010[k]) + fz * (p111[k] - p110[k])
            } else if fy >= fz {
                p000[k] + fy * (p010[k] - p000[k]) + fz * (p011[k] - p010[k]) + fx * (p111[k] - p011[k])
            } else {
                p000[k] + fz * (p001[k] - p000[k]) + fy * (p011[k] - p001[k]) + fx * (p111[k] - p011[k])
            };
            out[k] = v;
        }

        out
    }

    /// Look up RGB with chosen interpolation method.
    pub fn apply(&self, c: [f32; 3], tetrahedral: bool) -> [f32; 3] {
        if tetrahedral {
            self.apply_tetrahedral(c)
        } else {
            self.apply_trilinear(c)
        }
    }

    /// Apply this LUT in-place to an RGBA tiny-skia Pixmap with strength `amount` (0.0..=1.0).
    pub fn apply_to_pixmap(&self, pixmap: &mut Pixmap, amount: f32, tetrahedral: bool) {
        if amount <= 0.0 {
            return;
        }
        let amt = amount.clamp(0.0, 1.0);

        for px in pixmap.pixels_mut() {
            let a = px.alpha();
            if a == 0 {
                continue;
            }
            let inv_a = 1.0 / (a as f32);
            let r = (px.red() as f32 * inv_a).clamp(0.0, 1.0);
            let g = (px.green() as f32 * inv_a).clamp(0.0, 1.0);
            let b = (px.blue() as f32 * inv_a).clamp(0.0, 1.0);

            let lut_out = self.apply([r, g, b], tetrahedral);

            let res_r = (r + (lut_out[0].clamp(0.0, 1.0) - r) * amt) * a as f32;
            let res_g = (g + (lut_out[1].clamp(0.0, 1.0) - g) * amt) * a as f32;
            let res_b = (b + (lut_out[2].clamp(0.0, 1.0) - b) * amt) * a as f32;

            if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(
                res_r.round().min(255.0) as u8,
                res_g.round().min(255.0) as u8,
                res_b.round().min(255.0) as u8,
                a,
            ) {
                *px = c;
            }
        }
    }

    /// Built-in cinema profile: Kodak Portra 400 (warm golden skin tones, compressed highlights, gentle contrast).
    pub fn kodak_portra_400() -> Self {
        let size = 17;
        let mut data = Vec::with_capacity(size * size * size);
        for b_idx in 0..size {
            for g_idx in 0..size {
                for r_idx in 0..size {
                    let r = r_idx as f32 / (size - 1) as f32;
                    let g = g_idx as f32 / (size - 1) as f32;
                    let b = b_idx as f32 / (size - 1) as f32;

                    // Film S-curve tone response
                    let s_curve = |x: f32| -> f32 {
                        let c = x.clamp(0.0, 1.0);
                        c * c * (3.0 - 2.0 * c)
                    };

                    let r_tone = s_curve(r);
                    let g_tone = s_curve(g);
                    let b_tone = s_curve(b);

                    // Warm shift in midtones, slight cyan/blue shadow compression
                    let out_r = (r_tone * 1.05 + 0.02 * (1.0 - r)).clamp(0.0, 1.0);
                    let out_g = (g_tone * 0.98 + 0.01 * (1.0 - g)).clamp(0.0, 1.0);
                    let out_b = (b_tone * 0.92 + 0.04 * (1.0 - b)).clamp(0.0, 1.0);

                    data.push([out_r, out_g, out_b]);
                }
            }
        }
        Self::new(size, data, Some("Kodak Portra 400".into())).expect("valid lut")
    }

    /// Built-in cinema profile: Fujifilm Provia 100 (crisp high contrast, deep greens, vivid skies).
    pub fn fujifilm_provia_100() -> Self {
        let size = 17;
        let mut data = Vec::with_capacity(size * size * size);
        for b_idx in 0..size {
            for g_idx in 0..size {
                for r_idx in 0..size {
                    let r = r_idx as f32 / (size - 1) as f32;
                    let g = g_idx as f32 / (size - 1) as f32;
                    let b = b_idx as f32 / (size - 1) as f32;

                    let contrast = |x: f32| -> f32 {
                        let shifted = x - 0.5;
                        (0.5 + shifted * 1.2).clamp(0.0, 1.0)
                    };

                    let out_r = contrast(r) * 0.96;
                    let out_g = (contrast(g) * 1.04).clamp(0.0, 1.0);
                    let out_b = (contrast(b) * 1.06).clamp(0.0, 1.0);

                    data.push([out_r, out_g, out_b]);
                }
            }
        }
        Self::new(size, data, Some("Fujifilm Provia 100".into())).expect("valid lut")
    }

    /// Built-in cinema profile: Cinematic Teal & Orange (Hollywood blockbuster contrast).
    pub fn cinematic_teal_orange() -> Self {
        let size = 17;
        let mut data = Vec::with_capacity(size * size * size);
        for b_idx in 0..size {
            for g_idx in 0..size {
                for r_idx in 0..size {
                    let r = r_idx as f32 / (size - 1) as f32;
                    let g = g_idx as f32 / (size - 1) as f32;
                    let b = b_idx as f32 / (size - 1) as f32;

                    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;

                    // Shadows push teal (cyan+blue), highlights push warm orange
                    let shadow_teal = 1.0 - lum;
                    let highlight_orange = lum;

                    let out_r = (r + highlight_orange * 0.15 - shadow_teal * 0.1).clamp(0.0, 1.0);
                    let out_g = (g + highlight_orange * 0.05 + shadow_teal * 0.05).clamp(0.0, 1.0);
                    let out_b = (b - highlight_orange * 0.12 + shadow_teal * 0.20).clamp(0.0, 1.0);

                    data.push([out_r, out_g, out_b]);
                }
            }
        }
        Self::new(size, data, Some("Cinematic Teal & Orange".into())).expect("valid lut")
    }
}

fn registry() -> &'static RwLock<HashMap<String, Arc<Lut3d>>> {
    static R: OnceLock<RwLock<HashMap<String, Arc<Lut3d>>>> = OnceLock::new();
    R.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert("kodak_portra_400".into(), Arc::new(Lut3d::kodak_portra_400()));
        map.insert("fujifilm_provia_100".into(), Arc::new(Lut3d::fujifilm_provia_100()));
        map.insert("cinematic_teal_orange".into(), Arc::new(Lut3d::cinematic_teal_orange()));
        RwLock::new(map)
    })
}

/// Register a named LUT in the global registry.
pub fn register(id: &str, lut: Lut3d) {
    registry().write().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), Arc::new(lut));
}

/// Retrieve a registered LUT by name or identifier.
pub fn get(id: &str) -> Option<Arc<Lut3d>> {
    registry().read().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
}

/// Remove a registered LUT from the global registry.
pub fn unregister(id: &str) {
    registry().write().unwrap_or_else(|e| e.into_inner()).remove(id);
}
