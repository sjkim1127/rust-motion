//! Comprehensive integration tests for:
//! - #109: Resolution-Independent Ken Burns Slideshow & Backdrop Compositor
//! - #127: Pure-Rust EXIF, IPTC-IIM, XMP, Canon CR3, GPX Geotagging & Multi-Cam Sync
//! - #128: Pure-Rust Neural Face Detection (YuNet ONNX Runtime), 5-Point Landmarks, Auto-Reframe & Privacy Mask
//! - #129: Procedural Infinite-Resolution Rec. 2020 HDR Scene Generator & Brown-Conrady Optics

use rust_motion_rasterizer::exif_meta::{
    extract_container_metadata, geotag_from_gpx, parse_gpx_track, parse_iptc_iim, parse_tiff_exif,
    parse_xmp_packet, serialize_iptc_iim, serialize_tiff_exif, serialize_xmp_packet,
    sync_multicam_clips, MediaMetadata,
};
use rust_motion_rasterizer::lens_correction::{
    apply_brown_conrady_correction, BrownConradyModel,
};
use rust_motion_rasterizer::neural_face::{
    apply_privacy_face_mask, compute_auto_reframe_crop, detect_faces_yunet,
    parse_onnx_model_checked, PrivacyMaskMode, Tensor3D,
};
use rust_motion_rasterizer::procedural_scenes::{
    render_procedural_hdr_scene, render_procedural_scene_pixmap, ProceduralSceneType,
};
use rust_motion_rasterizer::slideshow::{
    detect_salient_focus, render_slideshow_frame, KenBurnsEasing, KenBurnsTrajectory,
    SlideshowBackdrop, SlideshowLayoutConfig,
};
use tiny_skia::{Color, Pixmap, PremultipliedColorU8};

#[test]
fn test_slideshow_resolution_independent_geometry_and_ken_burns() {
    let mut photo = Pixmap::new(160, 100).unwrap();
    photo.fill(Color::from_rgba8(30, 40, 60, 255));
    // High-contrast warm subject in upper-right quadrant (x: 110..140, y: 20..50)
    for y in 20..50 {
        for x in 110..140 {
            photo.pixels_mut()[(y * 160 + x) as usize] =
                PremultipliedColorU8::from_rgba(240, 180, 120, 255).unwrap();
        }
    }

    let (fx, fy) = detect_salient_focus(&photo);
    assert!(fx > 0.55, "Salient focus X should be in right half: {fx}");
    assert!(fy < 0.50, "Salient focus Y should be in upper half: {fy}");

    let cfg = SlideshowLayoutConfig::default();
    let g_720p = cfg.resolve_geometry(1280, 720, 1600, 1000);
    let g_4k = cfg.resolve_geometry(3840, 2160, 1600, 1000);

    // 4K is exactly 3x 720p; proportional metrics must scale by 3x
    assert!((g_4k.margin_px / g_720p.margin_px - 3.0).abs() < 0.1);
    assert!((g_4k.shadow_softness_px / g_720p.shadow_softness_px - 3.0).abs() < 0.05);

    let traj = KenBurnsTrajectory::auto_from_photo(&photo, 1.35);
    let kf0 = traj.sample(0.0);
    let kf1 = traj.sample(1.0);
    assert!((kf0.zoom - 1.0).abs() < 1e-5);
    assert!((kf1.zoom - 1.35).abs() < 1e-5);

    // Test CubicBezier easing
    let bez = KenBurnsEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
    };
    assert!((bez.evaluate(0.0) - 0.0).abs() < 1e-4);
    assert!((bez.evaluate(1.0) - 1.0).abs() < 1e-4);
    assert!(bez.evaluate(0.5) > 0.5);

    // Render with BlurredMirror, SolidColor, and LinearGradient backdrops
    let frame_mirror = render_slideshow_frame(&photo, 320, 180, &cfg, &traj, 0.5);
    assert_eq!(frame_mirror.width(), 320);
    assert_eq!(frame_mirror.height(), 180);

    let mut cfg_grad = cfg.clone();
    cfg_grad.backdrop = SlideshowBackdrop::LinearGradient {
        top: [20, 30, 50, 255],
        bottom: [10, 10, 20, 255],
    };
    let frame_grad = render_slideshow_frame(&photo, 320, 180, &cfg_grad, &traj, 1.0);
    assert_eq!(frame_grad.width(), 320);
}

#[test]
fn test_exif_xmp_iptc_cr3_gpx_and_multicam_sync() {
    let orig = MediaMetadata {
        make: Some("Sony".into()),
        model: Some("ILCE-7RM5".into()),
        lens_make: Some("Sony".into()),
        lens_model: Some("FE 24-70mm F2.8 GM II".into()),
        focal_length_mm: Some(35.0),
        f_number: Some(2.8),
        exposure_time_sec: Some(0.005),
        iso: Some(400),
        date_time_original: Some("2026:10:15 14:30:15".into()),
        orientation: Some(1),
        xmp_rating: Some(5),
        xmp_title: Some("Alpine Sunrise".into()),
        xmp_creator: Some("Storytold Studio".into()),
        iptc_object_name: Some("Scene 01".into()),
        iptc_caption: Some("Opening establishing shot".into()),
        iptc_byline: Some("Director of Photography".into()),
        iptc_copyright: Some("CC0 Public Domain".into()),
        iptc_keywords: vec!["hdr".into(), "cinema".into()],
        smpte_timecode: Some("01:00:10:12".into()),
        ..Default::default()
    };

    // 1. Binary TIFF/EXIF roundtrip
    let tiff_bytes = serialize_tiff_exif(&orig);
    let mut parsed_tiff = MediaMetadata::default();
    parse_tiff_exif(&tiff_bytes, &mut parsed_tiff).expect("parse TIFF EXIF");
    assert_eq!(parsed_tiff.make.as_deref(), Some("Sony"));
    assert_eq!(parsed_tiff.model.as_deref(), Some("ILCE-7RM5"));
    assert_eq!(
        parsed_tiff.lens_model.as_deref(),
        Some("FE 24-70mm F2.8 GM II")
    );
    assert!((parsed_tiff.focal_length_mm.unwrap() - 35.0).abs() < 1e-2);
    assert!((parsed_tiff.f_number.unwrap() - 2.8).abs() < 1e-2);
    assert_eq!(parsed_tiff.iso, Some(400));

    // 2. XMP packet roundtrip
    let xmp_str = serialize_xmp_packet(&orig);
    let mut parsed_xmp = MediaMetadata::default();
    parse_xmp_packet(&xmp_str, &mut parsed_xmp);
    assert_eq!(parsed_xmp.xmp_rating, Some(5));
    assert_eq!(parsed_xmp.xmp_title.as_deref(), Some("Alpine Sunrise"));
    assert_eq!(parsed_xmp.xmp_creator.as_deref(), Some("Storytold Studio"));
    assert_eq!(parsed_xmp.smpte_timecode.as_deref(), Some("01:00:10:12"));

    // 3. IPTC-IIM Record 2 binary roundtrip
    let iptc_bytes = serialize_iptc_iim(&orig);
    let mut parsed_iptc = MediaMetadata::default();
    parse_iptc_iim(&iptc_bytes, &mut parsed_iptc);
    assert_eq!(parsed_iptc.iptc_object_name.as_deref(), Some("Scene 01"));
    assert_eq!(
        parsed_iptc.iptc_caption.as_deref(),
        Some("Opening establishing shot")
    );
    assert_eq!(parsed_iptc.iptc_keywords, vec!["hdr", "cinema"]);

    // 4. Canon CR3 container (`ftypcrx ` + `CMT1` box) extraction
    let mut cr3_buf = Vec::new();
    cr3_buf.extend_from_slice(&24u32.to_be_bytes());
    cr3_buf.extend_from_slice(b"ftypcrx ");
    cr3_buf.extend_from_slice(&[0u8; 12]);
    cr3_buf.extend_from_slice(b"CMT1");
    cr3_buf.extend_from_slice(&tiff_bytes);
    let cr3_meta = extract_container_metadata(&cr3_buf).expect("extract CR3 metadata");
    assert_eq!(cr3_meta.make.as_deref(), Some("Sony"));
    assert_eq!(cr3_meta.iso, Some(400));

    // 5. GPX Track Log Geotagging interpolation
    let gpx_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1">
  <trk><trkseg>
    <trkpt lat="37.5000" lon="127.0000"><ele>100.0</ele><time>2026-10-15T14:30:00Z</time></trkpt>
    <trkpt lat="37.5100" lon="127.0200"><ele>140.0</ele><time>2026-10-15T14:30:30Z</time></trkpt>
  </trkseg></trk>
</gpx>"#;
    let track = parse_gpx_track(gpx_xml);
    assert_eq!(track.len(), 2);
    let mut geo_meta = parsed_tiff.clone();
    assert!(geotag_from_gpx(&mut geo_meta, &track));
    // 14:30:15 is exactly halfway between 14:30:00 and 14:30:30
    assert!((geo_meta.gps_latitude.unwrap() - 37.5050).abs() < 1e-4);
    assert!((geo_meta.gps_longitude.unwrap() - 127.0100).abs() < 1e-4);
    assert!((geo_meta.gps_altitude_m.unwrap() - 120.0).abs() < 1e-2);

    // 6. Multi-Camera Timeline Synchronization
    let cam_a = MediaMetadata {
        smpte_timecode: Some("01:00:00:00".into()),
        ..Default::default()
    };
    let cam_b = MediaMetadata {
        smpte_timecode: Some("01:00:02:15".into()),
        ..Default::default()
    };
    let offsets = sync_multicam_clips(&[cam_a, cam_b], 30.0);
    assert_eq!(offsets, vec![0, 75]);
}

#[test]
fn test_onnx_runtime_yunet_face_landmarks_reframe_and_privacy_mask() {
    // 1. Tensor3D depthwise separable conv & pooling
    let mut t = Tensor3D::zeros(2, 8, 8);
    for y in 0..8 {
        for x in 0..8 {
            t.set(0, y, x, 1.0);
            t.set(1, y, x, 0.5);
        }
    }
    let dw_w = vec![1.0 / 9.0; 2 * 9];
    let pw_w = vec![1.0; 4 * 2];
    let bias = vec![0.1; 4];
    let conv_out = t
        .depthwise_separable_conv3x3(&dw_w, &pw_w, &bias, 4)
        .expect("conv3x3");
    assert_eq!(conv_out.channels, 4);
    let pooled = conv_out.max_pool_2x2();
    assert_eq!((pooled.height, pooled.width), (4, 4));

    // 2. Checked ONNX protobuf parser (valid + hostile/corrupt inputs)
    // Field 1 (ir_version = 8): tag 0x08, val 0x08
    // Field 2 (producer_name = "yunet"): tag 0x12, len 5, b"yunet"
    // Field 7 (graph with 2 NodeProto): tag 0x3A, len 8, [0x0A, 0x02, 0x01, 0x02, 0x0A, 0x02, 0x03, 0x04]
    let valid_onnx = [
        0x08, 0x08, 0x12, 0x05, b'y', b'u', b'n', b'e', b't', 0x3A, 0x08, 0x0A, 0x02, 0x01, 0x02,
        0x0A, 0x02, 0x03, 0x04,
    ];
    let summary = parse_onnx_model_checked(&valid_onnx).expect("valid ONNX model");
    assert_eq!(summary.ir_version, 8);
    assert_eq!(summary.producer_name, "yunet");
    assert_eq!(summary.graph_node_count, 2);

    // Hostile truncated length-delimited field must return Err cleanly without panicking
    let hostile_onnx = [0x12, 0xFF, 0xFF, 0xFF, 0x01, 0x00];
    assert!(parse_onnx_model_checked(&hostile_onnx).is_err());

    // 3. YuNet Face & 5-Point Landmark Detection
    let mut frame = Pixmap::new(160, 90).unwrap();
    frame.fill(Color::from_rgba8(25, 35, 50, 255));
    // Draw a warm skin-toned face region around x: 100..132, y: 24..64
    for y in 24..64 {
        for x in 100..132 {
            frame.pixels_mut()[(y * 160 + x) as usize] =
                PremultipliedColorU8::from_rgba(225, 170, 135, 255).unwrap();
        }
    }

    let faces = detect_faces_yunet(&frame, 0.5);
    assert_eq!(faces.len(), 1);
    let f = &faces[0];
    assert!(f.landmarks[0].0 < f.landmarks[1].0, "Right eye left of left eye");
    assert!(f.landmarks[2].1 > f.landmarks[0].1, "Nose below eyes");
    assert!(f.landmarks[3].1 > f.landmarks[2].1, "Mouth below nose");

    // 4. Auto-Reframe 16:9 (160x90) -> 9:16 vertical crop
    let crop = compute_auto_reframe_crop(160, 90, 9.0 / 16.0, &faces, None, 0.5);
    assert!((crop[2] - 50.625).abs() < 0.1);
    assert!((crop[3] - 90.0).abs() < 0.1);
    // Crop center should align near face center (~116)
    let crop_cx = crop[0] + crop[2] * 0.5;
    assert!((crop_cx - 116.0).abs() < 6.0, "Crop center X: {crop_cx}");

    // 5. Smart Privacy Face Mask (Mosaic & Blur)
    apply_privacy_face_mask(&mut frame, &faces, PrivacyMaskMode::Mosaic { block_size: 8 });
    apply_privacy_face_mask(&mut frame, &faces, PrivacyMaskMode::Blur { radius: 4 });
}

#[test]
fn test_procedural_hdr_scenes_and_brown_conrady_optics() {
    for scene in [
        ProceduralSceneType::OceanSunset,
        ProceduralSceneType::AlpineLake,
        ProceduralSceneType::Dunes,
        ProceduralSceneType::Aurora,
    ] {
        let hdr = render_procedural_hdr_scene(scene, 96, 54, 42, 1.5);
        assert_eq!(hdr.width, 96);
        assert_eq!(hdr.height, 54);

        let max_channel = hdr
            .data
            .iter()
            .flat_map(|px| px.iter().copied())
            .fold(0.0f32, f32::max);
        assert!(
            max_channel > 1.0,
            "Scene {scene:?} must contain true HDR highlights > 1.0 (got {max_channel})"
        );

        let pm = render_procedural_scene_pixmap(scene, 64, 36, 42, 0.0, 0.0);
        assert_eq!(pm.width(), 64);
        assert_eq!(pm.height(), 36);
    }

    // Verify Brown-Conrady radial + tangential + cos^4 vignette model
    let mut test_pm = Pixmap::new(64, 64).unwrap();
    test_pm.fill(Color::from_rgba8(180, 180, 180, 255));
    let model = BrownConradyModel {
        radial_k: [-0.05, 0.01, 0.0],
        tangential_p: [0.005, -0.003],
        tca_shift: [0.001, -0.001],
        vignette_poly: [0.1, 0.0, 0.0],
        cos4_vignette_strength: 0.3,
        optical_center: [0.5, 0.5],
    };
    apply_brown_conrady_correction(&mut test_pm, &model);
    let center_px = test_pm.pixel(32, 32).unwrap();
    let corner_px = test_pm.pixel(2, 2).unwrap();
    assert!(
        corner_px.red() > center_px.red(),
        "Positive vignette + cos^4 compensation should brighten corners"
    );
}
