//! Small option enums shared by the filter parameters: [`Halo`], blur methods, noise distributions and distort modes.

use serde::{Deserialize, Serialize};

/// How far outside an output tile a filter reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halo {
    /// A margin of this many pixels.
    Radius(i32),
    /// The whole reference bounds (distortions, radial blur).
    Bounds,
}

/// Radial blur method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RadialMethod {
    #[default]
    Spin,
    Zoom,
}

/// Sampling quality for radial blur. Higher quality improves large-radius detail at greater cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RadialQuality {
    /// Up to 64 intervals per pixel for a faster, coarser result.
    Draft,
    /// Up to 256 intervals per pixel; the default.
    #[default]
    Good,
    /// Up to 4096 intervals per pixel.
    Best,
}

/// Noise distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Distribution {
    #[default]
    Uniform,
    Gaussian,
}

/// How pixels uncovered by a displacement are filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum UndefinedAreas {
    #[default]
    Wrap,
    Repeat,
    Transparent,
}

/// Minimum / Maximum › Preserve: the shape the filter grows or shrinks by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Preserve {
    /// A square of `2·radius + 1` pixels (keeps corners square).
    #[default]
    Squareness,
    /// A disc of `radius` pixels (rounds corners off evenly).
    Roundness,
}

/// Spherize mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SpherizeMode {
    #[default]
    Normal,
    HorizontalOnly,
    VerticalOnly,
}

/// Wave shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WaveType {
    #[default]
    Sine,
    Triangle,
    Square,
}

/// Ripple size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RippleSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// Polar coordinates direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PolarMode {
    #[default]
    RectangularToPolar,
    PolarToRectangular,
}
