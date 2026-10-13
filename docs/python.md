# Python Integration Guide

Rust Motion provides official Python bindings powered by PyO3, allowing Python applications and data pipelines to render programmatic video with native Rust performance.

---

## 1. Installation

```bash
pip install dioxuscut
```

### Supported Environments:
- **Python**: 3.10, 3.11, 3.12, 3.13
- **Platforms**: Linux (x86_64, aarch64), macOS (Apple Silicon, Intel), Windows (x86_64)

---

## 2. Basic Video Rendering

```python
import dioxuscut

# Render a registered composition
dioxuscut.render(
    composition="HelloWorld",
    output="hello_from_python.mp4",
    props={"title": "Generated via Python"},
    width=1920,
    height=1080,
    fps=30.0,
    duration=90,
)
print("Video successfully rendered!")
```

---

## 3. Rendering Rhai Scripts with Dynamic Data

```python
import dioxuscut

# Pass complex nested JSON/dict props to a Rhai template
props = {
    "title": "Quarterly Performance",
    "metrics": [
        {"label": "Revenue", "value": "$1.2M", "change": "+14%"},
        {"label": "Users", "value": "45,200", "change": "+28%"},
    ],
    "theme": {"primary": "#3B82F6", "background": "#0F172A"}
}

dioxuscut.render(
    script="charts.rhai",
    output="report.mp4",
    props=props,
    codec="h264",
    crf=18,
)
```

---

## 4. Integration with NumPy & PyTorch

For computer vision or ML training datasets, you can render individual frames directly into raw buffers:

```python
import numpy as np
import dioxuscut

# Render frame 15 as an RGBA byte buffer
raw_bytes = dioxuscut.render_frame_bytes(
    composition="HelloWorld",
    frame=15,
    width=640,
    height=360,
)

# Convert to NumPy array
frame_array = np.frombuffer(raw_bytes, dtype=np.uint8).reshape((360, 640, 4))
print(f"Frame shape: {frame_array.shape}")
```

---

## 5. Version Synchronization

The Python package versions remain strictly synchronized with the Rust workspace releases. Rust Motion 1.0.0 corresponds directly to `dioxuscut == 1.0.0` on PyPI.
