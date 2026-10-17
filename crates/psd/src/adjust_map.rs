//! Mapping between PSD adjustment-layer blocks and `rust_motion_color::Adjustment`.

use rust_motion_color::adjust::{
    CURVE_MAP_LEN, CurvePoint, HueRange, LevelsChannel, ToneSpace, curve_map,
    Adjustment,
};
use crate::descriptor::{Descriptor, Value, VersionedDescriptor};
use crate::header::ColorMode;

/// All PSD adjustment keys recognized as adjustment layers.
pub const ADJUSTMENT_KEYS: [&[u8; 4]; 16] = [
    b"levl", b"curv", b"hue2", b"brit", b"nvrt", b"thrs", b"post", b"expA",
    b"vibA", b"blnc", b"mixr", b"grdm", b"phfl", b"selc", b"blwh", b"clrL",
];

fn be16(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}
fn bei16(d: &[u8], at: usize) -> Option<i16> {
    be16(d, at).map(|v| v as i16)
}
fn be32(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
fn bef32(d: &[u8], at: usize) -> Option<f32> {
    d.get(at..at + 4).map(|b| f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn unsupported(key: &[u8; 4], data: &[u8]) -> Adjustment {
    Adjustment::Unsupported {
        psd_key: String::from_utf8_lossy(key).into_owned(),
        raw: data.to_vec(),
    }
}

fn levels_rec(d: &[u8], at: usize) -> Option<LevelsChannel> {
    Some(LevelsChannel {
        in_black: f32::from(be16(d, at)?) / 255.0,
        in_white: f32::from(be16(d, at + 2)?) / 255.0,
        out_black: f32::from(be16(d, at + 4)?) / 255.0,
        out_white: f32::from(be16(d, at + 6)?) / 255.0,
        gamma: f32::from(be16(d, at + 8)?) / 100.0,
    })
}

fn parse_curves(d: &[u8]) -> Option<Adjustment> {
    let pencil = *d.first()? != 0;
    let version = be16(d, 1)?;
    if version != 1 && version != 4 {
        return None;
    }
    let bits = u32::from_be_bytes(d.get(3..7)?.try_into().ok()?);
    let mut at = 7;
    let line = || vec![CurvePoint { input: 0.0, output: 0.0 }, CurvePoint { input: 1.0, output: 1.0 }];
    let mut curves: Vec<Vec<CurvePoint>> = vec![line(), line(), line(), line(), line()];
    for bit in 0..32usize {
        if bits & (1 << bit) == 0 {
            continue;
        }
        if pencil {
            let map = d.get(at..at + CURVE_MAP_LEN)?;
            at += CURVE_MAP_LEN;
            if let Some(slot) = curves.get_mut(bit) {
                *slot = curve_map(&map.iter().map(|&b| f32::from(b) / 255.0).collect::<Vec<_>>());
            }
            continue;
        }
        let n = usize::from(be16(d, at)?);
        at += 2;
        let mut pts = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            let out = be16(d, at)?;
            let inp = be16(d, at + 2)?;
            at += 4;
            pts.push(CurvePoint { input: f32::from(inp) / 255.0, output: f32::from(out) / 255.0 });
        }
        if let Some(slot) = curves.get_mut(bit) {
            *slot = pts;
        }
    }
    let mut it = curves.into_iter();
    let master = it.next()?;
    let (r, g, b, k) = (it.next()?, it.next()?, it.next()?, it.next()?);
    Some(Adjustment::Curves { master, per_channel: [r, g, b], space: ToneSpace::Rgb, black: k, pencil })
}

fn desc_num(d: &Descriptor, key: &str) -> Option<f32> {
    match d.get(key)? {
        Value::Integer(v) => Some(*v as f32),
        Value::Double(v) => Some(*v as f32),
        Value::UnitFloat { value, .. } => Some(*value as f32),
        _ => None,
    }
}

fn desc_bool(d: &Descriptor, key: &str) -> Option<bool> {
    match d.get(key)? {
        Value::Boolean(b) => Some(*b),
        _ => None,
    }
}

fn versioned(d: &[u8]) -> Option<Descriptor> {
    VersionedDescriptor::parse_prefix(d).ok().map(|(v, _)| v.descriptor)
}

fn parse_vibrance(d: &[u8]) -> Option<Adjustment> {
    let desc = versioned(d)?;
    let get = |k: &str| desc_num(&desc, k).unwrap_or(0.0).clamp(-100.0, 100.0);
    Some(Adjustment::Vibrance { vibrance: get("vibrance"), saturation: get("Strt") })
}

fn parse_color_balance(d: &[u8]) -> Option<Adjustment> {
    let tone = |k: usize| -> Option<[f32; 3]> {
        let mut out = [0.0; 3];
        for (i, v) in out.iter_mut().enumerate() {
            *v = f32::from(bei16(d, (k * 3 + i) * 2)?).clamp(-100.0, 100.0);
        }
        Some(out)
    };
    Some(Adjustment::ColorBalance {
        shadows: tone(0)?,
        midtones: tone(1)?,
        highlights: tone(2)?,
        preserve_luminosity: *d.get(18)? != 0,
    })
}

fn parse_selective(d: &[u8]) -> Option<Adjustment> {
    if be16(d, 0)? != 1 || d.len() < 84 {
        return None;
    }
    let relative = be16(d, 2)? == 0;
    let mut adjustments = [[0.0f32; 4]; 9];
    for (r, rec) in adjustments.iter_mut().enumerate() {
        for (k, v) in rec.iter_mut().enumerate() {
            *v = f32::from(bei16(d, 4 + (r + 1) * 8 + k * 2)?);
        }
    }
    Some(Adjustment::SelectiveColor { relative, adjustments })
}

fn desc_rgb(d: &Descriptor) -> Option<[f32; 3]> {
    let g = |k: &str| match d.get(k)? {
        Value::Double(v) => Some(*v as f32),
        Value::Integer(v) => Some(*v as f32),
        Value::UnitFloat { value, .. } => Some(*value as f32),
        _ => None,
    };
    if let (Some(r), Some(g), Some(b)) = (g("Rd  "), g("Grn "), g("Bl  ")) {
        return Some([(r / 255.0).clamp(0.0, 1.0), (g / 255.0).clamp(0.0, 1.0), (b / 255.0).clamp(0.0, 1.0)]);
    }
    None
}

const BW_DEFAULT: [f32; 6] = [40.0, 60.0, 40.0, 60.0, 20.0, 80.0];
const BW_KEYS: [&str; 6] = ["Rd  ", "Yllw", "Grn ", "Cyn ", "Bl  ", "Mgnt"];
const BW_TINT: [f32; 3] = [225.0 / 255.0, 211.0 / 255.0, 179.0 / 255.0];

fn parse_black_white(d: &[u8]) -> Option<Adjustment> {
    let desc = versioned(d)?;
    let mut weights = BW_DEFAULT;
    for (i, k) in BW_KEYS.iter().enumerate() {
        if let Some(v) = desc_num(&desc, k) {
            weights[i] = v;
        }
    }
    let tint = desc_bool(&desc, "useTint").unwrap_or(false).then(|| {
        desc.get("tintColor").and_then(|v| match v {
            Value::Descriptor(d) => desc_rgb(d),
            _ => None,
        }).unwrap_or(BW_TINT)
    });
    Some(Adjustment::BlackWhite { weights, tint })
}

fn parse_photo_filter(d: &[u8]) -> Option<Adjustment> {
    let version = be16(d, 0)?;
    let (color, at) = match version {
        1 | 2 => {
            let space = be16(d, 2)?;
            let c = [be16(d, 4)?, be16(d, 6)?, be16(d, 8)?, be16(d, 10)?];
            let rgb = if space == 0 {
                [c[0] as f32 / 65535.0, c[1] as f32 / 65535.0, c[2] as f32 / 65535.0]
            } else {
                [1.0, 0.5, 0.0]
            };
            (rgb, 12)
        }
        3 => {
            let desc = versioned(d.get(2..)?)?;
            let rgb = desc.get("Clr ").and_then(|v| match v {
                Value::Descriptor(d) => desc_rgb(d),
                _ => None,
            }).unwrap_or([1.0, 0.5, 0.0]);
            (rgb, d.len().saturating_sub(5))
        }
        _ => return None,
    };
    let density = be32(d, at)? as f32 / 100.0;
    let preserve_luminosity = *d.get(at + 4)? != 0;
    Some(Adjustment::PhotoFilter { color, density, preserve_luminosity })
}

fn parse_channel_mixer(d: &[u8]) -> Option<Adjustment> {
    let version = be16(d, 0)?;
    if version != 1 || d.len() < 16 {
        return None;
    }
    let monochrome = be16(d, 2)? != 0;
    let mut matrix = [[0.0f32; 4]; 3];
    for (r, row) in matrix.iter_mut().enumerate() {
        for (c, val) in row.iter_mut().enumerate() {
            let at = 4 + (r * 5 + c) * 2;
            if let Some(v) = bei16(d, at) {
                *val = f32::from(v) / 100.0;
            }
        }
    }
    Some(Adjustment::ChannelMixer { matrix, monochrome })
}

/// Parses an adjustment layer block into a `rust_motion_color::Adjustment`.
pub fn parse(key: &[u8; 4], data: &[u8], cged: Option<&[u8]>, mode: ColorMode) -> Adjustment {
    let mut a = parse_any(key, data, cged);
    match (&mut a, mode) {
        (Adjustment::Levels { space, .. }, ColorMode::Cmyk) => {
            *space = ToneSpace::Cmyk;
        }
        (Adjustment::Levels { space, black, .. }, ColorMode::Lab) => {
            *space = ToneSpace::Lab;
            *black = LevelsChannel::default();
        }
        (Adjustment::Curves { space, black, .. }, ColorMode::Cmyk) => {
            *space = ToneSpace::Cmyk;
            if rust_motion_color::adjust::is_identity_curve(black) {
                black.clear();
            }
        }
        (Adjustment::Curves { space, black, .. }, ColorMode::Lab) => {
            *space = ToneSpace::Lab;
            black.clear();
        }
        (Adjustment::Levels { black, .. }, _) => *black = LevelsChannel::default(),
        (Adjustment::Curves { black, .. }, _) => black.clear(),
        _ => {}
    }
    match (&mut a, mode) {
        (_, ColorMode::Rgb | ColorMode::Cmyk | ColorMode::Lab) => {}
        (Adjustment::Levels { per_channel, .. }, ColorMode::Grayscale) => {
            let g = per_channel[0].clone();
            *per_channel = [g.clone(), g.clone(), g];
        }
        (Adjustment::Curves { per_channel, .. }, ColorMode::Grayscale) => {
            let g = per_channel[0].clone();
            *per_channel = [g.clone(), g.clone(), g];
        }
        _ => {}
    }
    a
}

fn parse_any(key: &[u8; 4], data: &[u8], cged: Option<&[u8]>) -> Adjustment {
    let parsed = match key {
        b"nvrt" => Some(Adjustment::Invert),
        b"thrs" => be16(data, 0).map(|v| Adjustment::Threshold { level: f32::from(v) / 255.0 }),
        b"post" => be16(data, 0).map(|v| Adjustment::Posterize { levels: u32::from(v) }),
        b"brit" => {
            let modern = cged.and_then(|c| VersionedDescriptor::parse_prefix(c).ok()).and_then(|(v, _)| {
                let d = v.descriptor;
                Some(Adjustment::BrightnessContrast {
                    brightness: desc_num(&d, "Brgh")?,
                    contrast: desc_num(&d, "Cntr")?,
                    legacy: desc_bool(&d, "useLegacy").unwrap_or(false),
                })
            });
            modern.or_else(|| {
                Some(Adjustment::BrightnessContrast {
                    brightness: f32::from(bei16(data, 0)?),
                    contrast: f32::from(bei16(data, 2)?),
                    legacy: true,
                })
            })
        }
        b"hue2" => (|| {
            let colorize = *data.get(2)? != 0;
            let base = if colorize { 4 } else { 10 };
            let mut ranges = HueRange::defaults();
            for (i, r) in ranges.iter_mut().enumerate() {
                let at = 16 + i * 14;
                let v = |k: usize| bei16(data, at + 2 * k).map(f32::from);
                if let (Some(a), Some(b), Some(c), Some(d), Some(h), Some(s), Some(l)) = (v(0), v(1), v(2), v(3), v(4), v(5), v(6)) {
                    *r = HueRange { hue: h, saturation: s, lightness: l, bounds: HueRange::canonical_bounds([a, b, c, d]) };
                }
            }
            Some(Adjustment::HueSaturation {
                hue: f32::from(bei16(data, base)?),
                saturation: f32::from(bei16(data, base + 2)?),
                lightness: f32::from(bei16(data, base + 4)?),
                colorize,
                ranges,
            })
        })(),
        b"expA" => (|| Some(Adjustment::Exposure { exposure: bef32(data, 2)?, offset: bef32(data, 6)?, gamma: bef32(data, 10)? }))(),
        b"levl" => (|| {
            let m = levels_rec(data, 2)?;
            let r = levels_rec(data, 12)?;
            let g = levels_rec(data, 22)?;
            let b = levels_rec(data, 32)?;
            let k = levels_rec(data, 42).unwrap_or_default();
            Some(Adjustment::Levels { master: m, per_channel: [r, g, b], space: ToneSpace::Rgb, black: k })
        })(),
        b"curv" => parse_curves(data),
        b"selc" => parse_selective(data),
        b"vibA" => parse_vibrance(data),
        b"blnc" => parse_color_balance(data),
        b"blwh" => parse_black_white(data),
        b"phfl" => parse_photo_filter(data),
        b"mixr" => parse_channel_mixer(data),
        _ => None,
    };
    parsed.unwrap_or_else(|| unsupported(key, data))
}
