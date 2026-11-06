#!/usr/bin/env bash
#
# Regenerates minecraft-src/ with GitCraft, decompiling the Minecraft version
# targeted by the "+mc" suffix in Cargo.toml.
#
# By default only the targeted version is decompiled. Pass --min-version to also
# include every stable version from that one up to the target, e.g. to diff
# vanilla changes across an update.
#
# Usage: tools/update-minecraft-src.sh [--min-version <version>]
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

usage() {
    echo "Usage: $0 [--min-version <version>]" >&2
    echo "  --min-version <version>  Oldest version to decompile (default: the Cargo.toml target)" >&2
}

max_version="$(target_minecraft_version)"
min_version="$max_version"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --min-version)
            [[ $# -ge 2 ]] || { usage; exit 1; }
            min_version="$2"
            shift 2
            ;;
        --min-version=*)
            min_version="${1#*=}"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument '$1'" >&2
            usage
            exit 1
            ;;
    esac
done

MINECRAFT_SRC_DIR="$REPO_ROOT/minecraft-src"

# Temp directory on the same filesystem to avoid cross-device link errors.
TEMP_DIR="$REPO_ROOT/.gitcraft-tmp"
rm -rf "$TEMP_DIR"
mkdir -p "$TEMP_DIR"
trap 'rm -rf "$TEMP_DIR"' EXIT

echo "Decompiling Minecraft $min_version..$max_version into $MINECRAFT_SRC_DIR"

git clone https://github.com/WinPlay02/GitCraft "$TEMP_DIR/GitCraft"

# Increase heap from default 4G to 8G
sed -i.bak "s/-Xmx4G/-Xmx8G/" "$TEMP_DIR/GitCraft/build.gradle" && rm -f "$TEMP_DIR/GitCraft/build.gradle.bak"

# GitCraft downloads every extra version's metadata before applying version
# filters, and some combat test zips are only hosted on archive.org, which can
# fail and make GitCraft retry forever. Skip the combat test chain entirely; no
# other version lists a combat test as its parent.
METADATA_PROVIDER="$TEMP_DIR/GitCraft/src/main/groovy/com/github/winplay02/gitcraft/manifest/BaseMetadataProvider.java"
METADATA_SOURCE_ADD='this.metadataSources.add(MetadataSources.RemoteMetadata.of(manifestEntry));'
if ! grep -qF "$METADATA_SOURCE_ADD" "$METADATA_PROVIDER"; then
    echo "error: GitCraft changed; cannot skip combat test metadata in $METADATA_PROVIDER" >&2
    exit 1
fi
sed -i.bak "s/$METADATA_SOURCE_ADD/if (!manifestEntry.id().contains(\"_combat-\")) { $METADATA_SOURCE_ADD }/" "$METADATA_PROVIDER" && rm -f "$METADATA_PROVIDER.bak"

cd "$TEMP_DIR/GitCraft"
# Refreshing from the min version resets the repo's main branch, so
# minecraft-src only contains the requested range.
GITCRAFT_ARGS=(
    "--override-repo-target=$MINECRAFT_SRC_DIR"
    "--only-unobfuscated"
    "--only-stable"
    "--mappings=identity_unmapped"
    "--min-version=$min_version"
    "--max-version=$max_version"
    "--refresh-min-version=$min_version"
)
./gradlew run --args="${GITCRAFT_ARGS[*]}"

echo "Done! minecraft-src has been updated to Minecraft $max_version."
