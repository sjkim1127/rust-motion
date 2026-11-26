//! Scene builder methods for SVG, Mermaid and D3-style chart rendering.

use super::*;

impl SceneBuilder {
    pub(super) fn svg(&mut self, markup: &str) -> RhaiResult<()> {
        let doc = rust_motion_charts::parse_svg(markup)
            .map_err(|e| runtime_error(format!("SVG parse error: {e}")))?;
        self.scene.nodes.extend(doc.scene.nodes);
        Ok(())
    }

    pub(super) fn svg_at(&mut self, x: FLOAT, y: FLOAT, markup: &str) -> RhaiResult<()> {
        let doc = rust_motion_charts::parse_svg(markup)
            .map_err(|e| runtime_error(format!("SVG parse error: {e}")))?;
        let group = SceneNode::Group {
            transform: Transform2D::translate(finite_f32("x", x)?, finite_f32("y", y)?),
            opacity: 1.0,
            children: doc.scene.nodes,
        };
        self.scene.push(group);
        Ok(())
    }

    pub(super) fn mermaid(&mut self, code: &str) -> RhaiResult<()> {
        let scene = rust_motion_charts::render_mermaid(code, None)
            .map_err(|e| runtime_error(format!("Mermaid error: {e}")))?;
        self.scene.nodes.extend(scene.nodes);
        Ok(())
    }

    pub(super) fn mermaid_opts(&mut self, code: &str, opts: Map) -> RhaiResult<()> {
        let theme_str = get_opt_string(&opts, "theme");
        let theme = match theme_str.as_deref() {
            Some("light") => rust_motion_charts::DiagramTheme::light(),
            _ => rust_motion_charts::DiagramTheme::dark(),
        };
        let scene = rust_motion_charts::render_mermaid(code, Some(&theme))
            .map_err(|e| runtime_error(format!("Mermaid error: {e}")))?;

        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    pub(super) fn d3_line(&mut self, data: Dynamic, opts: Map) -> RhaiResult<()> {
        let width = get_opt_f32(&opts, "width", 800.0);
        let height = get_opt_f32(&opts, "height", 450.0);
        let stroke_width = get_opt_f32(&opts, "stroke_width", 3.0);
        let mut chart = rust_motion_charts::D3LineChart {
            width,
            height,
            stroke_width,
            ..Default::default()
        };
        if let Some(s) = get_opt_string(&opts, "stroke") {
            chart.stroke_color = parse_color(&s)?;
        }
        if let Some(c) = get_opt_string(&opts, "curve") {
            chart.curve = match c.as_str() {
                "linear" => rust_motion_charts::CurveType::Linear,
                "step" => rust_motion_charts::CurveType::Step,
                _ => rust_motion_charts::CurveType::MonotoneX,
            };
        }
        if let Some(f) = opts.get("fill") {
            if let Ok(b) = f.as_bool() {
                chart.fill_area = b;
            }
        }
        if let Some(fc) = get_opt_string(&opts, "fill_color") {
            chart.fill_color = Some(parse_color(&fc)?);
        }
        if let Some(t) = get_opt_string(&opts, "title") {
            chart.title = Some(t);
        }

        let mut points = Vec::new();
        if let Ok(arr) = data.clone().into_array() {
            for (idx, item) in arr.into_iter().enumerate() {
                if let Ok(pair) = item.clone().into_array() {
                    if pair.len() >= 2 {
                        let px = pair[0]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[0].as_int().map(|i| i as f32))
                            .unwrap_or(idx as f32);
                        let py = pair[1]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[1].as_int().map(|i| i as f32))
                            .unwrap_or(0.0);
                        points.push((px, py));
                    }
                } else if let Ok(f) = item.as_float() {
                    points.push((idx as f32, f as f32));
                } else if let Ok(i) = item.as_int() {
                    points.push((idx as f32, i as f32));
                }
            }
        }
        chart.points = points;

        let scene = chart.to_scene();
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    pub(super) fn d3_bar(&mut self, data: Dynamic, opts: Map) -> RhaiResult<()> {
        let width = get_opt_f32(&opts, "width", 800.0);
        let height = get_opt_f32(&opts, "height", 450.0);
        let corner_radius = get_opt_f32(&opts, "corner_radius", 6.0);
        let mut chart = rust_motion_charts::D3BarChart {
            width,
            height,
            corner_radius,
            ..Default::default()
        };
        if let Some(t) = get_opt_string(&opts, "title") {
            chart.title = Some(t);
        }

        if let Ok(arr) = data.clone().into_array() {
            for (idx, item) in arr.into_iter().enumerate() {
                if let Ok(pair) = item.clone().into_array() {
                    if pair.len() >= 2 {
                        let label = pair[0]
                            .clone()
                            .into_string()
                            .unwrap_or_else(|_| format!("{idx}"));
                        let val = pair[1]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[1].as_int().map(|i| i as f32))
                            .unwrap_or(0.0);
                        chart.categories.push(label);
                        chart.values.push(val);
                    }
                } else if let Ok(f) = item.as_float() {
                    chart.categories.push(format!("{idx}"));
                    chart.values.push(f as f32);
                } else if let Ok(i) = item.as_int() {
                    chart.categories.push(format!("{idx}"));
                    chart.values.push(i as f32);
                }
            }
        }

        let scene = chart.to_scene();
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    pub(super) fn d3_pie(&mut self, data: Dynamic, opts: Map) -> RhaiResult<()> {
        let width = get_opt_f32(&opts, "width", 600.0);
        let height = get_opt_f32(&opts, "height", 600.0);
        let outer_radius = width.min(height) * 0.38;
        let mut chart = rust_motion_charts::D3PieChart {
            width,
            height,
            outer_radius,
            ..Default::default()
        };
        let donut_fraction = get_opt_f32(&opts, "donut", 0.0);
        if donut_fraction > 0.0 {
            chart = chart.with_donut(donut_fraction);
        }
        if let Some(t) = get_opt_string(&opts, "title") {
            chart.title = Some(t);
        }

        if let Ok(arr) = data.clone().into_array() {
            for (idx, item) in arr.into_iter().enumerate() {
                if let Ok(pair) = item.clone().into_array() {
                    if pair.len() >= 2 {
                        let label = pair[0]
                            .clone()
                            .into_string()
                            .unwrap_or_else(|_| format!("{idx}"));
                        let val = pair[1]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[1].as_int().map(|i| i as f32))
                            .unwrap_or(0.0);
                        chart.slices.push((label, val));
                    }
                } else if let Ok(f) = item.as_float() {
                    chart.slices.push((format!("{idx}"), f as f32));
                } else if let Ok(i) = item.as_int() {
                    chart.slices.push((format!("{idx}"), i as f32));
                }
            }
        }

        let scene = chart.to_scene();
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    pub(super) fn d3_eval(&mut self, script: &str, data: Dynamic) -> RhaiResult<()> {
        let json_val = rhai::serde::from_dynamic::<Value>(&data)
            .map_err(|e| runtime_error(format!("Failed to serialize data for QuickJS D3: {e}")))?;

        thread_local! {
            static JS_ENGINE: std::cell::RefCell<Option<rust_motion_charts::QuickJsEngine>> = const { std::cell::RefCell::new(None) };
        }

        let doc = JS_ENGINE
            .with(
                |cell| -> Result<rust_motion_charts::SvgDocument, rust_motion_charts::ChartError> {
                    let mut borrow = cell.borrow_mut();
                    if borrow.is_none() {
                        *borrow = Some(rust_motion_charts::QuickJsEngine::new()?);
                    }
                    borrow.as_ref().unwrap().eval_d3(script, &json_val)
                },
            )
            .map_err(|e| runtime_error(format!("QuickJS D3 execution error: {e}")))?;

        self.scene.nodes.extend(doc.scene.nodes);
        Ok(())
    }
}
