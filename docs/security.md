# Security Policy and Advisory Analysis

This document describes the security model, untrusted input boundaries, and dependency vulnerability reachability analysis for Rust Motion 1.0.

---

## 1. Threat Model & Untrusted Input Boundaries

Rust Motion is designed to be executed headlessly in untrusted environments (e.g. user-submitted scripts, AI coding agents, SaaS rendering backends). To prevent host compromise, several layers of isolation are enforced:

### 1.1 Script Execution Sandboxing (Rhai Engine)
- Rhai scripts evaluated via the CLI are executed inside a sandboxed Rhai engine instance.
- The engine does not expose arbitrary OS syscalls, process spawning (`std::process::Command`), or unrestricted raw network sockets.
- Script evaluation has strict recursion depth and expression limits to prevent stack overflow or denial-of-service via infinite loops.

### 1.2 Filesystem Sandboxing & Path Traversal Guards
- When rendering media assets (images, audio, video, fonts, SVGs), paths are checked against allowed sandbox roots (`--sandbox-root` / `--media-root`).
- By default, the sandbox root is restricted to the script or project directory.
- Relative paths attempting path traversal (`../`) or symlink escaping outside allowed roots are rejected with exit code `4` (`EXIT_VALIDATION_ERROR`).
- Permissive mode (`--permissive`) disables filesystem isolation and is strictly opt-in for trusted local development.

### 1.3 Remote Asset Fetching
- Remote assets (HTTP/HTTPS URLs) are fetched via `reqwest` with explicit timeouts.
- Content lengths are bounded to prevent memory exhaustion / zip bombs.
- Downloaded assets are hashed (SHA-256) and cached locally in a dedicated, isolated cache directory.

### 1.4 Versioned Project JSON Schema Validation
- Project JSON schemas (`.rust_motion.json`) use strict serde deserialization (`deny_unknown_fields`).
- Frame dimensions (`width`, `height`), `fps`, and `duration` are bounds-checked before rendering starts. Zero or negative values fail preflight validation.

---

## 2. Dependency Vulnerability & Reachability Analysis

### 2.1 Fixed Advisories
- **RUSTSEC-2026-0285 (`rustls`)**:
  - *Status*: **Resolved** in Rust Motion 0.3.0 / 1.0.0.
  - *Resolution*: Upgraded `rustls` to `0.23.45` across all workspace targets.

### 2.2 Accepted Exceptions

None. `.cargo/audit.toml` ignores no advisories.

### 2.3 Open Warnings

`cargo audit` fails on known vulnerabilities; unmaintained, unsound, and yanked crates only warn. As of 2026-11-26 (cargo-audit 0.22.2, 567 locked dependencies) it reports no vulnerabilities and the following warnings. None of them has had a reachability analysis.

| Crate | Warning | Advisory | Pulled in by (direct path) |
|---|---|---|---|
| `rustix 1.1.4` | unsound, patched in 1.1.6 | RUSTSEC-2026-0486 | `tempfile`, among others |
| `tokio 1.53.1` | unsound, patched in 1.53.3 | RUSTSEC-2026-0386 | `rust-motion-cli`, `rust-motion-renderer` (via `axum` and others) |
| `paste 1.0.15` | unmaintained | RUSTSEC-2024-0436 | `wgpu` (`metal`), `image` (`exr`, `ravif`) |
| `rustybuzz 0.20.1` | unmaintained | RUSTSEC-2026-0206 | `rust-motion-rasterizer` |
| `ttf-parser 0.25.1` | unmaintained | RUSTSEC-2026-0192 | `rustybuzz`, `ab_glyph` |
| `smartstring 1.0.1` | unmaintained | RUSTSEC-2026-0249 | `rhai` |
| `wasmi 2.0.0` | yanked | none | `rust-motion-plugins` |
| `wasmi_core 2.0.0` | yanked | none | `wasmi` |

The list changes as advisories are published; run `cargo audit` for the current result.

---

## 3. Reporting a Vulnerability

If you discover a security vulnerability in Rust Motion:
1. Please do **NOT** open a public GitHub issue.
2. Report the vulnerability privately via GitHub Security Advisories or email the maintainers directly.
3. We will acknowledge receipt within 48 hours and work with you on a coordinated disclosure timeline.
