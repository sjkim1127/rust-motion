//! `spring()` — Rust port of Remotion's spring animation.
//!
//! Computes a physically-based spring animation value at a given frame.
//! Matches Remotion's `spring-utils.ts` using constant-time frame evaluation
//! and bounded, thread-local settling-duration caches.
//!
//! # Example
//! ```rust
//! use rust_motion_animation::spring::{spring, SpringConfig};
//!
//! let config = SpringConfig::default(); // damping=10, mass=1, stiffness=100
//! let value = spring(15, 30.0, config);
//! println!("frame 15 spring value: {value:.4}");
//! ```

/// Configuration for the spring physics model.
///
/// Defaults match Remotion's `spring()` defaults exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct SpringConfig {
    /// Resistance force — higher = less oscillation. Default: `10`.
    pub damping: f64,
    /// Object mass — higher = slower. Default: `1`.
    pub mass: f64,
    /// Spring tension — higher = faster. Default: `100`.
    pub stiffness: f64,
    /// If `true`, clamps overshoot to `[from, to]`. Default: `false`.
    pub overshoot_clamping: bool,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self {
            damping: 10.0,
            mass: 1.0,
            stiffness: 100.0,
            overshoot_clamping: false,
        }
    }
}

#[derive(Debug, Clone)]
struct AnimationNode {
    last_timestamp: f64,
    to_value: f64,
    current: f64,
    velocity: f64,
}

/// Closed-form oscillator response from Remotion's `advance()`.
/// Measurement steps retain its 64ms cap; direct evaluation supplies the
/// equivalent total elapsed time.
fn advance(node: &AnimationNode, now: f64, config: &SpringConfig, cap_step: bool) -> AnimationNode {
    let AnimationNode {
        to_value,
        last_timestamp,
        current,
        velocity,
        ..
    } = *node;

    let delta_time = now - last_timestamp;
    let delta_time = if cap_step {
        delta_time.min(64.0)
    } else {
        delta_time
    };

    let c = config.damping;
    let m = config.mass;
    let k = config.stiffness;

    let v0 = -velocity;
    let x0 = to_value - current;

    let zeta = c / (2.0 * (k * m).sqrt()); // damping ratio
    let omega0 = (k / m).sqrt(); // undamped angular frequency (rad/ms)
    let omega1 = omega0 * (1.0 - zeta.powi(2)).sqrt(); // exponential decay frequency

    let t = delta_time / 1000.0;

    let sin1 = (omega1 * t).sin();
    let cos1 = (omega1 * t).cos();

    // ── Under-damped (zeta < 1) ───────────────────────────────────────────────
    let under_damped_envelope = (-zeta * omega0 * t).exp();
    let under_damped_frag1 =
        under_damped_envelope * (sin1 * ((v0 + zeta * omega0 * x0) / omega1) + x0 * cos1);

    let under_damped_position = to_value - under_damped_frag1;
    let under_damped_velocity = zeta * omega0 * under_damped_frag1
        - under_damped_envelope * (cos1 * (v0 + zeta * omega0 * x0) - omega1 * x0 * sin1);

    // ── Critically damped (zeta >= 1) ─────────────────────────────────────────
    let critically_damped_envelope = (-omega0 * t).exp();
    let critically_damped_position =
        to_value - critically_damped_envelope * (x0 + (v0 + omega0 * x0) * t);
    let critically_damped_velocity =
        critically_damped_envelope * (v0 * (t * omega0 - 1.0) + t * x0 * omega0 * omega0);

    let (new_current, new_velocity) = if zeta < 1.0 {
        (under_damped_position, under_damped_velocity)
    } else {
        (critically_damped_position, critically_damped_velocity)
    };

    AnimationNode {
        to_value,
        last_timestamp: now,
        current: new_current,
        velocity: new_velocity,
    }
}

/// Compute the spring value at `frame` (0-indexed).
///
/// # Arguments
/// * `frame` — current frame number
/// * `fps`   — frames per second of the composition
/// * `config` — spring physics configuration
///
/// Returns a value interpolated between `0.0` (start) and `1.0` (settled).
/// Use [`crate::interpolate::interpolate`] to map this to any output range.
///
/// # Notes
/// Uses the same spring model as Remotion's `springCalculation()` from `spring-utils.ts`.
/// Valid inputs retain the original integer-frame behavior.
///
/// # Panics
/// Panics for invalid parameters or a requested frame beyond 100,000.
/// Use [`spring_with_options`] to handle errors explicitly.
pub fn spring(frame: u32, fps: f64, config: SpringConfig) -> f64 {
    spring_with_options(frame as f64, fps, config, SpringOptions::default())
        .expect("invalid spring parameters")
}

/// Playback controls for [`spring_with_options`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpringOptions {
    pub from: f64,
    pub to: f64,
    /// Stretch the natural settling duration to this positive number of frames.
    pub duration_in_frames: Option<f64>,
    /// Distance from 1 used to measure settling. Default: 0.005.
    pub duration_rest_threshold: f64,
    /// Offset in frames, applied before duration scaling. May be fractional.
    pub delay: f64,
    /// Play the spring backwards, preserving the delay before playback.
    pub reverse: bool,
}

impl Default for SpringOptions {
    fn default() -> Self {
        Self {
            from: 0.0,
            to: 1.0,
            duration_in_frames: None,
            duration_rest_threshold: 0.005,
            delay: 0.0,
            reverse: false,
        }
    }
}

/// Invalid input or a spring that cannot be evaluated within the work limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpringError {
    InvalidFps,
    InvalidConfig,
    InvalidFrame,
    InvalidRange,
    InvalidDelay,
    InvalidDuration,
    InvalidThreshold,
    CalculationLimit,
    NonFiniteResult,
}

impl std::fmt::Display for SpringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidFps => "spring fps must be finite and positive",
            Self::InvalidConfig => "spring damping, mass and stiffness must be finite and positive",
            Self::InvalidFrame => "spring frame must be finite",
            Self::InvalidRange => "spring endpoints and their difference must be finite",
            Self::InvalidDelay => "spring delay must be finite",
            Self::InvalidDuration => "spring duration must be finite and positive",
            Self::InvalidThreshold => "spring threshold must be finite and nonnegative; timing requires a value strictly between 0 and 1",
            Self::CalculationLimit => "spring calculation exceeded 100000 simulation frames",
            Self::NonFiniteResult => "spring calculation produced a non-finite value",
        })
    }
}

impl std::error::Error for SpringError {}

const MAX_SIMULATION_FRAMES: u32 = 100_000;

fn validate(fps: f64, config: &SpringConfig) -> Result<(), SpringError> {
    if !fps.is_finite() || fps <= 0.0 {
        return Err(SpringError::InvalidFps);
    }
    if [config.damping, config.mass, config.stiffness]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err(SpringError::InvalidConfig);
    }
    Ok(())
}

fn initial_node() -> AnimationNode {
    AnimationNode {
        last_timestamp: 0.0,
        current: 0.0,
        to_value: 1.0,
        velocity: 0.0,
    }
}

fn checked_advance(
    node: &AnimationNode,
    frame: f64,
    fps: f64,
    config: &SpringConfig,
) -> Result<AnimationNode, SpringError> {
    let next = advance(node, (frame / fps) * 1000.0, config, true);
    if !next.current.is_finite() || !next.velocity.is_finite() || !next.last_timestamp.is_finite() {
        return Err(SpringError::NonFiniteResult);
    }
    Ok(next)
}

fn calculate(frame: f64, fps: f64, config: &SpringConfig) -> Result<f64, SpringError> {
    if !frame.is_finite() {
        return Err(SpringError::NonFiniteResult);
    }
    let frame = frame.max(0.0);
    if frame > MAX_SIMULATION_FRAMES as f64 {
        return Err(SpringError::CalculationLimit);
    }
    // The oscillator has a closed-form solution. Repeated advances compose
    // into one elapsed time, including Remotion's per-step 64ms cap. Its final
    // fractional step skips the integer frame and can span up to two frames.
    let preceding_frame = (frame.floor() - 1.0).max(0.0);
    let preceding_timestamp = (preceding_frame / fps) * 1000.0;
    let timestamp = (frame / fps) * 1000.0;
    if !timestamp.is_finite() {
        return Err(SpringError::NonFiniteResult);
    }
    let elapsed_prefix = if 1000.0 / fps > 64.0 {
        preceding_frame * 64.0
    } else {
        preceding_timestamp
    };
    let elapsed = elapsed_prefix + (timestamp - preceding_timestamp).min(64.0);
    let node = advance(&initial_node(), elapsed, config, false);
    if node.current.is_finite() && node.velocity.is_finite() {
        Ok(node.current)
    } else {
        Err(SpringError::NonFiniteResult)
    }
}

/// Measure natural settling time using Remotion 4.0.495's 20-frame rest window.
///
/// The default threshold used by [`SpringOptions`] is `0.005` (28 frames at
/// 30 fps with the default config). Overshoot clamping does not affect measurement.
/// Threshold 0 returns infinity; thresholds >= 1 return 0. Invalid physics and
/// simulations exceeding 100,000 frames return an error instead of hanging.
pub fn measure_spring(fps: f64, config: &SpringConfig, threshold: f64) -> Result<f64, SpringError> {
    validate(fps, config)?;
    if !threshold.is_finite() || threshold < 0.0 {
        return Err(SpringError::InvalidThreshold);
    }
    if threshold == 0.0 {
        return Ok(f64::INFINITY);
    }
    if threshold >= 1.0 {
        return Ok(0.0);
    }

    let key = [
        fps.to_bits(),
        config.damping.to_bits(),
        config.mass.to_bits(),
        config.stiffness.to_bits(),
        threshold.to_bits(),
    ];
    MEASURE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, value)) = cache.iter().find(|(cached_key, _)| *cached_key == key) {
            return *value;
        }
        let result = measure_uncached(fps, config, threshold);
        // A render worker retains at most 64 measurements. Avoid global locks
        // and unbounded growth when projects use many animated configurations.
        if cache.len() == MEASURE_CACHE_CAPACITY {
            cache.pop_back();
        }
        cache.push_front((key, result));
        result
    })
}

const MEASURE_CACHE_CAPACITY: usize = 64;
type MeasureCache = std::collections::VecDeque<([u64; 5], Result<f64, SpringError>)>;
thread_local! {
    static MEASURE_CACHE: std::cell::RefCell<MeasureCache> = const {
        std::cell::RefCell::new(std::collections::VecDeque::new())
    };
}

fn measure_uncached(fps: f64, config: &SpringConfig, threshold: f64) -> Result<f64, SpringError> {
    // Advance incrementally once; subsequent calls reuse the measurement.
    let mut node = checked_advance(&initial_node(), 0.0, fps, config)?;
    let mut frame = 0;
    while (node.current - 1.0).abs() >= threshold {
        frame += 1;
        if frame > MAX_SIMULATION_FRAMES {
            return Err(SpringError::CalculationLimit);
        }
        node = checked_advance(&node, frame as f64, fps, config)?;
    }
    let mut finished_frame = frame;
    let mut rest = 0;
    while rest < 20 {
        frame += 1;
        if frame > MAX_SIMULATION_FRAMES {
            return Err(SpringError::CalculationLimit);
        }
        node = checked_advance(&node, frame as f64, fps, config)?;
        if (node.current - 1.0).abs() >= threshold {
            // Match upstream's reset followed by the for-loop increment.
            rest = 0;
            finished_frame = frame + 1;
        }
        rest += 1;
    }
    Ok(finished_frame as f64)
}

/// Evaluate a spring with fractional/negative frames, range, delay and reversal.
///
/// Duration scaling and reversal follow Remotion 4.0.495. Negative local frames
/// hold `from`; frames beyond an explicit duration hold `to`. Reversed playback
/// traverses that same curve backwards. At the duration itself the value is the
/// measured settling value, not a forced endpoint.
///
/// Unlike upstream's non-unit-range clamping, `overshoot_clamping` clamps the
/// normalized spring before mapping it into `[from, to]`, including descending
/// ranges. Duration/reverse require a rest threshold strictly between 0 and 1.
/// Frame evaluation takes constant time. Settling measurements are cached per
/// thread (at most 64 entries) and bounded to 100,000 simulation frames.
/// The supported adjusted frame range remains capped at 100,000 frames.
///
/// ```
/// use rust_motion_animation::{spring_with_options, SpringConfig, SpringOptions};
/// let options = SpringOptions {
///     from: 100.0,
///     to: 0.0,
///     delay: 10.0,
///     duration_in_frames: Some(30.0),
///     ..Default::default()
/// };
/// assert_eq!(spring_with_options(10.0, 30.0, SpringConfig::default(), options)?, 100.0);
/// # Ok::<(), rust_motion_animation::SpringError>(())
/// ```
pub fn spring_with_options(
    frame: f64,
    fps: f64,
    config: SpringConfig,
    options: SpringOptions,
) -> Result<f64, SpringError> {
    validate(fps, &config)?;
    if !frame.is_finite() {
        return Err(SpringError::InvalidFrame);
    }
    if !options.from.is_finite()
        || !options.to.is_finite()
        || !(options.to - options.from).is_finite()
    {
        return Err(SpringError::InvalidRange);
    }
    if !options.delay.is_finite() {
        return Err(SpringError::InvalidDelay);
    }
    if options
        .duration_in_frames
        .is_some_and(|d| !d.is_finite() || d <= 0.0)
    {
        return Err(SpringError::InvalidDuration);
    }
    if !options.duration_rest_threshold.is_finite() || options.duration_rest_threshold < 0.0 {
        return Err(SpringError::InvalidThreshold);
    }
    let natural_duration = if options.reverse || options.duration_in_frames.is_some() {
        if options.duration_rest_threshold <= 0.0 || options.duration_rest_threshold >= 1.0 {
            return Err(SpringError::InvalidThreshold);
        }
        measure_spring(fps, &config, options.duration_rest_threshold)?
    } else {
        0.0
    };
    let delayed = if options.reverse {
        options.duration_in_frames.unwrap_or(natural_duration) - frame + options.delay
    } else {
        frame - options.delay
    };
    if let Some(duration) = options.duration_in_frames {
        if delayed > duration {
            return Ok(options.to);
        }
    }
    let adjusted = match options.duration_in_frames {
        Some(duration) => delayed / (duration / natural_duration),
        None => delayed,
    };
    let value = calculate(adjusted, fps, &config)?;
    let value = if config.overshoot_clamping {
        value.clamp(0.0, 1.0)
    } else {
        value
    };
    let result = options.from + (options.to - options.from) * value;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(SpringError::NonFiniteResult)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_matches_iterative_physics_including_capped_fractional_steps() {
        for fps in [1.0, 15.0, 15.625, 30.0, 59.94, 120.0] {
            for damping in [0.01, 6.0, 10.0, 20.0, 200.0] {
                for mass in [0.5, 1.0, 2.0] {
                    for stiffness in [30.0, 100.0, 200.0] {
                        let config = SpringConfig {
                            damping,
                            mass,
                            stiffness,
                            ..Default::default()
                        };
                        for frame in [
                            0.0_f64, 0.2, 0.99, 1.0, 1.9, 5.99, 12.999, 60.5, 300.25, 999.9,
                        ] {
                            let floor = frame.floor() as u32;
                            let mut node = initial_node();
                            for f in 0..=floor {
                                let f = if f == floor { frame } else { f as f64 };
                                node = checked_advance(&node, f, fps, &config).unwrap();
                            }
                            let actual = calculate(frame, fps, &config).unwrap();
                            assert!((actual - node.current).abs() < 1e-9,
                                "fps={fps} damping={damping} mass={mass} stiffness={stiffness} frame={frame}: {actual} != {}", node.current);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn measurement_cache_is_bounded_and_keys_all_physics_inputs() {
        // The cache belongs to each worker; arbitrary seek order must not affect values.
        std::thread::spawn(|| {
            for index in 0..80 {
                let fps = 30.0 + (index % 3) as f64;
                let threshold = 0.005 + (index % 4) as f64 * 0.001;
                let config = SpringConfig {
                    damping: 6.0 + index as f64 * 0.1,
                    mass: 1.0 + (index % 5) as f64 * 0.1,
                    stiffness: 80.0 + index as f64,
                    ..Default::default()
                };
                let expected = measure_uncached(fps, &config, threshold).unwrap();
                assert_eq!(measure_spring(fps, &config, threshold).unwrap(), expected);
                assert_eq!(measure_spring(fps, &config, threshold).unwrap(), expected);
                MEASURE_CACHE.with(|cache| assert!(cache.borrow().len() <= MEASURE_CACHE_CAPACITY));
            }
            let config = SpringConfig::default();
            assert_eq!(measure_spring(30.0, &config, 0.005).unwrap(), 28.0);
            assert_eq!(
                measure_spring(
                    30.0,
                    &SpringConfig {
                        overshoot_clamping: true,
                        ..config
                    },
                    0.005
                )
                .unwrap(),
                28.0
            );
        })
        .join()
        .unwrap();
    }

    /// Verify the spring settles to 1.0 and starts at 0.0.
    #[test]
    fn settles_to_one() {
        let config = SpringConfig::default();
        // By frame 60 (2s at 30fps) a default spring should be settled within 0.001 of 1.0
        let v = spring(60, 30.0, config);
        assert!(
            (v - 1.0).abs() < 0.001,
            "frame 60 should be ~1.0, got {v:.6}"
        );
    }

    #[test]
    fn increases_monotonically_early() {
        let config = SpringConfig::default();
        // The spring should increase for the first few frames
        let v0 = spring(0, 30.0, config.clone());
        let v1 = spring(1, 30.0, config.clone());
        let v3 = spring(3, 30.0, config.clone());
        assert!(v1 > v0, "v1({v1:.4}) should be > v0({v0:.4})");
        assert!(v3 > v1, "v3({v3:.4}) should be > v1({v1:.4})");
    }

    #[test]
    fn overshoot_clamping() {
        let config = SpringConfig {
            overshoot_clamping: true,
            ..Default::default()
        };
        // Frame 10 normally overshoots slightly above 1.0
        let v = spring(10, 30.0, config);
        assert!(v <= 1.0, "value {v} should be clamped to <= 1.0");
    }

    #[test]
    fn frame_zero_is_zero() {
        let v = spring(0, 30.0, SpringConfig::default());
        assert!((v - 0.0).abs() < 1e-6, "got {v}");
    }
}
