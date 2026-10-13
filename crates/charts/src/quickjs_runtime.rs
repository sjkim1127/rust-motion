//! QuickJS embedded runtime for executing JS-based charting and diagram generators.

use crate::svg_parser::{parse_svg, SvgDocument};
use rquickjs::{Context, Runtime};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChartError {
    #[error("QuickJS execution error: {0}")]
    JsError(String),
    #[error("SVG parsing error: {0}")]
    SvgError(String),
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

const DOM_SHIM_JS: &str = r#"
class Element {
    constructor(tag) {
        this.tagName = tag.toLowerCase();
        this.attributes = {};
        this.children = [];
        this.textContent = '';
        this.style = {};
    }
    setAttribute(name, value) {
        this.attributes[name] = String(value);
        return this;
    }
    getAttribute(name) {
        return this.attributes[name];
    }
    removeAttribute(name) {
        delete this.attributes[name];
        return this;
    }
    appendChild(child) {
        if (typeof child === 'string') {
            this.textContent += child;
        } else if (child) {
            this.children.push(child);
        }
        return child;
    }
    get outerHTML() {
        let attrs = Object.entries(this.attributes)
            .map(([k, v]) => ` ${k}="${String(v).replace(/"/g, '&quot;')}"`)
            .join('');
        if (this.children.length === 0 && !this.textContent) {
            return `<${this.tagName}${attrs}/>`;
        }
        let inner = this.textContent || this.children.map(c => c.outerHTML).join('');
        return `<${this.tagName}${attrs}>${inner}</${this.tagName}>`;
    }
}

const document = {
    createElementNS(ns, tag) { return new Element(tag); },
    createElement(tag) { return new Element(tag); },
    createTextNode(text) {
        const el = new Element('#text');
        el.textContent = String(text);
        return el;
    }
};

const window = { document };
globalThis.document = document;
globalThis.window = window;
globalThis.Element = Element;
"#;

const D3_CORE_JS: &str = r##"
function scaleLinear() {
    let domain = [0, 1];
    let range = [0, 1];
    function scale(x) {
        const [d0, d1] = domain;
        const [r0, r1] = range;
        if (d1 === d0) return r0;
        return r0 + ((x - d0) / (d1 - d0)) * (r1 - r0);
    }
    scale.domain = function(d) { if (!arguments.length) return domain; domain = d; return scale; };
    scale.range = function(r) { if (!arguments.length) return range; range = r; return scale; };
    scale.ticks = function(count = 5) {
        const [d0, d1] = domain;
        const step = (d1 - d0) / count;
        const ticks = [];
        for (let i = 0; i <= count; i++) ticks.push(d0 + i * step);
        return ticks;
    };
    return scale;
}

function scaleBand() {
    let domain = [];
    let range = [0, 1];
    let padding = 0.1;
    function scale(d) {
        const idx = domain.indexOf(d);
        if (idx === -1) return undefined;
        const step = (range[1] - range[0]) / domain.length;
        return range[0] + idx * step + (step * padding / 2);
    }
    scale.domain = function(d) { if (!arguments.length) return domain; domain = d; return scale; };
    scale.range = function(r) { if (!arguments.length) return range; range = r; return scale; };
    scale.padding = function(p) { if (!arguments.length) return padding; padding = p; return scale; };
    scale.bandwidth = function() {
        if (domain.length === 0) return 0;
        const step = (range[1] - range[0]) / domain.length;
        return step * (1 - padding);
    };
    return scale;
}

function scaleOrdinal() {
    let domain = [];
    let range = ["#3b82f6", "#10b981", "#f59e0b", "#ef4444", "#8b5cf6", "#ec4899", "#06b6d4"];
    function scale(d) {
        const idx = domain.indexOf(d);
        if (idx >= 0) return range[idx % range.length];
        domain.push(d);
        return range[(domain.length - 1) % range.length];
    }
    scale.domain = function(d) { if (!arguments.length) return domain; domain = d; return scale; };
    scale.range = function(r) { if (!arguments.length) return range; range = r; return scale; };
    return scale;
}

function curveLinear(data, x, y) {
    let d = "";
    for (let i = 0; i < data.length; i++) {
        const px = x(data[i], i);
        const py = y(data[i], i);
        d += (i === 0 ? `M ${px} ${py}` : ` L ${px} ${py}`);
    }
    return d;
}

function curveMonotoneX(data, x, y) {
    if (data.length <= 2) return curveLinear(data, x, y);
    let pts = data.map((d, i) => [x(d, i), y(d, i)]);
    let d = `M ${pts[0][0]} ${pts[0][1]}`;
    for (let i = 0; i < pts.length - 1; i++) {
        let p0 = pts[Math.max(i - 1, 0)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[Math.min(i + 2, pts.length - 1)];
        let cp1x = p1[0] + (p2[0] - p0[0]) / 6;
        let cp1y = p1[1] + (p2[1] - p0[1]) / 6;
        let cp2x = p2[0] - (p3[0] - p1[0]) / 6;
        let cp2y = p2[1] - (p3[1] - p1[1]) / 6;
        d += ` C ${cp1x.toFixed(2)} ${cp1y.toFixed(2)}, ${cp2x.toFixed(2)} ${cp2y.toFixed(2)}, ${p2[0].toFixed(2)} ${p2[1].toFixed(2)}`;
    }
    return d;
}

function line() {
    let x = d => d[0];
    let y = d => d[1];
    let curve = curveLinear;
    function generate(data) {
        if (!data || data.length === 0) return "";
        return curve(data, x, y);
    }
    generate.x = function(fn) { if (!arguments.length) return x; x = typeof fn === 'function' ? fn : () => fn; return generate; };
    generate.y = function(fn) { if (!arguments.length) return y; y = typeof fn === 'function' ? fn : () => fn; return generate; };
    generate.curve = function(c) { if (!arguments.length) return curve; curve = c; return generate; };
    return generate;
}

function area() {
    let x = d => d[0];
    let y0 = () => 0;
    let y1 = d => d[1];
    let curve = curveLinear;
    function generate(data) {
        if (!data || data.length === 0) return "";
        let top = [];
        let bottom = [];
        for (let i = 0; i < data.length; i++) {
            top.push([x(data[i], i), y1(data[i], i)]);
            bottom.push([x(data[i], i), y0(data[i], i)]);
        }
        let d = `M ${top[0][0]} ${top[0][1]}`;
        for (let i = 1; i < top.length; i++) {
            d += ` L ${top[i][0]} ${top[i][1]}`;
        }
        for (let i = bottom.length - 1; i >= 0; i--) {
            d += ` L ${bottom[i][0]} ${bottom[i][1]}`;
        }
        return d + " Z";
    }
    generate.x = function(fn) { if (!arguments.length) return x; x = typeof fn === 'function' ? fn : () => fn; return generate; };
    generate.y0 = function(fn) { if (!arguments.length) return y0; y0 = typeof fn === 'function' ? fn : () => fn; return generate; };
    generate.y1 = function(fn) { if (!arguments.length) return y1; y1 = typeof fn === 'function' ? fn : () => fn; return generate; };
    generate.curve = function(c) { if (!arguments.length) return curve; curve = c; return generate; };
    return generate;
}

function pie() {
    let value = d => d;
    function generate(data) {
        const values = data.map((d, i) => value(d, i));
        const total = values.reduce((a, b) => a + b, 0);
        let startAngle = 0;
        return data.map((d, i) => {
            const v = values[i];
            const endAngle = total > 0 ? startAngle + (v / total) * 2 * Math.PI : startAngle;
            const slice = {
                data: d,
                value: v,
                startAngle,
                endAngle
            };
            startAngle = endAngle;
            return slice;
        });
    }
    generate.value = function(fn) { if (!arguments.length) return value; value = typeof fn === 'function' ? fn : () => fn; return generate; };
    return generate;
}

function arc() {
    let innerRadius = () => 0;
    let outerRadius = () => 100;
    function generate(d) {
        const r0 = typeof innerRadius === 'function' ? innerRadius(d) : innerRadius;
        const r1 = typeof outerRadius === 'function' ? outerRadius(d) : outerRadius;
        const a0 = d.startAngle - Math.PI / 2;
        const a1 = d.endAngle - Math.PI / 2;
        const largeArc = (a1 - a0) > Math.PI ? 1 : 0;
        const x01 = r1 * Math.cos(a0);
        const y01 = r1 * Math.sin(a0);
        const x02 = r1 * Math.cos(a1);
        const y02 = r1 * Math.sin(a1);
        if (r0 <= 0) {
            return `M 0 0 L ${x01.toFixed(2)} ${y01.toFixed(2)} A ${r1} ${r1} 0 ${largeArc} 1 ${x02.toFixed(2)} ${y02.toFixed(2)} Z`;
        } else {
            const x03 = r0 * Math.cos(a1);
            const y03 = r0 * Math.sin(a1);
            const x04 = r0 * Math.cos(a0);
            const y04 = r0 * Math.sin(a0);
            return `M ${x01.toFixed(2)} ${y01.toFixed(2)} A ${r1} ${r1} 0 ${largeArc} 1 ${x02.toFixed(2)} ${y02.toFixed(2)} L ${x03.toFixed(2)} ${y03.toFixed(2)} A ${r0} ${r0} 0 ${largeArc} 0 ${x04.toFixed(2)} ${y04.toFixed(2)} Z`;
        }
    }
    generate.innerRadius = function(r) { if (!arguments.length) return innerRadius; innerRadius = typeof r === 'function' ? r : () => r; return generate; };
    generate.outerRadius = function(r) { if (!arguments.length) return outerRadius; outerRadius = typeof r === 'function' ? r : () => r; return generate; };
    return generate;
}

class Selection {
    constructor(node) {
        this.node = node;
    }
    append(tag) {
        const child = document.createElement(tag);
        this.node.appendChild(child);
        return new Selection(child);
    }
    attr(name, value) {
        if (arguments.length === 1) return this.node.getAttribute(name);
        this.node.setAttribute(name, value);
        return this;
    }
    text(str) {
        if (arguments.length === 0) return this.node.textContent;
        this.node.textContent = String(str);
        return this;
    }
    html() {
        return this.node.outerHTML;
    }
}

function select(selectorOrEl) {
    const el = typeof selectorOrEl === 'string' ? document.createElement(selectorOrEl) : selectorOrEl;
    return new Selection(el);
}

const d3 = {
    scaleLinear,
    scaleBand,
    scaleOrdinal,
    line,
    area,
    pie,
    arc,
    curveLinear,
    curveMonotoneX,
    select,
};
globalThis.d3 = d3;
"##;

/// Sandboxed QuickJS execution environment for chart scripts.
pub struct QuickJsEngine {
    _runtime: Runtime,
    context: Context,
}

impl QuickJsEngine {
    /// Create a new QuickJS engine instance initialized with DOM shim and D3.
    pub fn new() -> Result<Self, ChartError> {
        let runtime = Runtime::new().map_err(|e| ChartError::JsError(e.to_string()))?;
        let context = Context::full(&runtime).map_err(|e| ChartError::JsError(e.to_string()))?;
        let engine = Self {
            _runtime: runtime,
            context,
        };
        engine.init_shims()?;
        Ok(engine)
    }

    fn init_shims(&self) -> Result<(), ChartError> {
        self.context.with(|ctx| {
            ctx.eval::<(), _>(DOM_SHIM_JS)
                .map_err(|e| ChartError::JsError(e.to_string()))?;
            ctx.eval::<(), _>(D3_CORE_JS)
                .map_err(|e| ChartError::JsError(e.to_string()))?;
            Ok(())
        })
    }

    /// Evaluate JavaScript code and return the result as a string.
    pub fn eval_string(&self, script: &str) -> Result<String, ChartError> {
        self.context.with(|ctx| {
            let res: rquickjs::Value = ctx
                .eval(script)
                .map_err(|e| ChartError::JsError(e.to_string()))?;
            if let Some(s) = res.as_string() {
                s.to_string()
                    .map_err(|e| ChartError::JsError(e.to_string()))
            } else {
                Ok(format!("{res:?}"))
            }
        })
    }

    /// Evaluate JavaScript code that produces an SVG string, then parse it into an [`SvgDocument`].
    pub fn eval_svg(&self, script: &str) -> Result<SvgDocument, ChartError> {
        let svg_str = self.eval_string(script)?;
        parse_svg(&svg_str).map_err(ChartError::SvgError)
    }

    /// Evaluate a D3 script with an injected `data` variable and return the rendered [`SvgDocument`].
    pub fn eval_d3<T: serde::Serialize>(
        &self,
        script: &str,
        data: &T,
    ) -> Result<SvgDocument, ChartError> {
        let data_json = serde_json::to_string(data)?;
        let wrapper = format!(
            r#"
            (() => {{
                const data = {data_json};
                {script}
            }})()
            "#
        );
        self.eval_svg(&wrapper)
    }
}

impl Default for QuickJsEngine {
    fn default() -> Self {
        Self::new().expect("Failed to initialize QuickJS engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_rasterizer::SceneNode;

    #[test]
    fn test_quickjs_d3_chart_generation() {
        let engine = QuickJsEngine::new().expect("engine init");

        let script = r##"
            const svg = d3.select("svg").attr("width", 500).attr("height", 300);
            const x = d3.scaleLinear().domain([0, 10]).range([20, 480]);
            const y = d3.scaleLinear().domain([0, 100]).range([280, 20]);
            
            const line = d3.line()
                .x(d => x(d.x))
                .y(d => y(d.y))
                .curve(d3.curveMonotoneX);

            const pathD = line(data);
            svg.append("path")
               .attr("d", pathD)
               .attr("stroke", "#3b82f6")
               .attr("stroke-width", 3)
               .attr("fill", "none");

            return svg.html();
        "##;

        let data = vec![
            serde_json::json!({ "x": 0, "y": 10 }),
            serde_json::json!({ "x": 5, "y": 80 }),
            serde_json::json!({ "x": 10, "y": 40 }),
        ];

        let doc = engine.eval_d3(script, &data).expect("eval_d3");
        assert_eq!(doc.width, 500.0);
        assert_eq!(doc.height, 300.0);
        assert_eq!(doc.scene.nodes.len(), 1);

        match &doc.scene.nodes[0] {
            SceneNode::Path {
                d,
                stroke,
                stroke_width,
                fill,
                ..
            } => {
                assert!(d.starts_with("M 20 254"));
                assert!(stroke.is_some());
                assert_eq!(*stroke_width, 3.0);
                assert!(fill.is_none());
            }
            _ => panic!("Expected Path node"),
        }
    }
}
