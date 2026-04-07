#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MINECRAFT_SRC_DIR="$SCRIPT_DIR/minecraft-src"

# Create temp directory on same filesystem to avoid cross-device link errors
TEMP_DIR="$SCRIPT_DIR/.gitcraft-tmp"
rm -rf "$TEMP_DIR"
mkdir -p "$TEMP_DIR"
echo "Cloning GitCraft into $TEMP_DIR..."

# Cleanup on exit
trap "rm -rf $TEMP_DIR" EXIT

# Clone GitCraft
git clone https://github.com/WinPlay02/GitCraft "$TEMP_DIR/GitCraft"

# Increase heap from default 4G to 8G
sed -i.bak "s/-Xmx4G/-Xmx8G/" "$TEMP_DIR/GitCraft/build.gradle" && rm -f "$TEMP_DIR/GitCraft/build.gradle.bak"

# Run GitCraft
cd "$TEMP_DIR/GitCraft"
echo "Running GitCraft..."
./gradlew run --args="--override-repo-target=$MINECRAFT_SRC_DIR --only-unobfuscated --mappings=identity_unmapped --min-version=1.21.11 --only-stable"

echo "Done! minecraft-src has been updated."
