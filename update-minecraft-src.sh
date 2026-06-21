#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MINECRAFT_SRC_DIR="$SCRIPT_DIR/minecraft-src"
VERSION_MANIFEST_URL="https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
LATEST_VER="$(curl -fsSL "$VERSION_MANIFEST_URL" \
    | tr -d '[:space:]' \
    | grep -o '"id":"[^"]*","type":"release"' \
    | sed -n '2s/^"id":"\([^"]*\)","type":"release"$/\1/p')"

if [ -z "$LATEST_VER" ]; then
    echo "Failed to fetch second latest Minecraft release from $VERSION_MANIFEST_URL" >&2
    exit 1
fi

echo "Using $LATEST_VER as minimum Minecraft release"

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
GITCRAFT_ARGS=(
    "--override-repo-target=$MINECRAFT_SRC_DIR"
    "--only-unobfuscated"
    "--mappings=identity_unmapped"
    "--min-version=$LATEST_VER"
    "--only-stable"
)
./gradlew run --args="${GITCRAFT_ARGS[*]}"

echo "Done! minecraft-src has been updated."
