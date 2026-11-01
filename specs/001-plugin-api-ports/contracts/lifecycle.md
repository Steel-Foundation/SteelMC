# Contract: Plugin Lifecycle v3

```rust
steel_plugin_api::export_plugin! {
    name: "worldguard",
    version: "0.1.0",
    depends: ["worldedit"],
    soft_depends: [],
    pre_init: pre_init,
    init: init,
}
fn pre_init(hooks: &mut Hooks<'_>) -> Result<(), String>;
fn init(context: &PluginContext<'_>) -> Result<(), String>;
```

- Bootstrap symbol: `steel_plugin_v3`. v2 libraries are rejected with a rebuild message.
- Order: load all descriptors → validate unique names → topo sort (hard + present soft deps) →
  pre-init each → registry events → init each → runtime → shutdown callbacks in reverse order.
- `PluginContext`: `name()`, `version()`, `config() -> String (TOML)`,
  `data_directory() -> String` (absolute, created before init).
- `hooks.on_shutdown(|server| ...)`: runs once on clean stop before worlds are saved.
- `log::{info,warn,error}` helpers route through the host logger with `[plugin]` prefix.

## Server config

```toml
[[plugins]]
path = "plugins/libworldguard.so"
[plugins.config]
max_region_count = 100
```

Data directory: `plugins/<name>/` relative to the server working directory.
