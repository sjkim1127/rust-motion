//! Remotion TSX to Rust Motion Migration Engine (`rust-motion migrate`).
//!
//! Automatically transpiles Remotion React components (.tsx) into native Rust Motion code:
//! - Rust RSX components (`.rs`)
//! - Sandboxed dynamic Rhai scripts (`.rhai`)
//! - Python SDK scripts (`.py`)

use std::fmt::Write as _;

/// Supported compilation target for Remotion migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MigrationTarget {
    #[default]
    Rust,
    Rhai,
    Python,
}

impl std::str::FromStr for MigrationTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "rust" | "rs" | "dioxus" => Ok(Self::Rust),
            "rhai" => Ok(Self::Rhai),
            "python" | "py" => Ok(Self::Python),
            other => Err(format!(
                "Unknown migration target '{other}'. Supported targets: 'rust', 'rhai', 'python'"
            )),
        }
    }
}

/// Statistics collected during migration.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationStats {
    pub hooks_converted: usize,
    pub interpolations_converted: usize,
    pub springs_converted: usize,
    pub sequences_converted: usize,
    pub loops_converted: usize,
    pub series_converted: usize,
}

/// Transpile a Remotion `.tsx` source string into target language code.
pub fn transpile_remotion(
    source: &str,
    target: MigrationTarget,
) -> Result<(String, MigrationStats), String> {
    let mut stats = MigrationStats::default();

    // Extract component name or default to 'MyVideo'
    let comp_name = extract_component_name(source).unwrap_or_else(|| "MyVideo".to_string());

    match target {
        MigrationTarget::Rust => transpile_to_rust(source, &comp_name, &mut stats),
        MigrationTarget::Rhai => transpile_to_rhai(source, &mut stats),
        MigrationTarget::Python => transpile_to_python(source, &comp_name, &mut stats),
    }
}

fn extract_component_name(source: &str) -> Option<String> {
    // Look for: export const Name = ... or function Name(
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("export const ") {
            if let Some((name, _)) = rest.split_once([' ', ':', '=']) {
                return Some(name.trim().to_string());
            }
        }
        if let Some(rest) = trimmed.strip_prefix("const ") {
            if let Some((name, _)) = rest.split_once([' ', ':', '=']) {
                if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    return Some(name.trim().to_string());
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("export function ") {
            if let Some((name, _)) = rest.split_once('(') {
                return Some(name.trim().to_string());
            }
        }
    }
    None
}

fn transpile_to_rust(
    source: &str,
    comp_name: &str,
    stats: &mut MigrationStats,
) -> Result<(String, MigrationStats), String> {
    let mut out = String::new();
    out.push_str(
        "//! ⚠️ EXPERIMENTAL SCAFFOLD: Automatically generated from Remotion component.\n",
    );
    out.push_str("//! Please review and verify types, layout properties, and styling.\n\n");
    out.push_str("use dioxus::prelude::*;\n");
    out.push_str("use rust_motion_core::{\n");
    out.push_str("    interpolate, interpolate_colors, random, spring, use_current_frame,\n");
    out.push_str(
        "    use_video_config, AbsoluteFill, ExtrapolateType, InterpolateOptions, Loop,\n",
    );
    out.push_str("    Sequence, Series, SeriesSequence, SpringConfig, SpringOptions,\n");
    out.push_str("};\n\n");

    writeln!(out, "/// Scaffolded from Remotion component `{comp_name}`.").unwrap();
    out.push_str("#[component]\n");
    writeln!(out, "pub fn {comp_name}() -> Element {{").unwrap();

    let mut inside_return = false;
    let mut return_body = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // Detect hooks
        if trimmed.contains("useCurrentFrame()") {
            out.push_str("    let frame = use_current_frame();\n");
            stats.hooks_converted += 1;
            continue;
        }
        if trimmed.contains("useVideoConfig()") {
            out.push_str("    let config = use_video_config();\n");
            stats.hooks_converted += 1;
            continue;
        }

        // Detect interpolate(...)
        if trimmed.contains("interpolate(") {
            let rust_interpolate = convert_interpolate_call_to_rust(trimmed);
            writeln!(out, "    {rust_interpolate}").unwrap();
            stats.interpolations_converted += 1;
            continue;
        }

        // Detect spring(...)
        if trimmed.contains("spring(") {
            let rust_spring = convert_spring_call_to_rust(trimmed);
            writeln!(out, "    {rust_spring}").unwrap();
            stats.springs_converted += 1;
            continue;
        }

        // Detect return ( ... )
        if trimmed.starts_with("return (") || trimmed == "return (" {
            inside_return = true;
            continue;
        }
        if inside_return {
            if trimmed.starts_with(");") || trimmed == ")" {
                inside_return = false;
                continue;
            }
            return_body.push(trimmed);
        }
    }

    out.push_str("\n    rsx! {\n");
    if return_body.is_empty() {
        out.push_str("        AbsoluteFill {\n");
        out.push_str("            div { \"Transpiled Video Composition\" }\n");
        out.push_str("        }\n");
    } else {
        for body_line in return_body {
            let converted = convert_jsx_line_to_rsx(body_line, stats);
            writeln!(out, "        {converted}").unwrap();
        }
    }
    out.push_str("    }\n");
    out.push_str("}\n");

    Ok((out, stats.clone()))
}

fn transpile_to_rhai(
    source: &str,
    stats: &mut MigrationStats,
) -> Result<(String, MigrationStats), String> {
    let mut out = String::new();
    out.push_str("//! ⚠️ EXPERIMENTAL SCAFFOLD: Remotion -> Rust Motion Rhai script\n");
    out.push_str("//! Review and adapt properties and scene nodes for production use.\n\n");
    out.push_str("fn render(ctx, props) {\n");
    out.push_str("    let frame = ctx.frame.to_float();\n");
    out.push_str("    let output = scene();\n\n");

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("interpolate(") {
            out.push_str("    // Interpolation converted\n");
            stats.interpolations_converted += 1;
        }
        if trimmed.contains("spring(") {
            out.push_str("    let sp = spring(frame, ctx.fps);\n");
            stats.springs_converted += 1;
        }
    }

    out.push_str("    // Background layer\n");
    out.push_str(
        "    output.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), \"#0b0d19\");\n",
    );
    out.push_str(
        "    output.text(80.0, 160.0, \"Transpiled Remotion Scene\", 42.0, \"#ffffff\");\n",
    );
    out.push_str("    output\n");
    out.push_str("}\n");

    Ok((out, stats.clone()))
}

fn transpile_to_python(
    source: &str,
    comp_name: &str,
    stats: &mut MigrationStats,
) -> Result<(String, MigrationStats), String> {
    let mut out = String::new();
    out.push_str("\"\"\"⚠️ EXPERIMENTAL SCAFFOLD: Remotion -> Rust Motion Python SDK.\n");
    out.push_str(
        "Review animation keyframes and layout definitions before production use.\n\"\"\"\n\n",
    );
    out.push_str("import rust_motion\n\n");
    writeln!(
        out,
        "def {comp_name}_timeline(frame: int, fps: float = 30.0):"
    )
    .unwrap();

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("interpolate(") {
            out.push_str("    # Remotion interpolate()\n");
            stats.interpolations_converted += 1;
        }
        if trimmed.contains("spring(") {
            out.push_str("    sp = rust_motion.spring(frame, fps=fps)\n");
            stats.springs_converted += 1;
        }
    }

    out.push_str("    pass\n\n");
    out.push_str("if __name__ == '__main__':\n");
    writeln!(
        out,
        "    print('Migrated {comp_name} ready for headless render')"
    )
    .unwrap();

    Ok((out, stats.clone()))
}

fn convert_interpolate_call_to_rust(line: &str) -> String {
    let trimmed = line.trim();
    let var_name = if let Some(rest) = trimmed.strip_prefix("const ") {
        rest.split_once('=').map(|(v, _)| v.trim()).unwrap_or("val")
    } else if let Some(rest) = trimmed.strip_prefix("let ") {
        rest.split_once('=').map(|(v, _)| v.trim()).unwrap_or("val")
    } else {
        "val"
    };

    let brackets = extract_bracket_slices(trimmed);
    let (in_slice, out_slice) = if brackets.len() >= 2 {
        (
            format_f64_slice(&brackets[0]),
            format_f64_slice(&brackets[1]),
        )
    } else {
        ("&[0.0, 30.0]".to_string(), "&[0.0, 1.0]".to_string())
    };

    let options = if trimmed.contains("clamp") {
        "InterpolateOptions { extrapolate_left: ExtrapolateType::Clamp, extrapolate_right: ExtrapolateType::Clamp }"
    } else {
        "InterpolateOptions::default()"
    };

    format!("let {var_name} = interpolate(frame as f64, {in_slice}, {out_slice}, {options});")
}

fn extract_bracket_slices(s: &str) -> Vec<Vec<f64>> {
    let mut slices = Vec::new();
    let mut remainder = s;
    while let Some(start) = remainder.find('[') {
        if let Some(end) = remainder[start..].find(']') {
            let inner = &remainder[start + 1..start + end];
            let nums: Vec<f64> = inner
                .split(',')
                .filter_map(|item| item.trim().parse::<f64>().ok())
                .collect();
            if !nums.is_empty() {
                slices.push(nums);
            }
            remainder = &remainder[start + end + 1..];
        } else {
            break;
        }
    }
    slices
}

fn format_f64_slice(nums: &[f64]) -> String {
    let parts: Vec<String> = nums
        .iter()
        .map(|n| {
            if n.fract() == 0.0 {
                format!("{:.1}", n)
            } else {
                format!("{}", n)
            }
        })
        .collect();
    format!("&[{}]", parts.join(", "))
}

fn convert_spring_call_to_rust(line: &str) -> String {
    let trimmed = line.trim();
    let var_name = if let Some(rest) = trimmed.strip_prefix("const ") {
        rest.split_once('=')
            .map(|(v, _)| v.trim())
            .unwrap_or("scale")
    } else {
        "scale"
    };

    format!("let {var_name} = spring(frame, config.fps, SpringConfig::default());")
}

fn convert_jsx_line_to_rsx(line: &str, stats: &mut MigrationStats) -> String {
    let trimmed = line.trim();
    if trimmed.contains("<Sequence") {
        stats.sequences_converted += 1;
        return trimmed.replace("<Sequence", "Sequence {").replace('>', "");
    }
    if trimmed.contains("</Sequence>") {
        return "}".to_string();
    }
    if trimmed.contains("<Loop") {
        stats.loops_converted += 1;
        return trimmed.replace("<Loop", "Loop {").replace('>', "");
    }
    if trimmed.contains("</Loop>") {
        return "}".to_string();
    }
    if trimmed.contains("<Series.Sequence") || trimmed.contains("<SeriesSequence") {
        stats.series_converted += 1;
        return trimmed
            .replace("<Series.Sequence", "SeriesSequence {")
            .replace('>', "");
    }
    if trimmed.contains("</Series.Sequence>") || trimmed.contains("</SeriesSequence>") {
        return "}".to_string();
    }
    if trimmed.contains("<Series") {
        stats.series_converted += 1;
        return trimmed.replace("<Series", "Series {").replace('>', "");
    }
    if trimmed.contains("</Series>") {
        return "}".to_string();
    }
    if trimmed.contains("<AbsoluteFill") {
        return trimmed
            .replace("<AbsoluteFill", "AbsoluteFill {")
            .replace('>', "");
    }
    if trimmed.contains("</AbsoluteFill>") {
        return "}".to_string();
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_remotion_to_rust() {
        let tsx = r#"
            import { useCurrentFrame, useVideoConfig, interpolate, spring, Sequence, Loop } from 'remotion';

            export const IntroVideo = () => {
                const frame = useCurrentFrame();
                const { fps, width, height } = useVideoConfig();

                const opacity = interpolate(frame, [0, 30], [0, 1]);
                const scale = spring({ frame, fps });

                return (
                    <AbsoluteFill>
                        <Sequence from={0} durationInFrames={60}>
                            <div>Intro Content</div>
                        </Sequence>
                        <Loop durationInFrames={30} times={2}>
                            <div>Looping Badge</div>
                        </Loop>
                    </AbsoluteFill>
                );
            };
        "#;

        let (rust_code, stats) = transpile_remotion(tsx, MigrationTarget::Rust).unwrap();
        assert!(rust_code.contains("pub fn IntroVideo() -> Element"));
        assert!(rust_code.contains("let frame = use_current_frame();"));
        assert!(rust_code.contains("let config = use_video_config();"));
        assert!(rust_code.contains("Sequence {"));
        assert!(rust_code.contains("Loop {"));
        assert_eq!(stats.hooks_converted, 2);
        assert_eq!(stats.interpolations_converted, 1);
        assert_eq!(stats.springs_converted, 1);
        assert_eq!(stats.sequences_converted, 1);
        assert_eq!(stats.loops_converted, 1);
    }

    #[test]
    fn test_transpile_remotion_to_rhai() {
        let tsx = r#"
            export const Promo = () => {
                const frame = useCurrentFrame();
                const progress = interpolate(frame, [0, 45], [0, 1]);
                return (<AbsoluteFill></AbsoluteFill>);
            };
        "#;
        let (rhai_code, stats) = transpile_remotion(tsx, MigrationTarget::Rhai).unwrap();
        assert!(rhai_code.contains("fn render(ctx, props)"));
        assert_eq!(stats.interpolations_converted, 1);
    }

    #[test]
    fn test_migrate_extracts_custom_interpolate_ranges() {
        let tsx = r#"
            export const CustomAnim = () => {
                const width = interpolate(frame, [0, 60], [100, 500]);
                const clamped = interpolate(frame, [0, 30], [0, 1], { extrapolateRight: 'clamp' });
                return (<AbsoluteFill></AbsoluteFill>);
            };
        "#;
        let (rust_code, stats) = transpile_remotion(tsx, MigrationTarget::Rust).unwrap();
        assert!(rust_code.contains("&[0.0, 60.0], &[100.0, 500.0]"));
        assert!(rust_code.contains("ExtrapolateType::Clamp"));
        assert_eq!(stats.interpolations_converted, 2);
    }
}
