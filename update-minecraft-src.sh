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

# Pin fabric-loom to avoid TinyJavadocProvider API breakage in newer versions
sed -i.bak 's/loom_version = 1\.+/loom_version = 1.15.5/' "$TEMP_DIR/GitCraft/gradle.properties"

# Run GitCraft
cd "$TEMP_DIR/GitCraft"
echo "Running GitCraft..."
JAVA_TOOL_OPTIONS="-Xmx8G" ./gradlew run --args="--override-repo-target=$MINECRAFT_SRC_DIR --only-unobfuscated --mappings=identity_unmapped --min-version=1.21.11 --only-stable"

echo "Done! minecraft-src has been updated."
