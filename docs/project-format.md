# Rust Motion Project Format Specification (v1)

## 1. Overview

Rust Motion project files (`.dioxuscut.json`) provide a declarative, versioned representation of multi-track timelines, compositions, settings, asset references, and temporal events.

- Schema URI: `https://dioxuscut.dev/schema/project/v1.json`
- Local Schema File: `schemas/dioxuscut-project-v1.schema.json`

---

## 2. Compatibility Guarantee

Throughout the `1.x` release series, Rust Motion guarantees:

1. **Forward & Backward Compatibility**: Project files adhering to the v1 specification will remain loadable, editable, and renderable by all future 1.x releases without manual migration.
2. **Deterministic Round-Tripping**: Loading a project file, editing timeline structures in Studio or via the Rust/Python APIs, and saving will preserve all properties, unknown/extension fields, and asset paths without semantic distortion.
3. **Relative Path Portability**: Local asset paths are stored relative to the project directory, allowing project folders to be moved, archived, and collaborated on across systems.

---

## 3. Schema Structure

A valid `v1` project object consists of:

```json
{
  "$schema": "https://dioxuscut.dev/schema/project/v1.json",
  "version": 1,
  "composition": "HelloWorld",
  "settings": {
    "width": 1920,
    "height": 1080,
    "fps": 30.0,
    "duration": 150,
    "scale": 1.0,
    "crf": 18,
    "preset": "fast",
    "backend": "native"
  },
  "props": {},
  "assets": [],
  "tracks": [],
  "events": [],
  "voice_over_asset_id": null
}
```

### 3.1 Top-Level Properties

| Field | Type | Required | Description |
|---|---|---|---|
| `version` | `integer` | Yes | Must be `1` for the v1 schema. |
| `composition` | `string` | Yes | Identifier of the root composition or template. |
| `settings` | `object` | Yes | Global render and timeline settings. |
| `props` | `object` | No | Arbitrary JSON data passed to the composition during evaluation. |
| `assets` | `array` | No | List of declared media asset entries. |
| `tracks` | `array` | No | List of timeline tracks containing clips. |
| `events` | `array` | No | List of named temporal markers (`TimeEvent`). |
| `voice_over_asset_id` | `string` / `null` | No | Optional asset identifier designated as the primary voice-over audio track. |

### 3.2 Assets (`assets[]`)

Each asset references an external resource:
- `id` (`string`, required): Unique identifier within the project.
- `path` (`string`, required): File path (relative or absolute) or `https://` remote URL.
- `kind` (`enum`, optional): `"image"`, `"video"`, `"audio"`, `"font"`, `"lottie"`, `"model"`, `"shader"`, or `"other"`.
- `sha256` (`string`, optional): Expected SHA-256 hash for integrity verification.
- `duration_in_frames` (`integer`, optional): Pre-computed media duration.
- `fps` (`number`, optional): Native frame rate for video assets.
- `width` / `height` (`integer`, optional): Pixel dimensions for visual media.

### 3.3 Tracks & Clips (`tracks[].clips[]`)

Timeline tracks organize sequential or layered media:
- `id` (`string`, required): Unique track identifier.
- `name` (`string`, optional): Display name in timeline UI.
- `muted` / `hidden` / `locked` (`boolean`, optional): Track visibility and edit locks.
- `clips` (`array`, required):
  - `id` (`string`, required): Clip identifier.
  - `asset_id` (`string`, optional): Referenced asset ID.
  - `start` (`integer`, required): Start frame on the project timeline.
  - `duration` (`integer`, required): Length of the clip in frames.
  - `offset` (`integer`, optional): Trim/in-point offset into source media.
  - `speed` (`number`, optional): Playback rate multiplier (default: `1.0`).
  - `volume` (`number`, optional): Base audio volume multiplier (default: `1.0`).
  - `props` (`object`, optional): Clip-specific props overrides.

### 3.4 Named Time Events (`events[]`)

Time events define frame markers and range spans:
- `id` (`string`, required): Marker ID.
- `name` (`string`, required): Semantic label (e.g., `"intro_end"`, `"chorus"`).
- `frame` (`integer`, required): Frame timestamp.
- `duration` (`integer`, optional): Span duration for range events.
- `payload` (`object`, optional): Custom event metadata.
