#!/bin/bash
# Build and package bridle for release
# Usage: ./scripts/release.sh v0.1.0

set -e

VERSION="${1:?Usage: $0 vX.Y.Z}"
RELEASE_DIR="release/${VERSION}"

mkdir -p "$RELEASE_DIR"

# macOS ARM (Apple Silicon)
echo "Building for aarch64-apple-darwin..."
cargo build --release
cp target/release/bridle "$RELEASE_DIR/bridle-${VERSION}-aarch64-apple-darwin"

# macOS x86_64 (Intel)
echo "Building for x86_64-apple-darwin..."
cargo build --release --target x86_64-apple-darwin 2>/dev/null || \
  echo "Skipping x86_64 (cross-compilation not set up)"

# Linux x86_64
echo "Building for x86_64-unknown-linux-musl..."
cross build --release --target x86_64-unknown-linux-musl 2>/dev/null || \
  cargo build --release --target x86_64-unknown-linux-gnu 2>/dev/null || \
  echo "Skipping Linux (cross-compilation not set up)"

# Create tarballs
cd "$RELEASE_DIR"
for bin in bridle-*; do
  if [ -f "$bin" ]; then
    tar -czf "${bin}.tar.gz" "$bin"
    sha256sum "${bin}.tar.gz" | awk '{print $1}' > "${bin}.tar.gz.sha256"
  fi
done

echo ""
echo "✅ Release artifacts in $RELEASE_DIR:"
ls -la
echo ""
echo "Next steps:"
echo "1. Create a GitHub release at https://github.com/kristianlentino/bridle/releases/new?tag=${VERSION}"
echo "2. Upload the .tar.gz files"
echo "3. Create homebrew-tap repo with the formula below"
