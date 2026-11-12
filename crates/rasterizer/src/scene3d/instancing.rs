//! GPU Instancing and Audio-Reactive parameter buffers (TouchDesigner style).

use crate::mesh3d::{Quat, Vec3};
use crate::scene::Color;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

/// A single 3D transform and color instance for GPU instanced rendering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InstanceTransform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub color_override: Option<Color>,
}

impl Default for InstanceTransform {
    fn default() -> Self {
        Self {
            position: Vec3::default(),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(1.0, 1.0, 1.0),
            color_override: None,
        }
    }
}

impl InstanceTransform {
    pub const fn new(position: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self {
            position,
            rotation,
            scale,
            color_override: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color_override = Some(color);
        self
    }
}

/// Audio FFT frequency band modulation configuration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioModulationConfig {
    /// Starting FFT frequency bin index (0 = sub-bass).
    pub frequency_bin_start: usize,
    /// Ending FFT frequency bin index.
    pub frequency_bin_end: usize,
    /// Dynamic scale multiplier driven by audio energy.
    pub scale_multiplier: f32,
    /// Direction vector along which instances displace under audio beat pressure.
    pub displacement_axis: Vec3,
}

impl Default for AudioModulationConfig {
    fn default() -> Self {
        Self {
            frequency_bin_start: 0,
            frequency_bin_end: 16,
            scale_multiplier: 1.0,
            displacement_axis: Vec3::new(0.0, 1.0, 0.0),
        }
    }
}

/// Buffer containing instance transforms and optional audio-reactive modulation.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InstancingBuffer {
    /// Array of instance transforms.
    pub instances: Vec<InstanceTransform>,
    /// Optional audio FFT frequency reactive modulation.
    pub audio_modulation: Option<AudioModulationConfig>,
}

impl InstancingBuffer {
    pub fn new(instances: Vec<InstanceTransform>) -> Self {
        Self {
            instances,
            audio_modulation: None,
        }
    }

    pub fn with_audio_modulation(mut self, config: AudioModulationConfig) -> Self {
        self.audio_modulation = Some(config);
        self
    }

    /// Generates a 3D grid array of instances.
    pub fn grid_3d(nx: usize, ny: usize, nz: usize, spacing: f32) -> Self {
        let mut instances = Vec::with_capacity(nx * ny * nz);
        let hx = (nx as f32 - 1.0) * spacing * 0.5;
        let hy = (ny as f32 - 1.0) * spacing * 0.5;
        let hz = (nz as f32 - 1.0) * spacing * 0.5;

        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let pos = Vec3::new(
                        x as f32 * spacing - hx,
                        y as f32 * spacing - hy,
                        z as f32 * spacing - hz,
                    );
                    instances.push(InstanceTransform::new(
                        pos,
                        Quat::IDENTITY,
                        Vec3::new(1.0, 1.0, 1.0),
                    ));
                }
            }
        }
        Self::new(instances)
    }

    /// Generates a circular ring of instances.
    pub fn ring(count: usize, radius: f32) -> Self {
        let mut instances = Vec::with_capacity(count);
        let step = (2.0 * PI) / count.max(1) as f32;

        for i in 0..count {
            let theta = i as f32 * step;
            let pos = Vec3::new(radius * theta.cos(), 0.0, radius * theta.sin());
            let rot = Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), -theta);
            instances.push(InstanceTransform::new(pos, rot, Vec3::new(1.0, 1.0, 1.0)));
        }
        Self::new(instances)
    }

    /// Generates a uniform spherical point cloud using Fibonacci spiral lattice.
    pub fn fibonacci_sphere(count: usize, radius: f32) -> Self {
        let mut instances = Vec::with_capacity(count);
        let phi = PI * (3.0 - (5.0f32).sqrt()); // golden angle

        for i in 0..count {
            let y = 1.0 - (i as f32 / (count as f32 - 1.0).max(1.0)) * 2.0;
            let radius_at_y = (1.0 - y * y).max(0.0).sqrt();
            let theta = phi * i as f32;

            let x = theta.cos() * radius_at_y;
            let z = theta.sin() * radius_at_y;

            let pos = Vec3::new(x * radius, y * radius, z * radius);
            instances.push(InstanceTransform::new(
                pos,
                Quat::IDENTITY,
                Vec3::new(1.0, 1.0, 1.0),
            ));
        }
        Self::new(instances)
    }
}
