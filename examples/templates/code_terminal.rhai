// macOS Terminal Code Typing & Execution Motion Template
// Props:
// - terminal_title: Window header title string
// - command: Shell command to type
// - bg: Background color
fn render(ctx, props) {
    let output = scene();
    let width = ctx.width.to_float();
    let height = ctx.height.to_float();
    let t = ctx.progress;
    let frame = ctx.frame;

    let bg = if props.bg != () { props.bg } else { "#0f111a" };
    let term_title = if props.terminal_title != () { props.terminal_title } else { "rust-motion — bash — 80x24" };
    let full_command = if props.command != () { props.command } else { "cargo run --release --bin rust-motion" };

    // 1. Cyber dark backdrop
    output.rect(0.0, 0.0, width, height, bg);

    // 2. Terminal Window Frame
    let win_w = width * 0.88;
    let win_h = height * 0.80;
    let win_x = (width - win_w) * 0.5;
    let win_y = (height - win_h) * 0.5;

    // Window shadow / body
    output.rect_stroke(win_x, win_y, win_w, win_h, "#1e1e2e", "#45475a", 1.5, 16.0);

    // Window Titlebar
    let bar_h = 44.0;
    output.round_rect(win_x, win_y, win_w, bar_h, "#181825", 16.0);

    // Traffic light buttons
    let dot_y = win_y + bar_h * 0.5;
    output.circle(win_x + 22.0, dot_y, 6.0, "#ff5f56");
    output.circle(win_x + 42.0, dot_y, 6.0, "#ffbd2e");
    output.circle(win_x + 62.0, dot_y, 6.0, "#27c93f");

    // Title text
    output.text(win_x + win_w * 0.35, win_y + 27.0, term_title, 13.0, "#a6adc8");

    // 3. Prompt & Typewriter Command
    let content_x = win_x + 30.0;
    let line_y = win_y + bar_h + 38.0;
    let font_size = if width < height { width * 0.035 } else { (win_w * 0.024).clamp(14.0, 22.0) };

    output.text_bold(content_x, line_y, "➜  rust-motion git:(main) ", font_size, "#a6e3a1");

    // Typing animation
    let typing_t = (t * 2.2).clamp(0.0, 1.0);
    let typed_cmd = typewriter_text(full_command, typing_t);
    output.text_bold(content_x + 235.0, line_y, typed_cmd, font_size, "#cdd6f4");

    // 4. Code lines preview
    line_y += 42.0;
    output.text(content_x + 20.0, line_y, "fn render_composition() -> Scene {", font_size * 0.95, "#cba6f7");
    line_y += 30.0;
    output.text(content_x + 20.0, line_y, "    let mut scene = Scene::new();", font_size * 0.95, "#9399b2");
    line_y += 30.0;
    output.text(content_x + 20.0, line_y, "    scene.render_native();", font_size * 0.95, "#89b4fa");
    line_y += 30.0;
    output.text(content_x + 20.0, line_y, "}", font_size * 0.95, "#cba6f7");

    // 5. Execution badge when t > 0.5
    if t > 0.5 {
        let badge_w = 260.0;
        let badge_h = 36.0;
        let badge_y = win_y + win_h - 60.0;

        output.rect_stroke(content_x, badge_y, badge_w, badge_h, "#27c93f22", "#27c93f", 1.0, 8.0);
        output.text_bold(content_x + 16.0, badge_y + 24.0, "✓ Render complete in 18ms", 14.0, "#a6e3a1");
    }

    // 6. Bottom timeline progress
    output.rect(0.0, height - 6.0, width * t, 6.0, "#cba6f7");

    output
}
