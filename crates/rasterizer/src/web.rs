//! Protocol types for browser-backed composition workers.

use serde::{Deserialize, Serialize};

/// Deterministic request sent to a browser-backed renderer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebTimelineClip {
    pub id: String,
    pub composition: String,
    pub start: u32,
    pub duration: u32,
    #[serde(default)]
    pub props: serde_json::Value,
}

/// A named project frame marker forwarded to browser compositions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebTimeEvent {
    pub id: String,
    pub frame: u32,
}

/// Deterministic request sent to a browser-backed renderer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebFrameRequest {
    /// Composition selected by the host. Older workers may ignore this field.
    #[serde(default)]
    pub composition: Option<String>,
    pub frame: u32,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub props: serde_json::Value,
    /// Host-provided local or URL assets that the browser composition may preload.
    #[serde(default)]
    pub assets: Vec<String>,
    #[serde(default)]
    pub timeline: Vec<WebTimelineClip>,
    #[serde(default)]
    pub time_events: Vec<WebTimeEvent>,
    /// Browser screenshot transport. Defaults to PNG for lossless compatibility.
    #[serde(default)]
    pub image_format: Option<String>,
    #[serde(default)]
    pub jpeg_quality: Option<u8>,
    /// Preserve alpha for PNG screenshots instead of compositing a page background.
    #[serde(default)]
    pub transparent: bool,
    /// Optional binary file transport. The worker writes the encoded image to
    /// a temporary file and returns only its path in the JSON response.
    #[serde(default)]
    pub transport: Option<String>,
}

/// Result returned by a browser-backed renderer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebVideoFrame {
    pub width: u32,
    pub height: u32,
    /// WebCodecs timestamp in microseconds.
    pub timestamp_us: i64,
    /// Tightly packed top-to-bottom RGBA8 bytes.
    #[serde(default)]
    pub rgba_base64: String,
    /// Optional process-local raw RGBA file used by the `rgba_file` transport.
    #[serde(default)]
    pub file_path: Option<String>,
    /// Actual browser-side producer, for example `webcodecs` or
    /// `canvas-readback`. Optional for compatibility with older workers.
    #[serde(default)]
    pub transport: Option<String>,
}

/// Timing metadata attached to a browser-produced WebCodecs frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WebFrameTiming {
    pub timestamp_us: i64,
    pub timeline_frame: f64,
}

/// Aggregate drift measurements for a rendered output-frame sequence.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WebFrameDriftReport {
    pub sample_count: usize,
    pub mean_abs_drift_frames: f64,
    pub max_abs_drift_frames: f64,
    /// Number of adjacent samples whose requested output frames are not
    /// consecutive. This catches dropped or duplicated scheduler requests.
    pub non_contiguous_samples: usize,
}

impl WebFrameDriftReport {
    pub fn from_samples(samples: &[(u32, WebFrameTiming)]) -> Option<Self> {
        let first = samples.first()?;
        let mut total_abs_drift = first.1.drift_frames(first.0).abs();
        let mut max_abs_drift = total_abs_drift;
        let mut non_contiguous_samples = 0;
        let mut previous_frame = first.0;
        for &(output_frame, timing) in &samples[1..] {
            let abs_drift = timing.drift_frames(output_frame).abs();
            total_abs_drift += abs_drift;
            max_abs_drift = max_abs_drift.max(abs_drift);
            if output_frame != previous_frame.saturating_add(1) {
                non_contiguous_samples += 1;
            }
            previous_frame = output_frame;
        }
        Some(Self {
            sample_count: samples.len(),
            mean_abs_drift_frames: total_abs_drift / samples.len() as f64,
            max_abs_drift_frames: max_abs_drift,
            non_contiguous_samples,
        })
    }

    /// Serialize a stable machine-readable validation artifact.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serialize one CSV row matching [`Self::csv_header`].
    pub fn csv_header() -> &'static str {
        "sample_count,mean_abs_drift_frames,max_abs_drift_frames,non_contiguous_samples"
    }

    pub fn to_csv_row(&self) -> String {
        format!(
            "{},{:.9},{:.9},{}",
            self.sample_count,
            self.mean_abs_drift_frames,
            self.max_abs_drift_frames,
            self.non_contiguous_samples
        )
    }
}

impl WebFrameTiming {
    pub fn from_timestamp(timestamp_us: i64, fps: f64) -> Option<Self> {
        if !fps.is_finite() || fps <= 0.0 {
            return None;
        }
        Some(Self {
            timestamp_us,
            timeline_frame: timestamp_us as f64 / 1_000_000.0 * fps,
        })
    }

    /// Difference between the media presentation position and the requested
    /// output frame, expressed in output frames.
    pub fn drift_frames(&self, output_frame: u32) -> f64 {
        self.timeline_frame - output_frame as f64
    }
}

impl WebVideoFrame {
    /// Return the WebCodecs presentation timestamp in seconds.
    pub fn timestamp_seconds(&self) -> f64 {
        self.timestamp_us as f64 / 1_000_000.0
    }

    /// Map the presentation timestamp onto the composition timeline.
    ///
    /// The result is intentionally fractional: callers that need a discrete
    /// output frame must choose an explicit rounding policy at the scheduling
    /// boundary instead of silently changing the media timestamp here.
    pub fn timeline_frame(&self, fps: f64) -> Option<f64> {
        if !fps.is_finite() || fps <= 0.0 {
            return None;
        }
        Some(self.timestamp_seconds() * fps)
    }

    /// Return the required tightly packed RGBA8 payload size, or `None` when
    /// the dimensions overflow a host `usize`.
    pub fn expected_rgba_bytes(&self) -> Option<usize> {
        usize::try_from(self.width)
            .ok()
            .and_then(|width| {
                usize::try_from(self.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebFrameResponse {
    pub frame: u32,
    pub width: u32,
    pub height: u32,
    /// Preferred transport: the browser's encoded PNG screenshot.
    #[serde(default)]
    pub png_base64: Option<String>,
    #[serde(default)]
    pub jpeg_base64: Option<String>,
    /// Legacy transport retained for older workers.
    #[serde(default)]
    pub rgba_base64: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    /// Optional raw WebCodecs frame transport for native consumers.
    #[serde(default)]
    pub video_frame: Option<WebVideoFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebWorkerMessage {
    Ready {
        protocol: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        compositions: Vec<String>,
    },
    Render(WebFrameRequest),
    Frame(WebFrameResponse),
    Error {
        frame: Option<u32>,
        message: String,
    },
    Shutdown,
}

pub const WEB_WORKER_PROTOCOL_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_request_round_trips_as_json() {
        let message = WebWorkerMessage::Render(WebFrameRequest {
            composition: Some("demo".into()),
            frame: 12,
            fps: 30.0,
            width: 1280,
            height: 720,
            props: serde_json::json!({"seed": 7}),
            assets: vec![],
            timeline: vec![],
            time_events: vec![],
            image_format: None,
            jpeg_quality: None,
            transparent: true,
            transport: None,
        });
        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serde_json::from_str::<WebWorkerMessage>(&json).unwrap(),
            message
        );
    }

    #[test]
    fn protocol_ready_message_is_stable() {
        let json = serde_json::to_string(&WebWorkerMessage::Ready {
            protocol: WEB_WORKER_PROTOCOL_VERSION,
            compositions: vec![],
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"ready","protocol":1}"#);
    }

    #[test]
    fn frame_response_accepts_legacy_rgba_transport() {
        let message = r#"{"type":"frame","frame":3,"width":1,"height":1,"rgba_base64":"AQIDBA=="}"#;
        let parsed: WebWorkerMessage = serde_json::from_str(message).unwrap();
        assert!(matches!(parsed, WebWorkerMessage::Frame(response)
            if response.png_base64.is_none() && response.rgba_base64.as_deref() == Some("AQIDBA==")));
    }

    #[test]
    fn frame_response_accepts_jpeg_transport() {
        let message = r#"{"type":"frame","frame":3,"width":1,"height":1,"jpeg_base64":"AQIDBA=="}"#;
        let parsed: WebWorkerMessage = serde_json::from_str(message).unwrap();
        assert!(matches!(parsed, WebWorkerMessage::Frame(response)
            if response.jpeg_base64.as_deref() == Some("AQIDBA==")));
    }

    #[test]
    fn frame_response_accepts_webcodecs_video_frame_transport() {
        let message = r#"{"type":"frame","frame":3,"width":1,"height":1,"video_frame":{"width":1,"height":1,"timestamp_us":1250000,"rgba_base64":"AQIDBA==","transport":"webcodecs"}}"#;
        let parsed: WebWorkerMessage = serde_json::from_str(message).unwrap();
        assert!(matches!(parsed, WebWorkerMessage::Frame(response)
            if response.video_frame.as_ref().is_some_and(|frame| frame.timestamp_us == 1_250_000
                && frame.transport.as_deref() == Some("webcodecs"))));
    }

    #[test]
    fn webcodecs_timestamp_maps_to_fractional_timeline_frame() {
        let frame = WebVideoFrame {
            width: 1,
            height: 1,
            timestamp_us: 1_250_000,
            rgba_base64: String::new(),
            file_path: None,
            transport: None,
        };
        assert_eq!(frame.timestamp_seconds(), 1.25);
        assert_eq!(frame.timeline_frame(30.0), Some(37.5));

        let ntsc = WebVideoFrame {
            timestamp_us: 1_000_000,
            ..frame
        };
        assert!((ntsc.timeline_frame(23.976).unwrap() - 23.976).abs() < 1e-12);
    }

    #[test]
    fn webcodecs_timestamp_rejects_invalid_timeline_fps() {
        let frame = WebVideoFrame {
            width: 1,
            height: 1,
            timestamp_us: 1_000_000,
            rgba_base64: String::new(),
            file_path: None,
            transport: None,
        };
        assert_eq!(frame.timeline_frame(0.0), None);
        assert_eq!(frame.timeline_frame(f64::NAN), None);
        assert_eq!(frame.timeline_frame(f64::INFINITY), None);
    }

    #[test]
    fn webcodecs_timing_reports_subframe_drift() {
        let timing = WebFrameTiming::from_timestamp(133_333, 30.0).unwrap();
        assert!((timing.drift_frames(4) + 0.00001).abs() < 1e-6);
    }

    #[test]
    fn webcodecs_drift_report_aggregates_sequence_and_gaps() {
        let samples = [
            (3, WebFrameTiming::from_timestamp(100_000, 30.0).unwrap()),
            (4, WebFrameTiming::from_timestamp(133_333, 30.0).unwrap()),
            (6, WebFrameTiming::from_timestamp(200_000, 30.0).unwrap()),
        ];
        let report = WebFrameDriftReport::from_samples(&samples).unwrap();
        assert_eq!(report.sample_count, 3);
        assert_eq!(report.non_contiguous_samples, 1);
        assert!(report.max_abs_drift_frames < 1e-5);
        assert!(report.mean_abs_drift_frames < 1e-5);
        assert_eq!(WebFrameDriftReport::from_samples(&[]), None);
        assert!(report.to_json().unwrap().contains("\"sample_count\": 3"));
        assert_eq!(report.to_csv_row(), "3,0.000003333,0.000010000,1");
    }

    #[test]
    fn webcodecs_rgba_size_is_checked_with_overflow_safety() {
        let frame = WebVideoFrame {
            width: 16,
            height: 8,
            timestamp_us: 0,
            rgba_base64: String::new(),
            file_path: None,
            transport: None,
        };
        assert_eq!(frame.expected_rgba_bytes(), Some(512));
        let overflowing = WebVideoFrame {
            width: u32::MAX,
            height: u32::MAX,
            timestamp_us: 0,
            rgba_base64: String::new(),
            file_path: None,
            transport: None,
        };
        assert_eq!(overflowing.expected_rgba_bytes(), None);
    }
}
