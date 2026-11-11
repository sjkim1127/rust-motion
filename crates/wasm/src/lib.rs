//! Rust Motion WebAssembly Engine — In-browser real-time timeline player & canvas renderer.
//!
//! Provides zero-latency frame scrubbing, sub-16ms CPU rasterization directly to HTML5 Canvas,
//! Remotion-compatible animation math, and TouchDesigner-inspired generative visual synthesis.

pub mod generative;
pub mod math;
pub mod player;

pub use generative::GenerativeCanvas;
pub use math::*;
pub use player::{TouchDesignerGenerativeComposition, WasmPlayer};

use wasm_bindgen::prelude::*;

/// Initialize console error panic hook for readable browser panic messages.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
