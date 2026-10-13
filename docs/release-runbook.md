# Rust Motion Release Runbook

This document defines the official, idempotent release procedures for Rust Motion releases.

---

## 1. Prerequisites

Before initiating a release, ensure you have:
- Write access to the repository on GitHub (`sjkim1127/rust-motion`).
- Crates.io publishing credentials configured via `cargo login`.
- PyPI publishing credentials configured via `twine` / `maturin`.
- Required tools installed:
  - Rust 1.80+ (`rustc`, `cargo`)
  - `cargo-audit`
  - Python 3.10+ and `maturin >= 1.4`
  - `ffmpeg` on `$PATH`

---

## 2. Preflight Checklist

Run all automated checks locally before touching any version files:

```bash
# 1. Verify working directory is completely clean
git status --porcelain

# 2. Check code formatting
cargo fmt --check

# 3. Check for any compiler / clippy warnings across all workspace targets
cargo clippy --workspace --all-targets -- -D warnings

# 4. Run the full workspace test suite
cargo test --workspace

# 5. Verify dependency security advisories
cargo audit

# 6. Run the release verification script
bash scripts/verify-release.sh
```

All 6 checks must succeed with exit code `0`.

---

## 3. Version Bump Procedure

To bump to version `X.Y.Z`:

1. Update workspace version in root `Cargo.toml`:
   ```toml
   [workspace.package]
   version = "X.Y.Z"
   ```
2. Update internal dependency versions in root `Cargo.toml`:
   ```toml
   dioxuscut-animation = { version = "X.Y.Z", path = "crates/animation" }
   # ... update all internal crates
   ```
3. Update Python version in `crates/python/pyproject.toml`:
   ```toml
   version = "X.Y.Z"
   ```
4. Update Studio Tauri version in `apps/studio-tauri/src-tauri/tauri.conf.json`:
   ```json
   "version": "X.Y.Z"
   ```
5. Update `CHANGELOG.md` with release notes, highlights, and migration guide links under `## [X.Y.Z] - YYYY-MM-DD`.
6. Regenerate lockfile:
   ```bash
   cargo check --workspace
   ```
7. Verify consistency:
   ```bash
   bash scripts/verify-release.sh
   ```

---

## 4. Crates.io Topological Publish Order

Because internal crates depend on one another, publishing to crates.io must follow topological dependency order:

```bash
# Core primitives
cargo publish -p dioxuscut-paths
cargo publish -p dioxuscut-animation
cargo publish -p dioxuscut-core

# Low-level rasterizer & media
cargo publish -p dioxuscut-rasterizer
cargo publish -p dioxuscut-shapes
cargo publish -p dioxuscut-charts
cargo publish -p dioxuscut-media

# High-level composition & effects
cargo publish -p dioxuscut-composition
cargo publish -p dioxuscut-transitions
cargo publish -p dioxuscut-noise
cargo publish -p dioxuscut-captions
cargo publish -p dioxuscut-project

# Engine & integrations
cargo publish -p dioxuscut-renderer
cargo publish -p dioxuscut-player
cargo publish -p dioxuscut-vdom
cargo publish -p dioxuscut-cli
```

> **Note**: Allow 10–30 seconds between publishes for crates.io index propagation.

---

## 5. Python Wheel Publishing

Build and upload the Python package to PyPI:

```bash
cd crates/python
maturin build --release --out dist
twine upload dist/*
```

---

## 6. Git Tag & GitHub Release

```bash
git add .
git commit -m "chore(release): vX.Y.Z"
git tag -a vX.Y.Z -m "Rust Motion vX.Y.Z release"
git push origin main --tags
```

Create a GitHub Release on `sjkim1127/rust-motion` matching tag `vX.Y.Z` with notes from `CHANGELOG.md`.

---

## 7. Recovery & Interruption Handling

- **Publish interrupted mid-sequence**:
  Because published crates on crates.io are immutable, do not attempt to re-publish already uploaded versions. Simply resume from the first crate that failed.
- **Tagging error before push**:
  Delete the local tag: `git tag -d vX.Y.Z`.
- **Preflight check failure**:
  Fix the underlying issue, commit, and restart from Step 2.
