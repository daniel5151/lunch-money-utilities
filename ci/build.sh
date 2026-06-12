#!/usr/bin/env bash
set -euo pipefail

# Ensure script runs from the repository root
cd "$(dirname "$0")/.."

echo "Building release binary for x86_64-unknown-linux-musl with split debug symbols..."
CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=packed \
CARGO_PROFILE_RELEASE_DEBUG=true \
cargo build --release --target x86_64-unknown-linux-musl

echo "Preparing release assets in target directory..."
TARGET_DIR="target/x86_64-unknown-linux-musl/release"
BINARY_NAME="splitwise-lunchmoney-x86_64-unknown-linux-musl"

cp "$TARGET_DIR/splitwise-lunchmoney" "$TARGET_DIR/$BINARY_NAME"
cp "$TARGET_DIR/splitwise-lunchmoney.dwp" "$TARGET_DIR/${BINARY_NAME}.dwp"

echo "Build complete. Created assets in $TARGET_DIR:"
echo "  - $TARGET_DIR/$BINARY_NAME"
echo "  - $TARGET_DIR/${BINARY_NAME}.dwp"
