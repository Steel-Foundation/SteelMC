# Quickstart: validating the plugin API and ports

## Build

```sh
cargo build --release -p steel                      # server (enables steel-plugin-api/host)
cargo build --release --manifest-path plugins/Cargo.toml   # all four ports
```

Libraries land in `plugins/target/release/` (`libvault.so`, `libessentials.so`,
`libworldedit.so`, `libworldguard.so`).

## Install

Add to `config/config.toml` (order does not matter; dependencies are sorted):

```toml
[[plugins]]
path = "plugins/target/release/libvault.so"
[[plugins]]
path = "plugins/target/release/libessentials.so"
[[plugins]]
path = "plugins/target/release/libworldedit.so"
[[plugins]]
path = "plugins/target/release/libworldguard.so"
```

Start the server; the log shows `Initialized plugin vault|essentials|worldedit|worldguard` in
dependency order. Make yourself operator with `/op <name>` from the console.

## Automated checks

```sh
cargo test -p steel-core extension::events
cargo test -p steel-plugin-api --features host --lib
cargo test -p steel-plugin-api --features host --test native --test behavior --test registration_failure -- --include-ignored
cargo test --manifest-path plugins/Cargo.toml
cargo build --manifest-path plugins/Cargo.toml       # debug libraries for the next test
cargo test -p steel-plugin-api --features host --test ports -- --include-ignored
```

`tests/ports.rs` loads all four ports (configured out of dependency order) into a real server
with a test peer and drives `/sethome`, `/home`, `/back`, `/eco`, `//pos1`, `//pos2`, `//set`,
`//undo`, `/rg define`, `/rg removeowner`, `/rg flag` and creative block breaks, then checks
the saved Essentials and WorldGuard files.

## Verification record (2026-10-31)

| Check | Result |
|---|---|
| `cargo test -p steel-core` | 2854 passed, 0 failed |
| `cargo test -p steel-plugin-api --features host -- --include-ignored` (lib + behavior, native, registration_failure, spleef, ports) | 27 passed |
| `cargo test -p steel-plugin-api --doc` | 12 passed, 1 ignored |
| `cargo test --manifest-path plugins/Cargo.toml` | 30 passed |
| `cargo clippy` steel-core, steel-plugin-api (host and plugin), steel, steel-login, plugins, examples | clean except pre-existing `absolute_paths`/`trivially_copy_pass_by_ref` output of `#[host_api]` expansion |
| `cargo fmt --check` (workspace and plugins) | clean |
| Release server start with the four ports, SIGINT stop | dependency order worldedit → worldguard, vault → essentials; Essentials saved on shutdown |
| Live client walkthroughs in each port README | pending (maintainer) |

## Live scenarios (summary; full scripts in each port README)

1. Vault: `/vault-info` lists `essentials` as economy and `vault` as permission/chat.
2. Essentials: `/sethome a`, walk away, `/home a`; `/back`; `/pay <friend> 10`; `/baltop`;
   chat shows the configured format; restart → home and balance persist.
3. WorldEdit: `//wand`, left/right click two corners, `//set stone`, `//undo`, `//redo`,
   `//copy`, move, `//paste`.
4. WorldGuard: select, `/rg define spawn`, `/rg flag spawn build deny`, non-op cannot break
   blocks there; `/rg flag spawn greeting Welcome`; walking in shows it.
