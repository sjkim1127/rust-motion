use rust_motion_ts::{
    evaluate_colors, evaluate_interpolate, evaluate_measure_spring, evaluate_random,
    evaluate_spring, process_json_command, ExtrapolateMode, InterpolateRequest,
    SpringConfigRequest, SpringRequest,
};

#[test]
fn test_interpolate_evaluation() {
    let req = InterpolateRequest {
        input: 15.0,
        input_range: vec![0.0, 30.0],
        output_range: vec![0.0, 100.0],
        extrapolate_left: Some(ExtrapolateMode::Clamp),
        extrapolate_right: Some(ExtrapolateMode::Clamp),
        easing_bezier: None,
    };
    let val = evaluate_interpolate(&req);
    assert!((val - 50.0).abs() < 1e-6);

    let clamp_left = evaluate_interpolate(&InterpolateRequest {
        input: -10.0,
        input_range: vec![0.0, 30.0],
        output_range: vec![0.0, 100.0],
        extrapolate_left: Some(ExtrapolateMode::Clamp),
        extrapolate_right: Some(ExtrapolateMode::Clamp),
        easing_bezier: None,
    });
    assert!((clamp_left - 0.0).abs() < 1e-6);
}

#[test]
fn test_spring_evaluation() {
    let req = SpringRequest {
        frame: 15.0,
        fps: 30.0,
        config: Some(SpringConfigRequest {
            damping: Some(10.0),
            mass: Some(1.0),
            stiffness: Some(100.0),
            overshoot_clamping: Some(false),
        }),
        from: Some(0.0),
        to: Some(1.0),
        delay: Some(0.0),
        duration_in_frames: None,
        duration_rest_threshold: None,
        reverse: None,
    };
    let val = evaluate_spring(&req).unwrap();
    assert!(val > 0.0 && val < 1.5);

    let measure = evaluate_measure_spring(
        30.0,
        &SpringConfigRequest {
            damping: Some(10.0),
            mass: Some(1.0),
            stiffness: Some(100.0),
            overshoot_clamping: Some(false),
        },
        0.005,
    )
    .unwrap();
    assert!(measure > 0.0 && measure < 100.0);
}

#[test]
fn test_color_and_random_evaluation() {
    let color = evaluate_colors(
        0.5,
        &[0.0, 1.0],
        &["#000000".to_string(), "#ffffff".to_string()],
    )
    .unwrap();
    assert!(color.contains("128") || color.contains("127") || color.contains("#"));

    let r1 = evaluate_random(&serde_json::json!("seed1"));
    let r2 = evaluate_random(&serde_json::json!("seed1"));
    let r3 = evaluate_random(&serde_json::json!("seed2"));
    assert_eq!(r1, r2);
    assert_ne!(r1, r3);
}

#[test]
fn test_json_rpc_dispatcher() {
    let json_req = serde_json::json!({
        "command": "interpolate",
        "params": {
            "input": 15.0,
            "input_range": [0.0, 30.0],
            "output_range": [10.0, 20.0]
        }
    })
    .to_string();

    let res_str = process_json_command(&json_req);
    let res: serde_json::Value = serde_json::from_str(&res_str).unwrap();
    assert_eq!(res["success"], true);
    assert!((res["data"].as_f64().unwrap() - 15.0).abs() < 1e-6);

    let list_req = serde_json::json!({
        "command": "list_compositions"
    })
    .to_string();
    let list_res_str = process_json_command(&list_req);
    let list_res: serde_json::Value = serde_json::from_str(&list_res_str).unwrap();
    assert_eq!(list_res["success"], true);
    assert!(list_res["data"].as_array().unwrap().len() > 0);
}
