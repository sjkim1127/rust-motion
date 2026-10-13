//! `use_input_props` hook — loads data provided by the AI agent / CLI renderer.
//!
//! Checks the `RUST_MOTION_PROPS` environment variable first, which is injected
//! by the `rust-motion-cli` when rendering headlessly.

use dioxus::prelude::*;
use serde::de::DeserializeOwned;
use std::env;

/// Hook to retrieve input properties passed into the composition.
///
/// Tries to parse the `RUST_MOTION_PROPS` environment variable as JSON.
/// If it doesn't exist or parsing fails, it falls back to the provided `default` value.
///
/// In a browser environment, it will also attempt to read from `window.RUST_MOTION_PROPS`
/// (WASM support to be expanded).
pub fn use_input_props<T>(default: impl FnOnce() -> T) -> T
where
    T: DeserializeOwned + Clone + 'static,
{
    // Run once per component instance
    use_hook(|| {
        // 1. Try environment variable (used by CLI / headless renderer)
        if let Ok(json_str) = env::var("RUST_MOTION_PROPS") {
            match serde_json::from_str::<T>(&json_str) {
                Ok(props) => return props,
                Err(e) => {
                    tracing::warn!("Failed to parse RUST_MOTION_PROPS env var: {}", e);
                }
            }
        }

        // 2. Browser global fallback (for web preview injected by agent scripts)
        #[cfg(all(target_arch = "wasm32", feature = "web"))]
        {
            if let Some(window) = web_sys::window() {
                if let Ok(val) =
                    js_sys::Reflect::get(&window, &js_sys::JsString::from("RUST_MOTION_PROPS"))
                {
                    if let Some(json_str) = val.as_string() {
                        match serde_json::from_str::<T>(&json_str) {
                            Ok(props) => return props,
                            Err(e) => {
                                tracing::warn!("Failed to parse window.RUST_MOTION_PROPS: {}", e)
                            }
                        }
                    }
                }
            }
        }

        // 3. Fallback to default
        default()
    })
}
