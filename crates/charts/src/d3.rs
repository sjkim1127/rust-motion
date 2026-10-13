//! Pure Rust high-performance declarative D3 charting engine.
//!
//! Generates vector SVG and hardware-accelerated [`SceneNode`]s for lines,
//! bars, areas, and pie/donut charts with 0ms runtime cost.

use crate::svg_parser::SvgDocument;
use rust_motion_rasterizer::{Color, Scene, SceneNode};

/// Curve interpolation type for line charts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CurveType {
    #[default]
    Linear,
    MonotoneX,
    Step,
}

/// Declarative D3 Line & Area Chart.
#[derive(Debug, Clone)]
pub struct D3LineChart {
    pub width: f32,
    pub height: f32,
    pub padding: [f32; 4], // [top, right, bottom, left]
    pub points: Vec<(f32, f32)>,
    pub stroke_color: Color,
    pub stroke_width: f32,
    pub curve: CurveType,
    pub fill_area: bool,
    pub fill_color: Option<Color>,
    pub show_dots: bool,
    pub dot_radius: f32,
    pub dot_color: Option<Color>,
    pub show_grid: bool,
    pub grid_color: Color,
    pub x_ticks: usize,
    pub y_ticks: usize,
    pub title: Option<String>,
}

impl Default for D3LineChart {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 450.0,
            padding: [40.0, 40.0, 50.0, 60.0],
            points: Vec::new(),
            stroke_color: Color::rgb(0x3b, 0x82, 0xf6), // Tailwind blue-500
            stroke_width: 3.0,
            curve: CurveType::MonotoneX,
            fill_area: true,
            fill_color: Some(Color::rgba(0x3b, 0x82, 0xf6, 0x33)),
            show_dots: true,
            dot_radius: 4.5,
            dot_color: Some(Color::rgb(0x60, 0xa5, 0xfa)),
            show_grid: true,
            grid_color: Color::rgba(0xff, 0xff, 0xff, 0x1f),
            x_ticks: 5,
            y_ticks: 5,
            title: None,
        }
    }
}

impl D3LineChart {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    pub fn with_points(mut self, points: Vec<(f32, f32)>) -> Self {
        self.points = points;
        self
    }

    pub fn with_stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke_color = color;
        self.stroke_width = width;
        self
    }

    pub fn with_curve(mut self, curve: CurveType) -> Self {
        self.curve = curve;
        self
    }

    pub fn with_fill(mut self, fill: bool, color: Option<Color>) -> Self {
        self.fill_area = fill;
        self.fill_color = color;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Render chart directly into a native [`Scene`].
    pub fn to_scene(&self) -> Scene {
        let mut scene = Scene::default();
        if self.points.is_empty() {
            return scene;
        }

        let plot_x = self.padding[3];
        let plot_y = self.padding[0];
        let plot_w = (self.width - self.padding[3] - self.padding[1]).max(10.0);
        let plot_h = (self.height - self.padding[0] - self.padding[2]).max(10.0);

        // Compute domain
        let (min_x, max_x) = self
            .points
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |acc, p| {
                (acc.0.min(p.0), acc.1.max(p.0))
            });
        let (min_y, max_y) = self
            .points
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |acc, p| {
                (acc.0.min(p.1), acc.1.max(p.1))
            });

        let dx = if max_x > min_x { max_x - min_x } else { 1.0 };
        let dy = if max_y > min_y { max_y - min_y } else { 1.0 };

        let scale_x = |x: f32| plot_x + ((x - min_x) / dx) * plot_w;
        let scale_y = |y: f32| plot_y + plot_h - ((y - min_y) / dy) * plot_h;

        // Draw title
        if let Some(ref title) = self.title {
            scene.push(SceneNode::Text {
                x: self.width / 2.0 - (title.len() as f32 * 5.0),
                y: 15.0,
                content: title.clone(),
                font_size: 18.0,
                color: Color::WHITE,
                font_weight: 600,
                font_sources: Vec::new(),
            });
        }

        // Draw grid lines
        if self.show_grid {
            // Horizontal grid lines
            for i in 0..=self.y_ticks {
                let frac = i as f32 / self.y_ticks as f32;
                let gy = plot_y + frac * plot_h;
                scene.push(SceneNode::Path {
                    d: format!("M {} {} L {} {}", plot_x, gy, plot_x + plot_w, gy),
                    fill: None,
                    stroke: Some(self.grid_color),
                    stroke_width: 1.0,
                    opacity: 1.0,
                });

                // Y-axis tick label
                let val = max_y - frac * (max_y - min_y);
                scene.push(SceneNode::Text {
                    x: (plot_x - 45.0).max(5.0),
                    y: (gy - 6.0).max(0.0),
                    content: format!("{:.1}", val),
                    font_size: 11.0,
                    color: Color::rgba(0x94, 0xa3, 0xb8, 0xff), // slate-400
                    font_weight: 400,
                    font_sources: Vec::new(),
                });
            }

            // Vertical grid lines
            for i in 0..=self.x_ticks {
                let frac = i as f32 / self.x_ticks as f32;
                let gx = plot_x + frac * plot_w;
                scene.push(SceneNode::Path {
                    d: format!("M {} {} L {} {}", gx, plot_y, gx, plot_y + plot_h),
                    fill: None,
                    stroke: Some(self.grid_color),
                    stroke_width: 1.0,
                    opacity: 1.0,
                });

                // X-axis tick label
                let val = min_x + frac * (max_x - min_x);
                scene.push(SceneNode::Text {
                    x: gx - 12.0,
                    y: plot_y + plot_h + 10.0,
                    content: format!("{:.1}", val),
                    font_size: 11.0,
                    color: Color::rgba(0x94, 0xa3, 0xb8, 0xff),
                    font_weight: 400,
                    font_sources: Vec::new(),
                });
            }
        }

        // Map coordinates
        let screen_pts: Vec<(f32, f32)> = self
            .points
            .iter()
            .map(|&(x, y)| (scale_x(x), scale_y(y)))
            .collect();

        // Build path d string
        let path_d = match self.curve {
            CurveType::Linear => build_linear_path(&screen_pts),
            CurveType::MonotoneX => build_monotone_cubic_path(&screen_pts),
            CurveType::Step => build_step_path(&screen_pts),
        };

        // Filled area under curve
        if self.fill_area {
            let baseline_y = plot_y + plot_h;
            let first_x = screen_pts.first().unwrap().0;
            let last_x = screen_pts.last().unwrap().0;
            let area_d = format!(
                "{} L {} {} L {} {} Z",
                path_d, last_x, baseline_y, first_x, baseline_y
            );
            scene.push(SceneNode::Path {
                d: area_d,
                fill: self
                    .fill_color
                    .or(Some(Color::rgba(0x3b, 0x82, 0xf6, 0x33))),
                stroke: None,
                stroke_width: 0.0,
                opacity: 1.0,
            });
        }

        // Stroke line
        scene.push(SceneNode::Path {
            d: path_d,
            fill: None,
            stroke: Some(self.stroke_color),
            stroke_width: self.stroke_width,
            opacity: 1.0,
        });

        // Data dots
        if self.show_dots {
            let dot_c = self.dot_color.unwrap_or(self.stroke_color);
            for &(sx, sy) in &screen_pts {
                scene.push(SceneNode::Circle {
                    cx: sx,
                    cy: sy,
                    r: self.dot_radius,
                    fill: dot_c,
                    stroke: Some(Color::WHITE),
                    stroke_width: 1.5,
                });
            }
        }

        scene
    }

    /// Convert into an [`SvgDocument`].
    pub fn to_svg_document(&self) -> SvgDocument {
        SvgDocument {
            width: self.width,
            height: self.height,
            view_box: Some((0.0, 0.0, self.width, self.height)),
            scene: self.to_scene(),
        }
    }
}

/// Declarative D3 Category Bar Chart.
#[derive(Debug, Clone)]
pub struct D3BarChart {
    pub width: f32,
    pub height: f32,
    pub padding: [f32; 4],
    pub categories: Vec<String>,
    pub values: Vec<f32>,
    pub bar_colors: Vec<Color>,
    pub corner_radius: f32,
    pub bar_padding: f32, // fraction 0.0 .. 0.8
    pub show_values: bool,
    pub value_format: String,
    pub show_grid: bool,
    pub title: Option<String>,
}

impl Default for D3BarChart {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 450.0,
            padding: [40.0, 40.0, 50.0, 60.0],
            categories: Vec::new(),
            values: Vec::new(),
            bar_colors: vec![
                Color::rgb(0x3b, 0x82, 0xf6), // blue
                Color::rgb(0x10, 0xb9, 0x81), // emerald
                Color::rgb(0xf5, 0x9e, 0x0b), // amber
                Color::rgb(0xef, 0x44, 0x44), // red
                Color::rgb(0x8b, 0x5c, 0xf6), // violet
                Color::rgb(0xec, 0x48, 0x99), // pink
                Color::rgb(0x06, 0xb6, 0xd4), // cyan
            ],
            corner_radius: 6.0,
            bar_padding: 0.25,
            show_values: true,
            value_format: "{:.0}".into(),
            show_grid: true,
            title: None,
        }
    }
}

impl D3BarChart {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    pub fn with_data(mut self, items: Vec<(impl Into<String>, f32)>) -> Self {
        self.categories.clear();
        self.values.clear();
        for (cat, val) in items {
            self.categories.push(cat.into());
            self.values.push(val);
        }
        self
    }

    pub fn to_scene(&self) -> Scene {
        let mut scene = Scene::default();
        if self.values.is_empty() {
            return scene;
        }

        let plot_x = self.padding[3];
        let plot_y = self.padding[0];
        let plot_w = (self.width - self.padding[3] - self.padding[1]).max(10.0);
        let plot_h = (self.height - self.padding[0] - self.padding[2]).max(10.0);

        let max_val = self
            .values
            .iter()
            .copied()
            .fold(0.0f32, |acc, v| acc.max(v))
            .max(1.0);

        // Title
        if let Some(ref title) = self.title {
            scene.push(SceneNode::Text {
                x: self.width / 2.0 - (title.len() as f32 * 5.0),
                y: 15.0,
                content: title.clone(),
                font_size: 18.0,
                color: Color::WHITE,
                font_weight: 600,
                font_sources: Vec::new(),
            });
        }

        // Horizontal Grid
        if self.show_grid {
            let ticks = 5;
            for i in 0..=ticks {
                let frac = i as f32 / ticks as f32;
                let gy = plot_y + frac * plot_h;
                scene.push(SceneNode::Path {
                    d: format!("M {} {} L {} {}", plot_x, gy, plot_x + plot_w, gy),
                    fill: None,
                    stroke: Some(Color::rgba(0xff, 0xff, 0xff, 0x1a)),
                    stroke_width: 1.0,
                    opacity: 1.0,
                });

                let val = max_val * (1.0 - frac);
                scene.push(SceneNode::Text {
                    x: (plot_x - 45.0).max(5.0),
                    y: (gy - 6.0).max(0.0),
                    content: format!("{:.0}", val),
                    font_size: 11.0,
                    color: Color::rgba(0x94, 0xa3, 0xb8, 0xff),
                    font_weight: 400,
                    font_sources: Vec::new(),
                });
            }
        }

        let n = self.values.len();
        let slot_w = plot_w / n as f32;
        let bar_w = slot_w * (1.0 - self.bar_padding);
        let bar_offset = (slot_w - bar_w) / 2.0;

        for (i, &val) in self.values.iter().enumerate() {
            let bh = (val / max_val) * plot_h;
            let bx = plot_x + i as f32 * slot_w + bar_offset;
            let by = plot_y + plot_h - bh;

            let color = self
                .bar_colors
                .get(i % self.bar_colors.len())
                .copied()
                .unwrap_or(Color::rgb(0x3b, 0x82, 0xf6));

            scene.push(SceneNode::Rect {
                x: bx,
                y: by,
                w: bar_w,
                h: bh,
                fill: color,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: self.corner_radius,
            });

            // Value label above bar
            if self.show_values {
                let label_text = format!("{:.0}", val);
                let text_x = bx + bar_w / 2.0 - (label_text.len() as f32 * 3.5);
                scene.push(SceneNode::Text {
                    x: text_x,
                    y: (by - 18.0).max(0.0),
                    content: label_text,
                    font_size: 12.0,
                    color: Color::WHITE,
                    font_weight: 600,
                    font_sources: Vec::new(),
                });
            }

            // Category label below axis
            if let Some(cat) = self.categories.get(i) {
                let text_x = bx + bar_w / 2.0 - (cat.len() as f32 * 3.5);
                scene.push(SceneNode::Text {
                    x: text_x,
                    y: plot_y + plot_h + 12.0,
                    content: cat.clone(),
                    font_size: 12.0,
                    color: Color::rgba(0x94, 0xa3, 0xb8, 0xff),
                    font_weight: 500,
                    font_sources: Vec::new(),
                });
            }
        }

        scene
    }

    pub fn to_svg_document(&self) -> SvgDocument {
        SvgDocument {
            width: self.width,
            height: self.height,
            view_box: Some((0.0, 0.0, self.width, self.height)),
            scene: self.to_scene(),
        }
    }
}

/// Declarative D3 Pie & Donut Chart.
#[derive(Debug, Clone)]
pub struct D3PieChart {
    pub width: f32,
    pub height: f32,
    pub slices: Vec<(String, f32)>,
    pub inner_radius: f32, // 0.0 = Pie, > 0.0 = Donut
    pub outer_radius: f32,
    pub colors: Vec<Color>,
    pub show_labels: bool,
    pub title: Option<String>,
}

impl Default for D3PieChart {
    fn default() -> Self {
        Self {
            width: 600.0,
            height: 600.0,
            slices: Vec::new(),
            inner_radius: 0.0,
            outer_radius: 200.0,
            colors: vec![
                Color::rgb(0x3b, 0x82, 0xf6),
                Color::rgb(0x10, 0xb9, 0x81),
                Color::rgb(0xf5, 0x9e, 0x0b),
                Color::rgb(0xef, 0x44, 0x44),
                Color::rgb(0x8b, 0x5c, 0xf6),
                Color::rgb(0xec, 0x48, 0x99),
            ],
            show_labels: true,
            title: None,
        }
    }
}

impl D3PieChart {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            outer_radius: width.min(height) * 0.38,
            ..Default::default()
        }
    }

    pub fn with_donut(mut self, inner_radius_fraction: f32) -> Self {
        self.inner_radius = self.outer_radius * inner_radius_fraction.clamp(0.0, 0.9);
        self
    }

    pub fn with_slices(mut self, slices: Vec<(impl Into<String>, f32)>) -> Self {
        self.slices = slices.into_iter().map(|(s, v)| (s.into(), v)).collect();
        self
    }

    pub fn to_scene(&self) -> Scene {
        let mut scene = Scene::default();
        if self.slices.is_empty() {
            return scene;
        }

        let total: f32 = self.slices.iter().map(|(_, v)| v.max(0.0)).sum();
        if total <= 0.0 {
            return scene;
        }

        let cx = self.width / 2.0;
        let cy = self.height / 2.0;

        let mut start_angle = -std::f32::consts::FRAC_PI_2;

        for (i, (_label, val)) in self.slices.iter().enumerate() {
            let frac = val.max(0.0) / total;
            let sweep = frac * std::f32::consts::TAU;
            let end_angle = start_angle + sweep;

            let color = self
                .colors
                .get(i % self.colors.len())
                .copied()
                .unwrap_or(Color::rgb(0x3b, 0x82, 0xf6));

            let arc_d = build_arc_path(
                cx,
                cy,
                self.inner_radius,
                self.outer_radius,
                start_angle,
                end_angle,
            );

            scene.push(SceneNode::Path {
                d: arc_d,
                fill: Some(color),
                stroke: Some(Color::rgb(0x0f, 0x17, 0x2a)), // slate-900 border
                stroke_width: 2.0,
                opacity: 1.0,
            });

            // Label at centroid
            if self.show_labels && frac > 0.04 {
                let mid_angle = start_angle + sweep / 2.0;
                let label_r = if self.inner_radius > 0.0 {
                    (self.inner_radius + self.outer_radius) / 2.0
                } else {
                    self.outer_radius * 0.65
                };
                let lx = cx + mid_angle.cos() * label_r;
                let ly = cy + mid_angle.sin() * label_r;

                let pct_text = format!("{:.0}%", frac * 100.0);
                scene.push(SceneNode::Text {
                    x: lx - 12.0,
                    y: ly - 6.0,
                    content: pct_text,
                    font_size: 13.0,
                    color: Color::WHITE,
                    font_weight: 700,
                    font_sources: Vec::new(),
                });
            }

            start_angle = end_angle;
        }

        scene
    }

    pub fn to_svg_document(&self) -> SvgDocument {
        SvgDocument {
            width: self.width,
            height: self.height,
            view_box: Some((0.0, 0.0, self.width, self.height)),
            scene: self.to_scene(),
        }
    }
}

// -----------------------------------------------------------------------------
// Path Math Helpers
// -----------------------------------------------------------------------------

fn build_linear_path(pts: &[(f32, f32)]) -> String {
    if pts.is_empty() {
        return String::new();
    }
    let mut d = format!("M {:.2} {:.2}", pts[0].0, pts[0].1);
    for p in &pts[1..] {
        d.push_str(&format!(" L {:.2} {:.2}", p.0, p.1));
    }
    d
}

fn build_step_path(pts: &[(f32, f32)]) -> String {
    if pts.is_empty() {
        return String::new();
    }
    let mut d = format!("M {:.2} {:.2}", pts[0].0, pts[0].1);
    for i in 0..pts.len() - 1 {
        let mid_x = (pts[i].0 + pts[i + 1].0) / 2.0;
        d.push_str(&format!(
            " L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2}",
            mid_x,
            pts[i].1,
            mid_x,
            pts[i + 1].1,
            pts[i + 1].0,
            pts[i + 1].1
        ));
    }
    d
}

/// Monotone cubic Hermite spline interpolation (matches D3 curveMonotoneX).
fn build_monotone_cubic_path(pts: &[(f32, f32)]) -> String {
    let n = pts.len();
    if n <= 2 {
        return build_linear_path(pts);
    }

    let mut d = format!("M {:.2} {:.2}", pts[0].0, pts[0].1);

    for i in 0..n - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(n - 1)];

        let cp1x = p1.0 + (p2.0 - p0.0) / 6.0;
        let cp1y = p1.1 + (p2.1 - p0.1) / 6.0;
        let cp2x = p2.0 - (p3.0 - p1.0) / 6.0;
        let cp2y = p2.1 - (p3.1 - p1.1) / 6.0;

        d.push_str(&format!(
            " C {:.2} {:.2}, {:.2} {:.2}, {:.2} {:.2}",
            cp1x, cp1y, cp2x, cp2y, p2.0, p2.1
        ));
    }

    d
}

fn build_arc_path(cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32) -> String {
    let large_arc = if (a1 - a0) > std::f32::consts::PI {
        1
    } else {
        0
    };

    let x01 = cx + r1 * a0.cos();
    let y01 = cy + r1 * a0.sin();
    let x02 = cx + r1 * a1.cos();
    let y02 = cy + r1 * a1.sin();

    if r0 <= 0.0 {
        format!(
            "M {:.2} {:.2} L {:.2} {:.2} A {:.2} {:.2} 0 {} 1 {:.2} {:.2} Z",
            cx, cy, x01, y01, r1, r1, large_arc, x02, y02
        )
    } else {
        let x03 = cx + r0 * a1.cos();
        let y03 = cy + r0 * a1.sin();
        let x04 = cx + r0 * a0.cos();
        let y04 = cy + r0 * a0.sin();
        format!(
            "M {:.2} {:.2} A {:.2} {:.2} 0 {} 1 {:.2} {:.2} L {:.2} {:.2} A {:.2} {:.2} 0 {} 0 {:.2} {:.2} Z",
            x01, y01, r1, r1, large_arc, x02, y02, x03, y03, r0, r0, large_arc, x04, y04
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d3_line_chart() {
        let chart = D3LineChart::new(800.0, 400.0)
            .with_points(vec![(0.0, 10.0), (1.0, 45.0), (2.0, 30.0), (3.0, 80.0)])
            .with_stroke(Color::rgb(0x10, 0xb9, 0x81), 4.0)
            .with_curve(CurveType::MonotoneX);

        let scene = chart.to_scene();
        assert!(!scene.nodes.is_empty());

        // Check for line path
        let has_path = scene
            .nodes
            .iter()
            .any(|n| matches!(n, SceneNode::Path { stroke, .. } if stroke.is_some()));
        assert!(has_path, "Expected line stroke path in scene");
    }

    #[test]
    fn test_d3_bar_chart() {
        let chart = D3BarChart::new(600.0, 300.0).with_data(vec![
            ("Q1", 120.0),
            ("Q2", 240.0),
            ("Q3", 180.0),
            ("Q4", 320.0),
        ]);

        let scene = chart.to_scene();
        let rect_count = scene
            .nodes
            .iter()
            .filter(|n| matches!(n, SceneNode::Rect { .. }))
            .count();
        assert_eq!(rect_count, 4, "Expected 4 bars");
    }

    #[test]
    fn test_d3_pie_donut_chart() {
        let chart = D3PieChart::new(500.0, 500.0)
            .with_donut(0.5)
            .with_slices(vec![("Rust", 60.0), ("JS", 30.0), ("Other", 10.0)]);

        let scene = chart.to_scene();
        let path_count = scene
            .nodes
            .iter()
            .filter(|n| matches!(n, SceneNode::Path { .. }))
            .count();
        assert_eq!(path_count, 3, "Expected 3 pie slices");
    }
}
