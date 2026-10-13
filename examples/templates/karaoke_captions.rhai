// Karaoke / Kinetic Subtitle Motion Template (9:16 Vertical Short-Form Video)
// Props:
// - tag: Category / hashtag badge (e.g. "#RUST #PERFORMANCE")
// - active_color: Highlight color for currently spoken word (e.g. "#ffe600")
// - inactive_color: Color for unplayed words (e.g. "#ffffff")
// - bg_top: Top background color
// - bg_bottom: Bottom background color
fn render(ctx, props) {
    let output = scene();
    let width = ctx.width.to_float();
    let height = ctx.height.to_float();
    let t = ctx.progress;

    let bg_top = if props.bg_top != () { props.bg_top } else { "#0d0e1a" };
    let bg_bottom = if props.bg_bottom != () { props.bg_bottom } else { "#1c142e" };
    let active_color = if props.active_color != () { props.active_color } else { "#ffe600" };
    let inactive_color = if props.inactive_color != () { props.inactive_color } else { "#ffffff" };
    let tag = if props.tag != () { props.tag } else { "#RUST #AI #AUTOMATION" };

    // 1. Vertical dark gradient backdrop
    output.linear_gradient(0.0, 0.0, width, height, 160.0, bg_top, bg_bottom);

    // 2. Ambient neon backglow
    output.circle(width * 0.5, height * 0.48, width * 0.38, "#ff007f18");

    // 3. Category Tag Badge
    let badge_w = 280.0;
    let badge_h = 44.0;
    let badge_x = (width - badge_w) * 0.5;
    let badge_y = height * 0.16;
    output.rect_stroke(badge_x, badge_y, badge_w, badge_h, "#ffffff12", "#ffffff33", 1.0, 22.0);
    output.text_bold(badge_x + 28.0, badge_y + 30.0, tag, 16.0, "#00f0ff");

    // 4. Kinetic Captions Card
    let card_w = width * 0.90;
    let card_h = 180.0;
    let card_x = (width - card_w) * 0.5;
    let card_y = height * 0.44;

    output.rect_stroke(card_x, card_y, card_w, card_h, "#000000bb", "#ffffff22", 1.0, 24.0);

    // Timed words: [word, start_pct, end_pct]
    let words = [
        ["Rust Motion", 0.00, 0.20],
        ["renders", 0.20, 0.40],
        ["video", 0.40, 0.60],
        ["natively", 0.60, 0.80],
        ["in Rust", 0.80, 1.00]
    ];

    let word_count = 5.0;
    let spacing = width * 0.015;
    let word_w = (card_w - spacing * (word_count + 1.0)) / word_count;
    let base_font = (width * 0.05).clamp(24.0, 48.0);
    let center_y = card_y + card_h * 0.52;

    for i in 0..5 {
        let entry = words[i];
        let text = entry[0];
        let w_start = entry[1];
        let w_end = entry[2];

        let is_active = t >= w_start && (t < w_end || (w_end == 1.0 && t <= 1.0));
        let is_past = t >= w_end;

        let font_size = if is_active { base_font * 1.18 } else { base_font };
        let col = if is_active {
            active_color
        } else if is_past {
            "#cbd5e1"
        } else {
            "#64748b"
        };

        let wx = card_x + spacing + i.to_float() * (word_w + spacing);

        if is_active {
            // Active word glowing pill
            output.rect_stroke(wx - 6.0, center_y - font_size * 0.85, word_w + 12.0, font_size * 1.2, "#ffe60028", active_color, 1.5, 8.0);
        }

        output.text_bold(wx, center_y + 4.0, text, font_size, col);
    }

    // 5. Attribution / Sync footer inside card
    output.text(card_x + 32.0, card_y + card_h - 22.0, "AUTONOMOUS VIDEO PIPELINE", 12.0, "#94a3b8");

    // 6. Bottom progress bar
    output.rect(0.0, height - 12.0, width * t, 12.0, active_color);

    output
}
