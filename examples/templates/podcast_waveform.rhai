// Podcast Waveform Motion Template
// Works in standard landscape (1920x1080) and mobile vertical (1080x1920).
// Props:
// - title: Show title string
// - host: Host / guest attribution
// - tag: Episode badge text
// - duration_label: Total time string (e.g. "45:00")
// - bg_top: Hex color for top background
// - bg_bottom: Hex color for bottom background
// - accent: Hex color for waveform and highlights
fn render(ctx, props) {
    let output = scene();
    let width = ctx.width.to_float();
    let height = ctx.height.to_float();
    let progress = ctx.progress;
    let t = progress;

    let bg_top = if props.bg_top != () { props.bg_top } else { "#0a0f1d" };
    let bg_bottom = if props.bg_bottom != () { props.bg_bottom } else { "#161e38" };
    let accent = if props.accent != () { props.accent } else { "#00f0ff" };
    let title = if props.title != () { props.title } else { "THE DEEP DIVE PODCAST" };
    let host = if props.host != () { props.host } else { "Episode 42 • Hosted by Sarah & Alex" };
    let tag = if props.tag != () { props.tag } else { "AI & NATIVE CODE" };
    let duration_label = if props.duration_label != () { props.duration_label } else { "45:00" };

    // 1. Studio backdrop gradient
    output.linear_gradient(0.0, 0.0, width, height, 180.0, bg_top, bg_bottom);

    // 2. Ambient neon backdrop glow
    output.circle(width * 0.25, height * 0.35, width * 0.3, "#00f0ff14");

    // 3. Category/Episode Badge
    let badge_w = 260.0;
    let badge_h = 38.0;
    let badge_x = (width - badge_w) * 0.5;
    let badge_y = height * 0.12;
    output.rect_stroke(badge_x, badge_y, badge_w, badge_h, "#ffffff10", accent, 1.0, 19.0);
    output.text_bold(badge_x + 32.0, badge_y + 26.0, tag, 14.0, accent);

    // 4. Host Avatar with Pulsating Sound Waves
    let avatar_cx = width * 0.5;
    let avatar_cy = height * 0.35;
    let avatar_r = if width < height { width * 0.16 } else { height * 0.14 };
    
    // Wave pulse outer rings
    let pulse_delta = (t * 25.0).sin().abs() * 12.0;
    output.circle_stroke(avatar_cx, avatar_cy, avatar_r + pulse_delta + 10.0, "#00000000", "#00f0ff44", 2.0);
    output.circle_stroke(avatar_cx, avatar_cy, avatar_r, "#1e293b", accent, 3.0);
    output.text(avatar_cx - 16.0, avatar_cy + 10.0, "MIC", avatar_r * 0.65, "#ffffff");

    // 5. Episode Title & Host Metadata
    let title_y = height * 0.56;
    let font_size = if width < height { width * 0.055 } else { width * 0.035 };
    output.text_bold(width * 0.1, title_y, title, font_size, "#ffffff");
    output.text(width * 0.1, title_y + 36.0, host, font_size * 0.55, "#94a3b8");

    // 6. Audio Waveform (Multi-band procedural sound bars)
    let wave_y = height * 0.76;
    let bar_count = 28;
    let total_wave_w = width * 0.8;
    let bar_w = (total_wave_w / bar_count.to_float()) * 0.65;
    let gap = (total_wave_w / bar_count.to_float()) * 0.35;
    let start_x = (width - total_wave_w) * 0.5;

    for i in 0..bar_count {
        let fi = i.to_float();
        let h1 = (fi * 0.45 + t * 18.0).sin();
        let h2 = (fi * 0.85 - t * 12.0).cos();
        let norm = ((h1 + h2) * 0.5).abs();
        let bar_h = 10.0 + norm * 80.0;
        let x = start_x + fi * (bar_w + gap);
        let y = wave_y - bar_h * 0.5;

        let col = if fi < (bar_count.to_float() * t) { accent } else { "#94a3b844" };
        output.round_rect(x, y, bar_w, bar_h, col, bar_w * 0.4);
    }

    // 7. Timecode Telemetry
    let cur_secs = (t * 2700.0).to_int();
    let mins = cur_secs / 60;
    let secs = cur_secs % 60;
    let sec_str = if secs < 10 { "0" + secs } else { "" + secs };
    let time_display = mins + ":" + sec_str + " / " + duration_label;
    output.text(start_x, wave_y + 48.0, time_display, 15.0, "#94a3b8");

    // 8. Bottom playback progress line
    output.rect(0.0, height - 8.0, width * t, 8.0, accent);

    output
}
