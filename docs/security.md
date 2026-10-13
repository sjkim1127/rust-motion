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

### 2.2 Reachability-Analyzed Advisories

#### RUSTSEC-2026-0257: `webbrowser 0.8.15` (Argument Injection)
- **Origin**: Transitive dependency pulled in by `dioxus-desktop 0.6.3` (which pins `webbrowser = "^0.8"`).
- **Reachability Assessment**: **UNREACHABLE in headless & server paths.**
  - `webbrowser` is only used by desktop GUI frameworks to launch the user's default browser when clicking external hyperlinks inside the Studio UI.
  - The core rasterizer (`rust-motion-rasterizer`), CLI binary (`rust-motion`), server pipeline (`rust-motion-cli serve`), and Python package (`rust-motion`) do not call `webbrowser::open`.
  - In automated agent and server environments, Studio GUI is not spawned.
- **Remediation Plan**: Upstream Dioxus is tracking the upgrade to `webbrowser >= 1.2.2`. Tracked in `.cargo/audit.toml`.

---

## 3. Reporting a Vulnerability

If you discover a security vulnerability in Rust Motion:
1. Please do **NOT** open a public GitHub issue.
2. Report the vulnerability privately via GitHub Security Advisories or email the maintainers directly.
3. We will acknowledge receipt within 48 hours and work with you on a coordinated disclosure timeline.
