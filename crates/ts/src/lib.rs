//! Native TypeScript/Node.js bindings and 1:1 Remotion API compatibility bridge for Rust Motion.

pub mod animation;
pub mod bridge;

pub use animation::{
    evaluate_colors, evaluate_interpolate, evaluate_measure_spring, evaluate_random,
    evaluate_spring, ExtrapolateMode, InterpolateRequest, SpringConfigRequest, SpringRequest,
};
pub use bridge::{
    list_available_compositions, render_media_native, render_still_native, TsCompositionMeta,
    TsRenderProgress, TsRenderRequest, TsRenderResult, TsStillRequest, TsStillResult,
};

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// JSON-RPC command dispatcher for TypeScript/Node.js FFI and IPC.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "command", content = "params")]
pub enum TsCommand {
    #[serde(rename = "interpolate")]
    Interpolate(InterpolateRequest),
    #[serde(rename = "spring")]
    Spring(SpringRequest),
    #[serde(rename = "measure_spring")]
    MeasureSpring {
        fps: f64,
        config: SpringConfigRequest,
        threshold: Option<f64>,
    },
    #[serde(rename = "interpolate_colors")]
    InterpolateColors {
        input: f64,
        input_range: Vec<f64>,
        output_range: Vec<String>,
    },
    #[serde(rename = "random")]
    Random { seed: serde_json::Value },
    #[serde(rename = "list_compositions")]
    ListCompositions,
    #[serde(rename = "render_media")]
    RenderMedia(TsRenderRequest),
    #[serde(rename = "render_still")]
    RenderStill(TsStillRequest),
    #[serde(rename = "version")]
    Version,
}

#[derive(Debug, serde::Serialize)]
pub struct TsResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> TsResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

/// Process a JSON request string and return the JSON response string.
pub fn process_json_command(input: &str) -> String {
    let command: TsCommand = match serde_json::from_str(input) {
        Ok(cmd) => cmd,
        Err(e) => {
            let res: TsResponse<()> = TsResponse::err(format!("Invalid command JSON: {e}"));
            return serde_json::to_string(&res).unwrap_or_else(|_| "{}".to_string());
        }
    };

    match command {
        TsCommand::Interpolate(req) => {
            let val = evaluate_interpolate(&req);
            serde_json::to_string(&TsResponse::ok(val)).unwrap()
        }
        TsCommand::Spring(req) => match evaluate_spring(&req) {
            Ok(val) => serde_json::to_string(&TsResponse::ok(val)).unwrap(),
            Err(e) => serde_json::to_string(&TsResponse::<f64>::err(e)).unwrap(),
        },
        TsCommand::MeasureSpring {
            fps,
            config,
            threshold,
        } => match evaluate_measure_spring(fps, &config, threshold.unwrap_or(0.005)) {
            Ok(val) => serde_json::to_string(&TsResponse::ok(val)).unwrap(),
            Err(e) => serde_json::to_string(&TsResponse::<f64>::err(e)).unwrap(),
        },
        TsCommand::InterpolateColors {
            input,
            input_range,
            output_range,
        } => match evaluate_colors(input, &input_range, &output_range) {
            Ok(val) => serde_json::to_string(&TsResponse::ok(val)).unwrap(),
            Err(e) => serde_json::to_string(&TsResponse::<String>::err(e)).unwrap(),
        },
        TsCommand::Random { seed } => {
            let val = evaluate_random(&seed);
            serde_json::to_string(&TsResponse::ok(val)).unwrap()
        }
        TsCommand::ListCompositions => {
            let list = list_available_compositions();
            serde_json::to_string(&TsResponse::ok(list)).unwrap()
        }
        TsCommand::RenderMedia(req) => match render_media_native(req, None) {
            Ok(res) => serde_json::to_string(&TsResponse::ok(res)).unwrap(),
            Err(e) => serde_json::to_string(&TsResponse::<TsRenderResult>::err(e)).unwrap(),
        },
        TsCommand::RenderStill(req) => match render_still_native(req) {
            Ok(res) => serde_json::to_string(&TsResponse::ok(res)).unwrap(),
            Err(e) => serde_json::to_string(&TsResponse::<TsStillResult>::err(e)).unwrap(),
        },
        TsCommand::Version => {
            let version = env!("CARGO_PKG_VERSION");
            serde_json::to_string(&TsResponse::ok(version)).unwrap()
        }
    }
}

/// C-ABI FFI entry point for Node.js / Bun FFI.
///
/// # Safety
///
/// `json_ptr` must be a valid null-terminated C string pointer or null.
#[no_mangle]
pub unsafe extern "C" fn rust_motion_ts_dispatch(json_ptr: *const c_char) -> *mut c_char {
    if json_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let c_str = CStr::from_ptr(json_ptr);
    let input_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let result_json = process_json_command(input_str);
    match CString::new(result_json) {
        Ok(c_out) => c_out.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free memory allocated by `rust_motion_ts_dispatch`.
///
/// # Safety
///
/// `ptr` must be a pointer allocated by `rust_motion_ts_dispatch` or null.
#[no_mangle]
pub unsafe extern "C" fn rust_motion_ts_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}
