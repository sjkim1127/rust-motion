#!/usr/bin/env bash
set -euo pipefail

# Rust Motion Release Verification Script
# Verifies version consistency across Cargo, PyProject, Tauri, and Changelog.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "=== Verifying Rust Motion Release Consistency ==="

# 1. Extract workspace version from root Cargo.toml
WORKSPACE_VERSION=$(grep -m 1 '^version *= *' Cargo.toml | head -n 1 | sed -E 's/version *= *"([^"]+)".*/\1/')
echo "Detected Workspace Version: $WORKSPACE_VERSION"

if [ -z "$WORKSPACE_VERSION" ]; then
    echo "ERROR: Could not parse workspace version from Cargo.toml"
    exit 1
fi

# 2. Verify all internal crates in Cargo.toml match this version
MISMATCHES=0
while IFS= read -r line; do
    CRATE_NAME=$(echo "$line" | sed -E 's/^(rust-motion-[a-z0-9_-]+) *=.*/\1/')
    CRATE_VER=$(echo "$line" | sed -E 's/.*version *= *"([^"]+)".*/\1/')
    if [ "$CRATE_VER" != "$WORKSPACE_VERSION" ]; then
        echo "ERROR: Internal crate $CRATE_NAME has version '$CRATE_VER', expected '$WORKSPACE_VERSION'"
        MISMATCHES=$((MISMATCHES + 1))
    fi
done < <(grep -E '^rust-motion-[a-z0-9_-]+ *= *\{.*version *= *"' Cargo.toml)

if [ "$MISMATCHES" -gt 0 ]; then
    echo "ERROR: Found $MISMATCHES internal crate version mismatch(es) in root Cargo.toml"
    exit 1
fi
echo "✓ All internal crate definitions in Cargo.toml match $WORKSPACE_VERSION"

# 3. Verify Python pyproject.toml version
PYPROJECT_PATH="crates/python/pyproject.toml"
if [ -f "$PYPROJECT_PATH" ]; then
    PY_VERSION=$(grep -m 1 '^version *= *' "$PYPROJECT_PATH" | sed -E 's/version *= *"([^"]+)".*/\1/')
    EXPECTED_PY_VERSION="${WORKSPACE_VERSION//-rc./rc}"
    if [ "$PY_VERSION" != "$WORKSPACE_VERSION" ] && [ "$PY_VERSION" != "$EXPECTED_PY_VERSION" ]; then
        echo "ERROR: $PYPROJECT_PATH has version '$PY_VERSION', expected '$WORKSPACE_VERSION' or '$EXPECTED_PY_VERSION'"
        exit 1
    fi
    echo "✓ Python pyproject.toml matches $WORKSPACE_VERSION"
else
    echo "WARN: $PYPROJECT_PATH not found"
fi

# 4. Verify Tauri config version
TAURI_CONF_PATH="apps/studio-tauri/src-tauri/tauri.conf.json"
if [ -f "$TAURI_CONF_PATH" ]; then
    TAURI_VERSION=$(grep -m 1 '"version":' "$TAURI_CONF_PATH" | sed -E 's/.*"version": *"([^"]+)".*/\1/')
    if [ "$TAURI_VERSION" != "$WORKSPACE_VERSION" ]; then
        echo "ERROR: $TAURI_CONF_PATH has version '$TAURI_VERSION', expected '$WORKSPACE_VERSION'"
        exit 1
    fi
    echo "✓ Tauri tauri.conf.json matches $WORKSPACE_VERSION"
else
    echo "WARN: $TAURI_CONF_PATH not found"
fi

# 5. Verify CHANGELOG.md has an entry for this version
if grep -qE "## \[?$WORKSPACE_VERSION\]?" CHANGELOG.md; then
    echo "✓ CHANGELOG.md contains release entry for $WORKSPACE_VERSION"
else
    echo "WARN: CHANGELOG.md missing heading for $WORKSPACE_VERSION"
fi

# 6. Verify Cargo metadata and compilation
echo "Checking workspace compilation..."
cargo check --workspace --quiet
echo "✓ Workspace cargo check succeeded"

echo "=== Release verification PASSED for v$WORKSPACE_VERSION ==="
exit 0
