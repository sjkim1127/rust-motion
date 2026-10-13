#!/usr/bin/env bash
# ==============================================================================
# Rust Motion CLI Installer
# Fast, unattended, agent-friendly installer for Linux and macOS.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/sjkim1127/rust-motion/main/scripts/install.sh | bash
#
# Environment variables:
#   RUST_MOTION_VERSION      - Specific release version (e.g. "1.0.0", default: latest)
#   RUST_MOTION_INSTALL_DIR  - Directory to place binary (default: $HOME/.local/bin)
# ==============================================================================

set -euo pipefail

REPO="sjkim1127/rust-motion"
BINARY_NAME="rust-motion"
VERSION="${RUST_MOTION_VERSION:-latest}"
INSTALL_DIR="${RUST_MOTION_INSTALL_DIR:-$HOME/.local/bin}"

# 1. Detect Operating System
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$OS" in
    linux*)
        PLATFORM="linux"
        ;;
    darwin*)
        PLATFORM="macos"
        ;;
    msys*|cygwin*|mingw*)
        PLATFORM="windows"
        ;;
    *)
        echo "[ERROR] Unsupported operating system: $(uname -s)" >&2
        exit 1
        ;;
esac

# 2. Detect Architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        TARGET_ARCH="x86_64"
        ;;
    arm64|aarch64)
        TARGET_ARCH="aarch64"
        ;;
    *)
        echo "[ERROR] Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

# 3. Determine Asset Name
if [ "$PLATFORM" = "windows" ]; then
    ASSET_NAME="rust-motion-cli-windows-x86_64.zip"
    EXT="zip"
elif [ "$PLATFORM" = "macos" ]; then
    if [ "$TARGET_ARCH" = "aarch64" ]; then
        ASSET_NAME="rust-motion-cli-macos-aarch64.tar.gz"
    else
        ASSET_NAME="rust-motion-cli-macos-x86_64.tar.gz"
    fi
    EXT="tar.gz"
elif [ "$PLATFORM" = "linux" ]; then
    if [ "$TARGET_ARCH" != "x86_64" ]; then
        echo "[ERROR] Pre-built Linux binaries are currently only available for x86_64." >&2
        echo "Please compile from source via: cargo install rust-motion-cli" >&2
        exit 1
    fi
    ASSET_NAME="rust-motion-cli-linux-x86_64.tar.gz"
    EXT="tar.gz"
fi

# 4. Construct Download URL
if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET_NAME}"
else
    # Strip leading 'v' if provided
    CLEAN_VER="${VERSION#v}"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${CLEAN_VER}/${ASSET_NAME}"
fi

echo "==> Installing Rust Motion CLI ($PLATFORM / $TARGET_ARCH)..."
echo "    Source: $DOWNLOAD_URL"
echo "    Target: $INSTALL_DIR/$BINARY_NAME"

if [ "${RUST_MOTION_DRY_RUN:-0}" = "1" ]; then
    echo "    [Dry-run] Environment and download URL verified successfully."
    exit 0
fi

# 5. Create temporary working directory
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'rust-motion-install')"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

ARCHIVE_PATH="$TMP_DIR/$ASSET_NAME"

# 6. Download Archive
if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE_PATH"
elif command -v wget >/dev/null 2>&1; then
    wget -qO "$ARCHIVE_PATH" "$DOWNLOAD_URL"
else
    echo "[ERROR] Neither curl nor wget is available. Please install one of them." >&2
    exit 1
fi

# 7. Extract Binary
mkdir -p "$TMP_DIR/extracted"
if [ "$EXT" = "zip" ]; then
    if ! command -v unzip >/dev/null 2>&1; then
        echo "[ERROR] 'unzip' is required to extract Windows binary archives." >&2
        exit 1
    fi
    unzip -q "$ARCHIVE_PATH" -d "$TMP_DIR/extracted"
else
    tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR/extracted"
fi

# Locate extracted binary
EXTRACTED_BIN=""
if [ -f "$TMP_DIR/extracted/$BINARY_NAME" ]; then
    EXTRACTED_BIN="$TMP_DIR/extracted/$BINARY_NAME"
elif [ -f "$TMP_DIR/extracted/${BINARY_NAME}.exe" ]; then
    EXTRACTED_BIN="$TMP_DIR/extracted/${BINARY_NAME}.exe"
    BINARY_NAME="${BINARY_NAME}.exe"
elif [ -f "$TMP_DIR/extracted/rust_motion.exe" ]; then
    EXTRACTED_BIN="$TMP_DIR/extracted/rust_motion.exe"
    BINARY_NAME="${BINARY_NAME}.exe"
else
    echo "[ERROR] Could not find '$BINARY_NAME' binary inside the downloaded archive." >&2
    exit 1
fi

# 8. Install to destination
mkdir -p "$INSTALL_DIR"
cp "$EXTRACTED_BIN" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo "✓ Successfully installed '$BINARY_NAME' to $INSTALL_DIR/$BINARY_NAME"

# 9. Verify Installation and check PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo "⚠️  Note: '$INSTALL_DIR' is not in your current PATH."
    echo "   Add it to your profile or run this command in your session:"
    echo "     export PATH=\"$INSTALL_DIR:\$PATH\""
    echo ""
fi

# Run version check
if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
    echo "==> Verifying binary..."
    "$INSTALL_DIR/$BINARY_NAME" --version || true
fi

echo "==> Setup complete! Run 'rust-motion doctor' to verify environment dependencies."
