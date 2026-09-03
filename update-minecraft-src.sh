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

# Use Gradle's default 4G heap (avoid bumping to 8G)

# Run GitCraft
cd "$TEMP_DIR/GitCraft"
echo "Running GitCraft..."
GITCRAFT_ARGS=(
    "--override-repo-target=$MINECRAFT_SRC_DIR"
    "--only-unobfuscated"
    "--mappings=identity_unmapped"
    "--min-version=1.21.11"
    "--refresh-min-version=1.21.11_unobfuscated"
    "--only-stable"
)
./gradlew run --args="${GITCRAFT_ARGS[*]}"

echo "Done! minecraft-src has been updated."
