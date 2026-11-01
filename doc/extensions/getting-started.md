# Create an extension

This guide builds a native Rust extension that replies to `/hello ping` with a
message from server configuration. Commands below run from the Steel checkout.

## Prepare the checkout

Use the Rust toolchain selected by the repository's `rust-toolchain.toml`, and
initialize its pinned Stabby dependency:

```sh
git submodule update --init --recursive
```

Use `steel-extension-api` from the same checkout as the server. Extensions build
as separate `cdylib` crates with the API's default features. The `host` feature is
for the server; enabling it in an extension pulls in server implementation code
and bypasses the intended extension-side bindings.

## Create the crate

Create this directory structure:

```text
extension-example/hello-extension/
├── Cargo.toml
├── extension.toml
└── src/
    └── lib.rs
```

Put this in `extension-example/hello-extension/Cargo.toml`:

```toml
[package]
name = "hello-extension"
version = "0.1.0"
edition = "2024"

# Keep extension and host builds separate, including Cargo feature resolution.
[workspace]

[lib]
crate-type = ["cdylib"]

[dependencies]
steel-extension-api = { path = "../../steel-extension-api" }
serde = { version = "1", features = ["derive"] }
toml = "1"
```

The dependency path assumes the directory layout above. Adjust it if you keep
the extension outside this checkout.

## Register a command and read configuration

Put this complete implementation in `extension-example/hello-extension/src/lib.rs`:

```rust
use std::sync::OnceLock;

use serde::Deserialize;
use steel_extension_api::{ApiResult, Command, Commands, ExtensionContext, Hooks, stabby};
use stabby::{slice::Slice, str::Str, string::String as ApiString};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    message: String,
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

#[stabby::export(callback)]
fn greet(command: Command<'_>, _action: Str<'_>) -> ApiResult<i32> {
    let Some(settings) = SETTINGS.get() else {
        return Err(ApiString::from("hello extension is not initialized")).into();
    };
    command.reply(settings.message.as_str().into());
    Ok(1).into()
}

#[stabby::export(callback)]
fn register_commands(mut commands: Commands<'_>) -> ApiResult<()> {
    let actions = [Str::new("ping")];
    commands.register_player_command(
        "hello:hello".into(),
        Slice::from(actions.as_slice()),
        greet,
    )
}

fn pre_init(mut hooks: Hooks<'_>) -> ApiResult<()> {
    hooks.register_commands(register_commands)
}

fn init(context: ExtensionContext<'_>) -> ApiResult<()> {
    let settings = match toml::from_str::<Settings>(context.config().as_str()) {
        Ok(settings) => settings,
        Err(error) => return Err(ApiString::from(error.to_string())).into(),
    };
    SETTINGS
        .set(settings)
        .map_err(|_| ApiString::from("hello extension was initialized twice"))
        .into()
}

steel_extension_api::export_extension!("hello", pre_init, init);
```

`pre_init` subscribes to command registration. Steel later passes the actual
mutable command registry to `register_commands`, freezes the dispatcher, and
calls `init` before creating worlds. `init` receives an `ExtensionContext` with
this extension's name, version, configuration and private data directory, and
parses only its own configuration. The command uses the initialized settings
when a player runs it.

`#[stabby::export(callback)]` checks boundary types and supplies the callback's
calling convention. `export_extension!` binds the host API and exports the
`steel_extension_v3` descriptor. You do not need to write a loader or `unsafe`
code for this extension. Both entrypoints may also return any
`Result<(), impl Display>`, so `?` works directly.

This example uses the low-level command binding to show the boundary. Most
extensions use the closure helpers instead (`hooks.on_commands`, typed
`command::CommandBuilder` trees, `hooks.on_*` for gameplay events); see the
[API crate README](../../steel-extension-api/README.md#hello-world). The long form of
the macro also declares a version and dependencies:

```rust
steel_extension_api::export_extension! {
    name: "hello",
    version: "0.1.0",
    depends: [],              // loaded and initialized first; missing ones skip this extension
    soft_depends: ["vault"],  // initialized first when present
    pre_init: pre_init,
    init: init,
}
```

## Describe the package

Put this in `extension-example/hello-extension/extension.toml`:

```toml
[package]
name = "hello"
version = "0.1.0"

[targets]
linux-x86_64 = "libhello_extension.so"
linux-aarch64 = "libhello_extension.so"
macos-x86_64 = "libhello_extension.dylib"
macos-aarch64 = "libhello_extension.dylib"
windows-x86_64 = "hello_extension.dll"
windows-aarch64 = "hello_extension.dll"
```

The package name `hello` must match `export_extension!("hello", ...)`. The Cargo
crate name determines the default native library name and output ZIP filename;
it can differ from the descriptor name. Target keys use Rust operating system
and architecture names.

## Build, package, and install

With Python 3.11 or newer, run:

```sh
python3 package-extension.py extension-example/hello-extension --debug
```

This builds the extension in a separate Cargo invocation and writes
`extensions/hello-extension.zip`. The archive contains `extension.toml` and the
library for the target actually built. Omit `--debug` for a release build. Use
`--target <rust-target-triple>` for a configured cross-compilation toolchain, or
`--output <path.zip>` to choose the archive path.

Add this at the top level of `config/config.toml`, outside `[server]`:

```toml
[[extensions]]
path = "extensions/hello-extension.zip"

[extensions.config]
message = "Hello from a Steel extension!"
```

All paths are relative to the server's working directory. Explicit entries come
first in configuration order. Steel then discovers the remaining ZIP packages and
native libraries in `./extensions` in filename order, skipping files already
selected by path, and initializes everything in dependency order. An extension
without an `[extensions.config]` table reads `extensions/<name>/config.toml`,
created empty on first start; this example requires a `message`, so either add
the table above or write `message = "..."` into `extensions/hello/config.toml`.

Start Steel normally, connect as a player, and run `/hello ping`. The namespaced
form `/hello:hello ping` is also available. This command permits players by
default while respecting an explicit deny of `hello.command.hello`; it does not
accept console sources.

For development, you can select a raw library instead of packaging it:

```sh
cargo build --manifest-path extension-example/hello-extension/Cargo.toml
```

On Linux, change the configured path to
`extension-example/hello-extension/target/debug/libhello_extension.so`. On macOS, use
`libhello_extension.dylib`; on Windows, use `hello_extension.dll`. Keep the same
configuration section. You can also copy the library itself into `./extensions`.
Move an older ZIP out of `./extensions` when selecting a raw library: two files
declaring `hello` are both skipped with a warning.

## Rebuild and troubleshoot

Restart the server after rebuilding an extension or changing its configuration.
Libraries stay loaded for the process lifetime; unloading and hot reload are
unsupported. Native extensions run with the server's privileges and must be
trusted. ABI checks detect incompatible boundary signatures, not malicious code.

| Failure | Check |
| --- | --- |
| Skipped: not a Steel extension | Build a `cdylib` and include `export_extension!`. |
| Skipped: built for an older Steel extension API | Rebuild against the server's API checkout. |
| Incompatible host signature | Rebuild against the server's API checkout and pinned Stabby revision, with `host` disabled. |
| Manifest name differs from descriptor | Use `hello` in both the package manifest and `export_extension!`. |
| Skipped: another file also declares the extension | Remove extra packages or libraries with the same descriptor name, including ZIPs discovered alongside a raw library. |
| Skipped: requires missing extension | Install the extension named in the message, or remove it from `depends`. |
| Unsupported target or missing library | Package for the server's operating system and architecture; include the matching target entry. |
| Configuration parsing failed | Add `[extensions.config]` with `message`; match the extension's own schema. |
| Command unavailable | Connect as a player, check command permissions, and check startup logs for registration collisions. |

Continue with the [API reference](api.md), the
[wool shearing](../../extension-example/wool-shearing/README.md) for block behaviors,
[Game of Life](../../extension-example/game-of-life/README.md) for world ticks,
[strange terrain](../../extension-example/strange-terrain/README.md) for generation,
or [Spleef](../../extension-example/spleef/README.md) for temporary
worlds, commands, and bots.
