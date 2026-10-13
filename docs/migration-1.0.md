# Rust Motion 1.0 Migration Guide

This document assists developers migrating from pre-1.0 versions (0.1.x, 0.2.x, 0.3.x) to Rust Motion 1.0.0.

---

## 1. Overview & 1.0 Stability Promise

Rust Motion 1.0 marks the transition from rapid experimentation to a stable, long-term supported programmatic video rendering platform. Under SemVer 1.0:
- **No breaking changes** will be introduced within the 1.x line for stable public APIs.
- The project format `.dioxuscut.json` (schema v1) is guaranteed to remain readable across all 1.x releases.
- The CLI command structure and exit codes are frozen.

---

## 2. Breaking Changes & Updates

### 2.1 CLI Exit Codes & Machine-Readable Output

- **Pre-1.0**: The CLI exited with code `1` on all errors, outputting freeform log text.
- **1.0**: Standardized, predictable exit codes are now guaranteed:
  - `0`: Success (`EXIT_SUCCESS`)
  - `1`: General runtime error (`EXIT_GENERAL_ERROR`)
  - `2`: Invalid flags/arguments (`EXIT_INVALID_ARGUMENTS`)
  - `3`: Missing dependency like FFmpeg (`EXIT_MISSING_DEPENDENCY`)
  - `4`: Preflight schema/script/asset validation failure (`EXIT_VALIDATION_ERROR`)
  - `5`: Timeout or cancellation (`EXIT_TIMEOUT_OR_CANCELLED`)

**Action**: If your CI/CD or agent scripts checked exit codes or scraped stdout, switch to `--json` or `DIOXUSCUT_JSON=1` to consume structured JSON.

---

### 2.2 New Preflight Commands: `validate`, `doctor`, `inspect`

- Instead of attempting a full render to verify a template, use `dioxuscut validate`:
  ```bash
  dioxuscut validate --script video.rhai --props props.json
  ```
- Instead of manual environment checks, use `dioxuscut doctor`:
  ```bash
  dioxuscut doctor
  ```
- Use `dioxuscut inspect` for discovering backends and codecs:
  ```bash
  dioxuscut inspect --backends
  dioxuscut inspect --codecs
  ```

---

### 2.3 Project Format (`.dioxuscut.json`) Schema v1

- **Pre-1.0**: Schema version was loosely typed.
- **1.0**: `version` is strictly an integer (`u32`), e.g. `"version": 1`. Unknown fields are rejected to prevent subtle typos.
- Existing project files should ensure `"version": 1`.

---

### 2.4 Rendering Backend Capability Model

- The **`native`** (CPU) renderer is the authoritative Tier 1 reference implementation for 1.0.
- The **`gpu`** (WGPU) backend is designated Tier 2 (Experimental).
- The **`browser`** backend is designated Tier 2 (Supported with fallback).
- See [Backend Capabilities Matrix](backends.md) for full details.

---

### 2.5 Migrating from Remotion

If you have existing Remotion TypeScript projects, you can use the built-in migration scaffolding:

```bash
# Transpile Remotion TypeScript component to Rhai script
dioxuscut migrate -i RemotionComp.tsx -t rhai -o comp.rhai

# Transpile Remotion TypeScript component to Rust template
dioxuscut migrate -i RemotionComp.tsx -t rust -o comp.rs
```

The migration tool automatically converts:
- `useCurrentFrame()` and `useVideoConfig()` hooks
- `interpolate()` calls and easing curves
- `spring()` animations
- `<Sequence>` and `<Loop>` elements
