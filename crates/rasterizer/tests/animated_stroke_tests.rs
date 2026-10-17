use rust_motion_rasterizer::{
    Color, FrameConfig, RasterizerBackend, Scene, SceneNode, TinySkiaBackend,
};
use rust_motion_rasterizer::paint::{BrushSettings, Stroke, StrokePoint};
use rust_motion_rasterizer::tablet::{Sample, StrokeRecorder};

#[test]
fn test_animated_stroke_monotonic_write_on_progress() {
    let backend = TinySkiaBackend::new();
    let width = 200;
    let height = 200;
    let config = FrameConfig::new(width, height, 0, 30.0);

    // Construct a brush and a 4-point horizontal stroke
    let mut brush = BrushSettings::default();
    brush.size = 10.0;
    brush.color = [1.0, 0.0, 0.0, 1.0]; // Red

    let points = vec![
        StrokePoint::new(20.0, 100.0, 0.2),
        StrokePoint::new(60.0, 100.0, 0.5),
        StrokePoint::new(120.0, 100.0, 0.8),
        StrokePoint::new(180.0, 100.0, 1.0),
    ];
    let stroke = Stroke { brush, points };

    let count_painted_pixels = |progress: f32| -> usize {
        let scene = Scene {
            nodes: vec![SceneNode::AnimatedStroke {
                stroke: stroke.clone(),
                progress,
                color: None,
                opacity: 1.0,
            }],
        };
        let frame = backend.render_frame(&scene, &config).expect("render frame failed");
        frame
            .chunks_exact(4)
            .filter(|px| px[3] > 0) // check alpha
            .count()
    };

    let p0 = count_painted_pixels(0.0);
    let p25 = count_painted_pixels(0.25);
    let p50 = count_painted_pixels(0.5);
    let p75 = count_painted_pixels(0.75);
    let p100 = count_painted_pixels(1.0);

    assert_eq!(p0, 0, "Progress 0.0 must paint 0 pixels");
    assert!(p25 > 0, "Progress 0.25 should paint some pixels");
    assert!(p50 > p25, "Progress 0.5 should paint more pixels than 0.25");
    assert!(p75 > p50, "Progress 0.75 should paint more pixels than 0.5");
    assert!(p100 >= p75, "Progress 1.0 should paint at least as many as 0.75");
}

#[test]
fn test_tablet_recorder_to_animated_stroke_rendering() {
    let mut recorder = StrokeRecorder::new();
    recorder.start();

    // Simulate stylus events with varying pressure and tilt
    for i in 0..10 {
        let x = 30.0 + (i as f64) * 14.0;
        let y = 50.0 + ((i as f64) * 0.5).sin() * 20.0;
        let sample = Sample {
            pressure: 0.1 + (i as f32) * 0.09,
            tilt_x: 10.0,
            tilt_y: 5.0,
            rotation: 45.0,
            eraser: false,
        };
        recorder.record_sample(x, y, Some(sample), (i as f64) * 16.6);
    }

    assert_eq!(recorder.point_count(), 10);
    let mut brush = BrushSettings::default();
    brush.size = 12.0;

    let stroke = recorder.finish(brush);
    assert_eq!(stroke.points.len(), 10);

    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(250, 120, 0, 60.0);

    let scene = Scene {
        nodes: vec![SceneNode::AnimatedStroke {
            stroke,
            progress: 1.0,
            color: Some(Color::rgba(0, 200, 255, 255)), // Cyan override
            opacity: 0.9,
        }],
    };

    let frame = backend.render_frame(&scene, &config).expect("render failed");
    let has_cyan = frame
        .chunks_exact(4)
        .any(|px| px[0] == 0 && px[1] > 100 && px[2] > 150 && px[3] > 0);
    assert!(has_cyan, "Rendered frame must contain cyan painted stroke");
}

#[test]
fn test_animated_stroke_serde_roundtrip() {
    let mut brush = BrushSettings::default();
    brush.size = 8.0;
    let stroke = Stroke {
        brush,
        points: vec![
            StrokePoint::new(10.0, 20.0, 0.4),
            StrokePoint::new(30.0, 40.0, 0.9),
        ],
    };

    let node = SceneNode::AnimatedStroke {
        stroke,
        progress: 0.65,
        color: Some(Color::rgb(255, 128, 0)),
        opacity: 0.8,
    };

    let json = serde_json::to_string(&node).expect("serialize failed");
    let deserialized: SceneNode = serde_json::from_str(&json).expect("deserialize failed");

    match deserialized {
        SceneNode::AnimatedStroke { progress, color, opacity, .. } => {
            assert!((progress - 0.65).abs() < 1e-4);
            assert_eq!(color, Some(Color::rgb(255, 128, 0)));
            assert!((opacity - 0.8).abs() < 1e-4);
        }
        _ => panic!("Expected AnimatedStroke variant"),
    }
}
