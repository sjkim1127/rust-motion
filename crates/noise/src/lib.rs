//! Rust Motion Noise — Pure-Rust procedural Simplex 2D/3D/4D noise generation,
//! Fractal Brownian Motion (fBm), turbulent flow domain warping, and animated organic noise backgrounds.
//!
//! Ported directly from Remotion's `@remotion/noise`:
//! - [`noise2d`], [`noise_2d`]
//! - [`noise3d`], [`noise_3d`]
//! - [`noise4d`], [`noise_4d`]
//! - [`fbm_2d`], [`fbm_3d`]
//! - [`turbulence_warp_2d`]
//! - [`mulberry32`], [`hash_code`], [`random`], [`NoiseSeed`]
//! - [`NoiseBackground`]

pub mod fbm;
pub mod noise_bg;
pub mod seed;
pub mod simplex;

pub use fbm::{
    domain_warp_2d, fbm_2d, fbm_2d_with_noise, fbm_3d, turbulence_2d, turbulence_warp_2d,
    warp_points_2d, FbmOptions,
};
pub use noise_bg::{
    generate_noise_svg_data_url, generate_noise_wave_path, NoiseBackground, NoiseBackgroundProps,
    NoisePatternKind, WavePathOptions,
};
pub use seed::{hash_code, hash_seed, mulberry32, random, seed_to_float, NoiseSeed};
pub use simplex::{noise2d, noise3d, noise4d, noise_2d, noise_3d, noise_4d, SimplexNoise};
