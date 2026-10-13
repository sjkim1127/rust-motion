use rust_motion_animation::{
    measure_spring, spring, spring_with_options, SpringConfig, SpringError, SpringOptions,
};

fn number(s: &str) -> f64 {
    s.parse().unwrap()
}

fn config(row: &[&str]) -> SpringConfig {
    SpringConfig {
        damping: number(row[1]),
        mass: number(row[2]),
        stiffness: number(row[3]),
        overshoot_clamping: row[4] == "true",
    }
}

#[test]
fn matches_vendored_remotion_samples() {
    for (line, text) in include_str!("fixtures/remotion_spring.tsv")
        .lines()
        .enumerate()
    {
        if text.starts_with('#') {
            continue;
        }
        let row: Vec<_> = text.split('\t').collect();
        let options = SpringOptions {
            from: number(row[6]),
            to: number(row[7]),
            duration_in_frames: if row[8].is_empty() {
                None
            } else {
                Some(number(row[8]))
            },
            duration_rest_threshold: number(row[9]),
            delay: number(row[10]),
            reverse: row[11] == "true",
        };
        let actual =
            spring_with_options(number(row[5]), number(row[0]), config(&row), options).unwrap();
        let expected = number(row[12]);
        assert!(
            (actual - expected).abs() < 1e-9,
            "fixture line {}: expected {expected}, got {actual}",
            line + 1
        );
    }
}

#[test]
fn matches_vendored_remotion_settling_times() {
    for text in include_str!("fixtures/remotion_measure_spring.tsv").lines() {
        if text.starts_with('#') {
            continue;
        }
        let row: Vec<_> = text.split('\t').collect();
        assert_eq!(
            measure_spring(number(row[0]), &config(&row), number(row[5])).unwrap(),
            number(row[6]),
            "{text}"
        );
    }
}

#[test]
fn legacy_api_keeps_integer_frame_values() {
    for frame in 0..120 {
        let config = SpringConfig::default();
        assert_eq!(
            spring(frame, 30.0, config.clone()),
            spring_with_options(frame as f64, 30.0, config, SpringOptions::default()).unwrap()
        );
    }
}

#[test]
fn overshoot_clamping_respects_both_range_directions() {
    for (from, to) in [(100.0, 200.0), (200.0, 100.0), (-50.0, -100.0)] {
        for frame in 0..60 {
            let value = spring_with_options(
                frame as f64,
                30.0,
                SpringConfig {
                    overshoot_clamping: true,
                    ..Default::default()
                },
                SpringOptions {
                    from,
                    to,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(value >= from.min(to) && value <= from.max(to));
            if frame == 0 {
                assert_eq!(value, from);
            }
        }
    }
}

#[test]
fn duration_and_reverse_endpoints_include_delay() {
    for reverse in [false, true] {
        let options = SpringOptions {
            from: 100.0,
            to: -100.0,
            delay: 12.5,
            duration_in_frames: Some(10.0),
            reverse,
            ..Default::default()
        };
        let at = |frame| {
            spring_with_options(frame, 30.0, SpringConfig::default(), options.clone()).unwrap()
        };
        assert_eq!(at(0.0), if reverse { -100.0 } else { 100.0 });
        assert_eq!(at(30.0), if reverse { 100.0 } else { -100.0 });
        let shifted = SpringOptions {
            delay: 0.0,
            ..options
        };
        assert_eq!(
            at(17.5),
            spring_with_options(5.0, 30.0, SpringConfig::default(), shifted).unwrap()
        );
    }
}

#[test]
fn invalid_inputs_return_errors_without_unbounded_work() {
    let default = SpringConfig::default();
    for fps in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            measure_spring(fps, &default, 0.005),
            Err(SpringError::InvalidFps)
        );
    }
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        for config in [
            SpringConfig {
                damping: value,
                ..default.clone()
            },
            SpringConfig {
                mass: value,
                ..default.clone()
            },
            SpringConfig {
                stiffness: value,
                ..default.clone()
            },
        ] {
            assert_eq!(
                measure_spring(30.0, &config, 0.005),
                Err(SpringError::InvalidConfig)
            );
        }
    }
    for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            spring_with_options(
                1.0,
                30.0,
                default.clone(),
                SpringOptions {
                    duration_in_frames: Some(duration),
                    ..Default::default()
                }
            ),
            Err(SpringError::InvalidDuration)
        );
    }
    for threshold in [0.0, 1.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            spring_with_options(
                1.0,
                30.0,
                default.clone(),
                SpringOptions {
                    reverse: true,
                    duration_rest_threshold: threshold,
                    ..Default::default()
                }
            ),
            Err(SpringError::InvalidThreshold)
        );
    }
    assert_eq!(measure_spring(30.0, &default, 0.0).unwrap(), f64::INFINITY);
    assert_eq!(measure_spring(30.0, &default, 1.0).unwrap(), 0.0);
    assert_eq!(
        spring_with_options(f64::NAN, 30.0, default.clone(), SpringOptions::default()),
        Err(SpringError::InvalidFrame)
    );
    assert_eq!(
        spring_with_options(1e12, 30.0, default.clone(), SpringOptions::default()),
        Err(SpringError::CalculationLimit)
    );
    assert_eq!(
        measure_spring(
            30.0,
            &SpringConfig {
                damping: 1e-12,
                ..default
            },
            0.005
        ),
        Err(SpringError::CalculationLimit)
    );
}

#[test]
fn numerical_overflow_is_reported_instead_of_becoming_a_valid_frame() {
    let run = |frame, options| spring_with_options(frame, 30.0, SpringConfig::default(), options);
    assert_eq!(
        run(
            0.0,
            SpringOptions {
                duration_in_frames: Some(f64::from_bits(1)),
                ..Default::default()
            }
        ),
        Err(SpringError::NonFiniteResult)
    );
    assert_eq!(
        run(
            f64::MAX,
            SpringOptions {
                delay: -f64::MAX,
                ..Default::default()
            }
        ),
        Err(SpringError::NonFiniteResult)
    );
    assert_eq!(
        run(
            0.0,
            SpringOptions {
                from: -f64::MAX,
                to: f64::MAX,
                ..Default::default()
            }
        ),
        Err(SpringError::InvalidRange)
    );
    assert_eq!(
        run(
            0.0,
            SpringOptions {
                delay: f64::NAN,
                ..Default::default()
            }
        ),
        Err(SpringError::InvalidDelay)
    );
}
