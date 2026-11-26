//! Built-in native compositions and the small prop helpers they share.

use rust_motion_rasterizer::Color;
use serde_json::Value;

mod basic;
mod data;
mod graphics;
mod media;

pub use basic::*;
pub use data::*;
pub use graphics::*;
pub use media::*;

fn color_prop(props: &Value, key: &str, fallback: Color) -> Color {
    props
        .get(key)
        .and_then(Value::as_str)
        .and_then(Color::from_hex)
        .unwrap_or(fallback)
}

fn string_prop(props: &Value, key: &str, fallback: &str) -> String {
    props
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}
