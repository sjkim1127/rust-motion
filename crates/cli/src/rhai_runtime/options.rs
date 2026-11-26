//! Option-map readers and layout-option parsing shared by the scene API.

use super::*;

pub(super) fn get_opt_f32(map: &Map, key: &str, default: f32) -> f32 {
    if let Some(v) = map.get(key) {
        if let Ok(f) = v.as_float() {
            return f as f32;
        }
        if let Ok(i) = v.as_int() {
            return i as f32;
        }
    }
    default
}

fn get_opt_f32_opt(map: &Map, key: &str) -> Option<f32> {
    if let Some(v) = map.get(key) {
        if let Ok(f) = v.as_float() {
            return Some(f as f32);
        }
        if let Ok(i) = v.as_int() {
            return Some(i as f32);
        }
    }
    None
}

pub(super) fn get_opt_string(map: &Map, key: &str) -> Option<String> {
    map.get(key).and_then(|v| v.clone().into_string().ok())
}

pub(super) fn get_opt_usize(map: &Map, key: &str, default: usize) -> usize {
    if let Some(v) = map.get(key) {
        if let Ok(i) = v.as_int() {
            if i >= 0 {
                return i as usize;
            }
        }
    }
    default
}

pub(super) fn get_opt_u64(map: &Map, key: &str, default: u64) -> u64 {
    if let Some(v) = map.get(key) {
        if let Ok(i) = v.as_int() {
            if i >= 0 {
                return i as u64;
            }
        }
    }
    default
}

pub(super) fn apply_layout_options(box_layout: &mut LayoutBox, opts: &Map) -> RhaiResult<()> {
    if let Some(p) = get_opt_f32_opt(opts, "padding") {
        box_layout.style.padding = Rect {
            left: length(p),
            right: length(p),
            top: length(p),
            bottom: length(p),
        };
    }
    if let Some(w) = get_opt_f32_opt(opts, "width") {
        box_layout.style.size.width = length(w);
    }
    if let Some(h) = get_opt_f32_opt(opts, "height") {
        box_layout.style.size.height = length(h);
    }
    if let Some(bg) = get_opt_string(opts, "bg") {
        box_layout.background = Some(parse_color(&bg)?);
    }
    if let Some(bc) = get_opt_string(opts, "border_color") {
        let bw = get_opt_f32(opts, "border_width", 1.0);
        let br = get_opt_f32(opts, "border_radius", 0.0);
        box_layout.border = Some((parse_color(&bc)?, bw, br));
    }
    Ok(())
}

pub(super) fn dynamic_to_non_negative_f32(val: &Dynamic, name: &str) -> RhaiResult<f32> {
    if let Ok(f) = val.as_float() {
        return non_negative_f32(name, f);
    }
    if let Ok(i) = val.as_int() {
        return non_negative_f32(name, i as FLOAT);
    }
    Err(runtime_error(format!("{name} must be a valid number")))
}

pub(super) fn dynamic_to_finite_f32(val: &Dynamic, name: &str) -> RhaiResult<f32> {
    if let Ok(f) = val.as_float() {
        return finite_f32(name, f);
    }
    if let Ok(i) = val.as_int() {
        return finite_f32(name, i as FLOAT);
    }
    Err(runtime_error(format!("{name} must be a valid number")))
}
