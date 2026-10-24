#!/usr/bin/env bash
#
# Regenerates minecraft-src/ for the Minecraft version targeted in Cargo.toml.
#
# Downloads the vanilla server jar from Mojang, decompiles it with Vineflower into
# minecraft-src/minecraft/src, and runs the vanilla data generator for the reports
# in minecraft-src/minecraft/resources/datagen-reports.
#
# Run this after bumping the "+mc" suffix in Cargo.toml.
set -euo pipefail

VINEFLOWER_VERSION="1.12.0"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_TOML="$SCRIPT_DIR/Cargo.toml"
MINECRAFT_SRC_DIR="$SCRIPT_DIR/minecraft-src"
MANIFEST_URL="https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
VINEFLOWER_URL="https://repo1.maven.org/maven2/org/vineflower/vineflower/$VINEFLOWER_VERSION/vineflower-$VINEFLOWER_VERSION.jar"

for tool in curl jq java unzip sha1sum; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: '$tool' is required but not installed" >&2
        exit 1
    fi
done

# [workspace.package].version, e.g. "0.15.2+mc26.2". Same parsing as
# update-minecraft-assets.sh.
cargo_version="$(
    awk '
        /^\[workspace\.package\]/ { in_section = 1; next }
        /^\[/                     { in_section = 0 }
        in_section && /^version[[:space:]]*=/ { print; exit }
    ' "$CARGO_TOML" | sed -E 's/.*"(.*)".*/\1/'
)"

if [[ "$cargo_version" != *"+mc"* ]]; then
    echo "error: could not read a '+mc' version from [workspace.package].version in $CARGO_TOML" >&2
    exit 1
fi

minecraft_version="${cargo_version##*+mc}"
echo "Targeting Minecraft $minecraft_version (from $cargo_version)"

# Temp directory on the same filesystem so the final move is a cheap rename.
TEMP_DIR="$SCRIPT_DIR/.minecraft-src-tmp"
rm -rf "$TEMP_DIR"
mkdir -p "$TEMP_DIR"
trap 'rm -rf "$TEMP_DIR"' EXIT

version_url="$(
    curl -fsSL "$MANIFEST_URL" \
        | jq -r --arg id "$minecraft_version" '.versions[] | select(.id == $id) | .url'
)"
if [[ -z "$version_url" ]]; then
    echo "error: Minecraft $minecraft_version not found in the version manifest" >&2
    exit 1
fi

server_json="$(curl -fsSL "$version_url" | jq -c '.downloads.server')"
server_url="$(jq -r '.url // empty' <<<"$server_json")"
server_sha1="$(jq -r '.sha1 // empty' <<<"$server_json")"
if [[ -z "$server_url" || -z "$server_sha1" ]]; then
    echo "error: no server download listed for Minecraft $minecraft_version" >&2
    exit 1
fi

echo "Downloading server jar..."
curl -fsSL -o "$TEMP_DIR/server.jar" "$server_url"
if [[ "$(sha1sum "$TEMP_DIR/server.jar" | cut -d' ' -f1)" != "$server_sha1" ]]; then
    echo "error: server jar checksum mismatch" >&2
    exit 1
fi

echo "Downloading Vineflower $VINEFLOWER_VERSION..."
curl -fsSL -o "$TEMP_DIR/vineflower.jar" "$VINEFLOWER_URL"

# The server jar is a bundler: the game classes live in a nested jar listed in
# META-INF/versions.list (tab separated: hash, id, path).
inner_path="$(unzip -p "$TEMP_DIR/server.jar" META-INF/versions.list | cut -f3 | head -n1)"
if [[ -z "$inner_path" ]]; then
    echo "error: server jar has no META-INF/versions.list entry" >&2
    exit 1
fi
unzip -q -j -d "$TEMP_DIR/inner" "$TEMP_DIR/server.jar" "META-INF/versions/$inner_path"
inner_jar="$TEMP_DIR/inner/$(basename "$inner_path")"

echo "Decompiling $(basename "$inner_jar")..."
mkdir -p "$TEMP_DIR/out/minecraft/src"
java -Xmx8G -jar "$TEMP_DIR/vineflower.jar" -dgs=1 -log=WARN "$inner_jar" "$TEMP_DIR/out/minecraft/src"

echo "Running vanilla data generator reports..."
# The bundler unpacks libraries and writes logs into the working directory.
mkdir -p "$TEMP_DIR/run"
(cd "$TEMP_DIR/run" && java -DbundlerMainClass=net.minecraft.data.Main -jar "$TEMP_DIR/server.jar" \
    --reports --output "$TEMP_DIR/datagen")
mkdir -p "$TEMP_DIR/out/minecraft/resources"
mv "$TEMP_DIR/datagen/reports" "$TEMP_DIR/out/minecraft/resources/datagen-reports"

rm -rf "$MINECRAFT_SRC_DIR"
mv "$TEMP_DIR/out" "$MINECRAFT_SRC_DIR"

echo "Done! minecraft-src has been updated to Minecraft $minecraft_version."
