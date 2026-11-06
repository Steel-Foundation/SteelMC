# Shared helpers for scripts in tools/. Source this file; do not execute it.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="$REPO_ROOT/Cargo.toml"

# Prints [workspace.package].version from Cargo.toml, e.g. "0.15.2+mc26.2".
# Parsed rather than read with a TOML library because Nix's builtins.fromTOML
# cannot handle this file, and generated pins have to agree with what the flake sees.
cargo_workspace_version() {
    local version
    version="$(
        awk '
            /^\[workspace\.package\]/ { in_section = 1; next }
            /^\[/                     { in_section = 0 }
            in_section && /^version[[:space:]]*=/ { print; exit }
        ' "$CARGO_TOML" | sed -E 's/.*"(.*)".*/\1/'
    )"

    if [[ -z "$version" ]]; then
        echo "error: could not read [workspace.package].version from $CARGO_TOML" >&2
        return 1
    fi

    if [[ "$version" != *"+mc"* ]]; then
        echo "error: version '$version' has no '+mc' suffix" >&2
        return 1
    fi

    echo "$version"
}

# Prints the targeted Minecraft version (the "+mc" suffix), e.g. "26.2".
target_minecraft_version() {
    local version
    version="$(cargo_workspace_version)" || return 1
    echo "${version##*+mc}"
}
