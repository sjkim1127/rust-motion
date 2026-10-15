use rust_motion_animation::keyframe::{
    InterpolationType, Keyframe, KeyframeEase, KeyframeTrack,
};

#[test]
fn test_ae_speed_graph_asymmetric_influence() {
    let mut track = KeyframeTrack::new(0.0f64, 30.0);
    // Asymmetric ease: Launch fast (influence 15%), brake hard (influence 85%)
    let out_ease = KeyframeEase::new(0.0, 15.0);
    let in_ease = KeyframeEase::new(0.0, 85.0);

    track.insert_keyframe(Keyframe::bezier(0.0, 0.0, KeyframeEase::easy_ease(), out_ease));
    track.insert_keyframe(Keyframe::bezier(30.0, 100.0, in_ease, KeyframeEase::easy_ease()));

    // At frame 10 (1/3rd of duration), because of fast launch, value should be well above 33.3%
    let v10 = track.evaluate(10.0);
    assert!(
        v10 > 50.0,
        "Asymmetric fast-launch curve should exceed 50% at frame 10, got {v10}"
    );

    // At frame 25 (approaching the end), growth should be very slow (brake hard)
    let v25 = track.evaluate(25.0);
    let v30 = track.evaluate(30.0);
    assert!(
        (v30 - v25) < 15.0,
        "End segment should brake hard, delta is {}",
        v30 - v25
    );
}

#[test]
fn test_ae_velocity_matches_numerical_derivative() {
    let mut track = KeyframeTrack::new(0.0f64, 60.0);
    track.insert_keyframe(Keyframe::easy_ease(0.0, 0.0));
    track.insert_keyframe(Keyframe::easy_ease(60.0, 600.0));

    // Test velocity at various points against finite difference numerical derivative
    let dt = 1e-4; // Small frame delta
    for f in [5.0, 15.0, 30.0, 45.0, 55.0] {
        let v_analytical = track.velocity(f);

        // Finite difference: [f(t + dt) - f(t - dt)] / (2 * dt_in_seconds)
        let v_plus = track.evaluate(f + dt);
        let v_minus = track.evaluate(f - dt);
        let dt_sec = dt / 60.0;
        let v_numerical = (v_plus - v_minus) / (2.0 * dt_sec);

        let diff = (v_analytical - v_numerical).abs();
        let rel_err = diff / v_numerical.max(1.0);
        assert!(
            rel_err < 0.05,
            "Velocity analytical ({v_analytical}) vs numerical ({v_numerical}) at frame {f} discrepancy: {rel_err}"
        );
    }
}

#[test]
fn test_multi_keyframe_mixed_interpolations() {
    let mut track = KeyframeTrack::new(0.0f64, 30.0);
    // Segment 1: Linear 0 -> 100 (0 to 10f)
    track.insert_keyframe(Keyframe::linear(0.0, 0.0));
    // Segment 2: Hold at 100 (10 to 20f)
    track.insert_keyframe(Keyframe {
        frame: 10.0,
        value: 100.0,
        in_ease: None,
        out_ease: None,
        interpolation_in: InterpolationType::Linear,
        interpolation_out: InterpolationType::Hold,
    });
    // Segment 3: Easy Ease 100 -> 300 (20 to 50f)
    track.insert_keyframe(Keyframe::easy_ease(20.0, 100.0));
    track.insert_keyframe(Keyframe::easy_ease(50.0, 300.0));

    // Verify Segment 1 (linear)
    assert!((track.evaluate(5.0) - 50.0).abs() < 1e-5);

    // Verify Segment 2 (hold)
    assert_eq!(track.evaluate(10.0), 100.0);
    assert_eq!(track.evaluate(15.0), 100.0);
    assert_eq!(track.evaluate(19.999), 100.0);

    // Verify Segment 3 (easy ease)
    assert_eq!(track.evaluate(20.0), 100.0);
    assert!((track.evaluate(35.0) - 200.0).abs() < 1.0);
    assert_eq!(track.evaluate(50.0), 300.0);
}

#[test]
fn test_3d_spatial_and_rotation_track() {
    let mut track = KeyframeTrack::new([0.0f32, 0.0f32, 0.0f32], 30.0);
    track.insert_keyframe(Keyframe::easy_ease(0.0, [0.0, 0.0, -500.0]));
    track.insert_keyframe(Keyframe::easy_ease(30.0, [1920.0, 1080.0, 0.0]));

    let mid = track.evaluate(15.0);
    assert!((mid[0] - 960.0).abs() < 1.0);
    assert!((mid[1] - 540.0).abs() < 1.0);
    assert!((mid[2] - -250.0).abs() < 1.0);
}
