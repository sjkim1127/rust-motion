//! Tablet pointer input recording and stroke reconstruction.
//!
//! Captures live W3C PointerEvents (or stylus samples from platform monitors)
//! and converts them into deterministic [`photocraft_paint::Stroke`] instances
//! for write-on animation and reproducible rendering.

use photocraft_paint::{BrushSettings, Stroke, StrokePoint};
use serde::{Deserialize, Serialize};

use crate::Sample;

/// A live pointer or stylus event adhering to W3C Pointer Events standards.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerEvent {
    /// Document / viewport X coordinate in pixels.
    pub x: f64,
    /// Document / viewport Y coordinate in pixels.
    pub y: f64,
    /// Normalized pressure in `0.0..=1.0` (default 1.0 for mouse / non-pressure devices).
    #[serde(default = "default_pressure")]
    pub pressure: f32,
    /// Plane tilt angle along X axis in degrees `-90.0..=90.0`.
    #[serde(default)]
    pub tilt_x: f32,
    /// Plane tilt angle along Y axis in degrees `-90.0..=90.0`.
    #[serde(default)]
    pub tilt_y: f32,
    /// Barrel rotation / twist in degrees `0.0..=360.0`.
    #[serde(default)]
    pub rotation: f32,
    /// Optional tangential airbrush wheel `0.0..=1.0`.
    #[serde(default = "default_pressure")]
    pub wheel: f32,
    /// Timestamp in milliseconds.
    #[serde(default)]
    pub time_ms: f64,
}

fn default_pressure() -> f32 {
    1.0
}

impl Default for PointerEvent {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            pressure: 1.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            rotation: 0.0,
            wheel: 1.0,
            time_ms: 0.0,
        }
    }
}

impl PointerEvent {
    pub fn new(x: f64, y: f64, pressure: f32) -> Self {
        Self {
            x,
            y,
            pressure: pressure.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    pub fn with_tilt(mut self, tilt_x: f32, tilt_y: f32) -> Self {
        self.tilt_x = tilt_x.clamp(-90.0, 90.0);
        self.tilt_y = tilt_y.clamp(-90.0, 90.0);
        self
    }

    pub fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation.rem_euclid(360.0);
        self
    }

    pub fn with_time(mut self, time_ms: f64) -> Self {
        self.time_ms = time_ms;
        self
    }

    /// Construct from raw coordinates and a platform [`Sample`].
    pub fn from_sample(x: f64, y: f64, sample: Option<Sample>, time_ms: f64) -> Self {
        if let Some(s) = sample {
            let s = s.sanitized();
            Self {
                x,
                y,
                pressure: s.pressure,
                tilt_x: s.tilt_x,
                tilt_y: s.tilt_y,
                rotation: s.rotation,
                wheel: 1.0,
                time_ms,
            }
        } else {
            Self {
                x,
                y,
                pressure: 1.0,
                tilt_x: 0.0,
                tilt_y: 0.0,
                rotation: 0.0,
                wheel: 1.0,
                time_ms,
            }
        }
    }

    /// Convert to internal paint stroke point.
    pub fn to_stroke_point(&self) -> StrokePoint {
        StrokePoint {
            x: self.x,
            y: self.y,
            pressure: self.pressure,
            tilt_x: self.tilt_x,
            tilt_y: self.tilt_y,
            rotation: self.rotation,
            wheel: self.wheel,
            time: self.time_ms,
        }
    }
}

/// Records live tablet / stylus pointer events to generate a deterministic [`Stroke`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StrokeRecorder {
    /// Recorded points.
    pub points: Vec<StrokePoint>,
    /// Active recording state.
    pub is_recording: bool,
    /// Timestamp of first point in stroke.
    pub start_time_ms: Option<f64>,
}

impl StrokeRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a new stroke recording.
    pub fn start(&mut self) {
        self.points.clear();
        self.is_recording = true;
        self.start_time_ms = None;
    }

    /// Add a pointer event to the active recording.
    pub fn record_event(&mut self, event: &PointerEvent) {
        if !self.is_recording {
            return;
        }
        if self.start_time_ms.is_none() {
            self.start_time_ms = Some(event.time_ms);
        }
        self.points.push(event.to_stroke_point());
    }

    /// Record from coordinates, sample, and timestamp.
    pub fn record_sample(&mut self, x: f64, y: f64, sample: Option<Sample>, time_ms: f64) {
        let ev = PointerEvent::from_sample(x, y, sample, time_ms);
        self.record_event(&ev);
    }

    /// Complete recording and return the constructed stroke with the specified brush settings.
    pub fn finish(&mut self, brush: BrushSettings) -> Stroke {
        self.is_recording = false;
        Stroke {
            brush,
            points: self.points.clone(),
        }
    }

    /// Number of recorded points.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Total duration of the recorded stroke in seconds.
    pub fn duration_secs(&self) -> f64 {
        if self.points.len() < 2 {
            return 0.0;
        }
        let first = self.points.first().map(|p| p.time).unwrap_or(0.0);
        let last = self.points.last().map(|p| p.time).unwrap_or(0.0);
        ((last - first).max(0.0)) / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_event_from_sample() {
        let sample = Sample {
            pressure: 0.75,
            tilt_x: 25.0,
            tilt_y: -15.0,
            rotation: 120.0,
            eraser: false,
        };
        let ev = PointerEvent::from_sample(100.0, 200.0, Some(sample), 500.0);
        assert_eq!(ev.x, 100.0);
        assert_eq!(ev.y, 200.0);
        assert_eq!(ev.pressure, 0.75);
        assert_eq!(ev.tilt_x, 25.0);
        assert_eq!(ev.tilt_y, -15.0);
        assert_eq!(ev.rotation, 120.0);
        assert_eq!(ev.time_ms, 500.0);

        let pt = ev.to_stroke_point();
        assert_eq!(pt.x, 100.0);
        assert_eq!(pt.y, 200.0);
        assert_eq!(pt.pressure, 0.75);
    }

    #[test]
    fn test_stroke_recorder_flow() {
        let mut recorder = StrokeRecorder::new();
        recorder.start();
        recorder.record_sample(10.0, 10.0, None, 0.0);
        recorder.record_sample(20.0, 20.0, Some(Sample { pressure: 0.5, ..Default::default() }), 50.0);
        recorder.record_sample(30.0, 30.0, Some(Sample { pressure: 0.8, ..Default::default() }), 100.0);

        assert_eq!(recorder.point_count(), 3);
        assert!((recorder.duration_secs() - 0.1).abs() < 1e-4);

        let stroke = recorder.finish(BrushSettings::default());
        assert_eq!(stroke.points.len(), 3);
        assert_eq!(stroke.points[1].pressure, 0.5);
    }
}
