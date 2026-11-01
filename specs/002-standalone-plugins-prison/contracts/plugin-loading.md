# Contract: Plugin loading v4

- Server scans `plugins/` for `*.so`/`*.dll`/`*.dylib`, plus `[[plugins]] path = ...` entries.
- Symbol `steel_plugin_v4` returns a descriptor with `api_version`; older symbols are reported as
  "built for an older Steel plugin API, rebuild it".
- Config: `plugins/<name>/config.toml` (created empty) unless `[plugins.config]` is given.
- Skips (warning, startup continues): unopenable file, missing symbol, ABI mismatch, duplicate
  name, missing hard dependency (transitively). Fatal: pre-init/init error, dependency cycle.
- Commands: `/plugins` (`steel.command.plugins`, default access) lists `name vX (api Y)`;
  `/version <plugin>` shows version, API version, dependencies and file.
- Template `plugin-template/`: `Cargo.toml` with git dependency, `src/lib.rs` hello plugin,
  README with build/install steps and the optional local `[patch]`.
