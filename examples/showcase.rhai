// Rust Motion README showcase.
//
// Designed at 1920x1080 for 5 seconds; every coordinate is scaled by `k`, so any
// 16:9 size works. The background layer repeats with period T and the content fades
// out over the last 0.45 s, so the clip loops seamlessly as a GIF.
//
//   rust-motion render --script examples/showcase.rhai --output showcase.mp4 \
//     --width 1920 --height 1080 --fps 60 --duration 300

// Two-digit hex alpha for 0.0..=1.0.
fn hex2(a) {
    let v = (clamp(a, 0.0, 1.0) * 255.0).round().to_int();
    let h = to_hex(v);
    if h.len() < 2 { h = "0" + h; }
    h
}

fn rgba(c, a) { c + hex2(a) }

fn fmax(a, b) { if a > b { a } else { b } }

fn fmin(a, b) { if a < b { a } else { b } }

// Normalised progress of `t` through [start, start + dur].
fn seg(t, start, dur) { clamp((t - start) / dur, 0.0, 1.0) }

fn ease_out(x) {
    let u = 1.0 - x;
    1.0 - u * u * u
}

fn ease_in_out(x) {
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        let u = -2.0 * x + 2.0;
        1.0 - u * u * u / 2.0
    }
}

// Spring that starts at `start` seconds (0 before it).
fn pop(t, start, fps, damping, stiffness) {
    if t <= start { return 0.0; }
    spring((t - start) * fps, fps, damping, 1.0, stiffness)
}

fn pad3(n) {
    let s = "" + n;
    while s.len() < 3 { s = "0" + s; }
    s
}

fn render(ctx, props) {
    let ORANGE = "#d35427";
    let ORANGE_SOFT = "#f08a5d";
    let INK = "#f5f7fa";
    let MUTED = "#8fa3b8";
    let BG_A = "#0a1624";
    let BG_B = "#13304f";
    let out = scene();
    let W = ctx.width.to_float();
    let H = ctx.height.to_float();
    let k = W / 1920.0;
    let fps = ctx.fps;
    let t = ctx.frame.to_float() / fps;
    let T = ctx.duration.to_float() / fps;
    let p = t / T;

    // Global fade-out at the end so the GIF loops back to an empty stage.
    let fo = 1.0 - ease_in_out(seg(t, T - 0.45, 0.45));

    // ---------------------------------------------------------------- background
    out.linear_gradient(0.0, 0.0, W, H, 125.0 + 12.0 * sin(p * 2.0 * PI()), BG_A, BG_B);

    // Dot grid with a light band sweeping diagonally twice per loop.
    let band = -500.0 + 3200.0 * ((p * 2.0) % 1.0);
    let step = 64.0;
    for gy in 0..17 {
        for gx in 0..30 {
            let x = 48.0 + gx.to_float() * step;
            let y = 36.0 + gy.to_float() * step;
            let d = (x + y * 0.6 - band).abs();
            let glow = fmax(0.0, 1.0 - d / 300.0);
            out.circle(x * k, y * k, (1.6 + 1.4 * glow) * k, rgba(INK, 0.05 + 0.20 * glow));
        }
    }

    // Film-frame outlines drifting upward; positions repeat every loop.
    for i in 0..7 {
        let fi = i.to_float();
        let fw = 120.0 + 90.0 * random(i * 7 + 1);
        let fh = fw * 0.5625;
        let fx = 60.0 + 1800.0 * random(i * 13 + 5);
        let span = 1080.0 + fh * 2.0;
        let fy = ((1.0 - p - fi / 7.0 + 2.0) % 1.0) * span - fh;
        out.rect_stroke(fx * k, fy * k, fw * k, fh * k, "#00000000", rgba(INK, 0.06), 2.0 * k, 10.0 * k);
    }

    // ---------------------------------------------------------------- logo mark
    // The R mark from assets/logo.svg, converted to absolute path commands (the CPU
    // path parser only accepts absolute M/L/H/V/C/Q/A/Z).
    let ms = pop(t, 0.15, fps, 11.0, 150.0);
    if ms > 0.001 {
        let mark = scene();
        mark.path("M 48 36 L 155 36 C 232 36 276 78 276 141 C 276 193 248 223 198 237 L 294 326 L 212 326 L 130 245 L 130 326 L 48 326 L 48 36 Z M 130 98 L 130 179 L 152 179 C 180 179 196 166 196 139 C 196 112 180 98 152 98 L 130 98 Z",
            rgba(INK, fo), "", 0.0, 1.0);
        let play = pop(t, 0.45, fps, 9.0, 180.0);
        if play > 0.001 {
            mark.path("M 144 119 C 144 114 149 111 153 114 L 190 136 C 194 139 194 144 190 147 L 153 169 C 149 172 144 169 144 164 L 144 119 Z",
                rgba(ORANGE, fo * clamp(play, 0.0, 1.0)), "", 0.0, 1.0);
        }
        // world = s * (local + offset): keep the mark centred on (cx, cy) while it scales.
        let s = 0.45 * ms * k;
        let cx = 200.0 * k;
        let cy = 365.0 * k;
        out.group(mark, cx / s - 171.0, cy / s - 181.0, s, 0.0, 1.0);
    }

    // ---------------------------------------------------------------- title
    let words = [["Rust", ORANGE, 300.0, 0.35], ["Motion", INK, 569.0, 0.5]];
    for w in words {
        let e = pop(t, w[3], fps, 13.0, 120.0);
        let a = clamp(e * 1.4, 0.0, 1.0) * fo;
        if a > 0.0 {
            out.text_bold(w[2] * k, (405.0 + 40.0 * (1.0 - e)) * k, w[0], 112.0 * k, rgba(w[1], a));
        }
    }

    // Tagline typed out with a cursor.
    let tag = "Programmatic video, rendered natively in Rust.";
    let tp = seg(t, 1.05, 1.1);
    if tp > 0.0 {
        let shown = if tp >= 1.0 && ((t * 2.0).floor().to_int() % 2 == 0) { tag } else { typewriter_text(tag, tp) };
        out.text(304.0 * k, 472.0 * k, shown, 34.0 * k, rgba("#b6c4d4", fo));
    }

    // Front-end chips. After 3 s the highlight walks across them once, then returns.
    let chips = ["Rust", "Rhai", "Python", "TypeScript", "WASM"];
    let active = 0;
    if t >= 3.0 {
        active = ((t - 3.0) / 0.3).floor().to_int();
        if active >= chips.len() { active = 0; }
    }
    let cxp = 304.0;
    for i in 0..chips.len() {
        let label = chips[i];
        let cw = 46.0 + 15.5 * label.len().to_float();
        let e = pop(t, 1.75 + 0.11 * i.to_float(), fps, 12.0, 160.0);
        let a = clamp(e * 1.5, 0.0, 1.0) * fo;
        if a > 0.0 {
            let y = 515.0 + 26.0 * (1.0 - e);
            let first = i == active;
            let fill = if first { rgba(ORANGE, 0.22 * a) } else { rgba(INK, 0.06 * a) };
            let stroke = if first { rgba(ORANGE, 0.9 * a) } else { rgba(INK, 0.28 * a) };
            out.rect_stroke(cxp * k, y * k, cw * k, 56.0 * k, fill, stroke, 2.0 * k, 28.0 * k);
            out.text((cxp + 23.0) * k, (y + 37.0) * k, label, 26.0 * k, rgba(if first { ORANGE_SOFT } else { INK }, a));
        }
        cxp += cw + 14.0;
    }

    // ---------------------------------------------------------------- code card
    let ce = ease_out(seg(t, 0.55, 0.7));
    let ca = ce * fo;
    if ca > 0.0 {
        let oy = 50.0 * (1.0 - ce);
        let x0 = 1150.0;
        let y0 = 168.0 + oy;
        let cw = 620.0;
        let ch = 560.0;
        out.rect_stroke(x0 * k, y0 * k, cw * k, ch * k, rgba("#0d1f33", 0.85 * ca), rgba(INK, 0.14 * ca), 2.0 * k, 26.0 * k);
        for d in 0..3 {
            let dc = ["#ff5f57", "#febc2e", "#28c840"][d];
            out.circle((x0 + 34.0 + 24.0 * d.to_float()) * k, (y0 + 34.0) * k, 7.0 * k, rgba(dc, 0.85 * ca));
        }
        out.text((x0 + 120.0) * k, (y0 + 43.0) * k, "scene.rhai", 24.0 * k, rgba(MUTED, ca));
        out.rect(x0 * k, (y0 + 68.0) * k, cw * k, 2.0 * k, rgba(INK, 0.08 * ca));

        // Code line describing what the plot shows.
        out.text((x0 + 36.0) * k, (y0 + 118.0) * k, "let v =", 30.0 * k, rgba("#9cc4ff", ca));
        out.text((x0 + 138.0) * k, (y0 + 118.0) * k, "spring(frame, fps);", 30.0 * k, rgba(INK, ca));

        // Plot of the spring value over time, drawn up to the current time.
        let px0 = x0 + 46.0;
        let pw = 380.0;
        let py0 = y0 + 470.0;   // value 0
        let py1 = y0 + 230.0;   // value 1
        out.rect(px0 * k, py0 * k, pw * k, 2.0 * k, rgba(INK, 0.16 * ca));
        let dash = 0.0;
        while dash < pw {
            out.rect((px0 + dash) * k, py1 * k, 12.0 * k, 2.0 * k, rgba(ORANGE, 0.45 * ca));
            dash += 22.0;
        }
        out.text((px0 + pw + 12.0) * k, (py1 + 8.0) * k, "1.0", 20.0 * k, rgba(MUTED, ca));
        out.text((px0 + pw + 12.0) * k, (py0 + 8.0) * k, "0", 20.0 * k, rgba(MUTED, ca));

        let s0 = 1.2;      // spring start (seconds)
        let win = 1.9;     // seconds shown across the plot
        let elapsed = clamp(t - s0, 0.0, win);
        let v = 0.0;
        if elapsed > 0.0 {
            let n = 72;
            let d = "";
            for i in 0..=n {
                let st = elapsed * i.to_float() / n.to_float();
                let sv = spring(st * fps, fps, 7.0, 1.0, 110.0);
                let x = (px0 + pw * st / win) * k;
                let y = (py0 - (py0 - py1) * sv) * k;
                d += if i == 0 { "M" } else { " L" };
                d += " " + x + " " + y;
                v = sv;
            }
            out.path(d, "", rgba(ORANGE, ca), 4.0 * k, 1.0);
            let hx = (px0 + pw * elapsed / win) * k;
            let hy = (py0 - (py0 - py1) * v) * k;
            out.circle(hx, hy, 16.0 * k, rgba(ORANGE, 0.25 * ca));
            out.circle(hx, hy, 8.0 * k, rgba(INK, ca));
        }

        // The animated object driven by the same value.
        let ox = x0 + 505.0;
        let oyc = y0 + 350.0;
        let box = scene();
        let half = 48.0;
        box.rect_stroke(-half, -half, half * 2.0, half * 2.0, rgba(ORANGE, ca), rgba(ORANGE_SOFT, ca), 3.0, 22.0);
        let sc = fmax(v, 0.001) * k;
        let ang = (1.0 - v) * 120.0;
        // world = s * R * (local + offset)  =>  offset = R^-1 * centre / s
        let rad = -ang * PI() / 180.0;
        let cxw = ox * k;
        let cyw = oyc * k;
        let ux = (cxw * cos(rad) - cyw * sin(rad)) / sc;
        let uy = (cxw * sin(rad) + cyw * cos(rad)) / sc;
        if v > 0.001 {
            out.group(box, ux, uy, sc, ang, 1.0);
        }
        let iv = (v * 100.0).round().to_int();
        let frac = "" + (iv % 100).abs();
        if frac.len() < 2 { frac = "0" + frac; }
        out.text((ox - 62.0) * k, (y0 + 520.0) * k, "v = " + (iv / 100) + "." + frac, 26.0 * k, rgba(MUTED, ca));
    }

    // ---------------------------------------------------------------- timeline
    let tl = ease_out(seg(t, 0.0, 0.7));
    let ta = tl * fo;
    let tx0 = 160.0;
    let tw = 1600.0;
    let ty = 900.0;
    out.round_rect(tx0 * k, ty * k, tw * tl * k, 6.0 * k, rgba(INK, 0.12 * ta), 3.0 * k);
    let head = tx0 + tw * p;
    out.round_rect(tx0 * k, ty * k, fmax(0.0, (head - tx0)) * tl * k, 6.0 * k, rgba(ORANGE, ta), 3.0 * k);

    for s in 0..6 {
        let sx = tx0 + tw * s.to_float() / T;
        if sx <= tx0 + tw * tl + 0.5 {
            out.rect((sx - 1.0) * k, (ty + 16.0) * k, 2.0 * k, 12.0 * k, rgba(INK, 0.3 * ta));
            out.text((sx - 8.0) * k, (ty + 56.0) * k, "" + s + "s", 22.0 * k, rgba(MUTED, ta));
        }
    }

    // Keyframes light up when the playhead passes them.
    let keys = [0.15, 0.35, 0.55, 1.05, 1.2, 1.75];
    for kt in keys {
        let kx = tx0 + tw * kt / T;
        let lit = t >= kt;
        let r = if lit { 9.0 } else { 7.0 };
        let c = if lit { rgba(ORANGE_SOFT, ta) } else { rgba(INK, 0.35 * ta) };
        out.path("M " + (kx * k) + " " + ((ty + 3.0 - r) * k) + " L " + ((kx + r) * k) + " " + ((ty + 3.0) * k)
            + " L " + (kx * k) + " " + ((ty + 3.0 + r) * k) + " L " + ((kx - r) * k) + " " + ((ty + 3.0) * k) + " Z",
            c, "", 0.0, 1.0);
    }

    // Playhead.
    out.rect((head - 1.5) * k, (ty - 34.0) * k, 3.0 * k, 46.0 * k, rgba(ORANGE, ta));
    out.circle(head * k, (ty + 3.0) * k, 11.0 * k, rgba(INK, ta));
    out.circle(head * k, (ty + 3.0) * k, 6.0 * k, rgba(ORANGE, ta));

    out.text(tx0 * k, (ty - 30.0) * k, "" + ctx.width + " × " + ctx.height + "  ·  " + fps.round().to_int() + " fps", 22.0 * k, rgba(MUTED, ta));
    out.text_box((tx0 + tw - 400.0) * k, (ty - 58.0) * k, 400.0 * k, 40.0 * k,
        "frame " + pad3(ctx.frame) + " / " + pad3(ctx.duration), 22.0 * k, 10.0 * k, 1, rgba(MUTED, ta), "", "end");

    out
}
