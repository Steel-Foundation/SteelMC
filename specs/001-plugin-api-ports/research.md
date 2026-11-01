# Research: Plugin API Foundations and Reference Plugin Ports

All findings are from the current tree (`another-plugin-experiment`, Steel `0.16.0+mc26.3`).

## R1. How events reach gameplay code

- **Decision**: Add a typed, ABI-independent event bus in `steel-core/src/extension/events/`
  stored in `ExtensionHooks` (already shared as `Arc<Extensions>` by `Server`). Each event kind
  has its own `EventBus<E>` holding `Box<dyn EventHandler<E>>` entries sorted by
  `EventPriority`. Gameplay code builds a plain Rust event struct, calls
  `server.events().fire(&mut event)`, and honors the result.
- **Rationale**: Matches the existing `WorldTickHandler`/`ServerTickHandler` pattern
  (`steel-core/src/extension/mod.rs`); core stays independent of stabby; native embedders and
  tests can subscribe directly; empty buses cost one length check (FR-005).
- **Alternatives**: (a) per-callsite `Option<fn>` globals — no ordering/cancellation model;
  (b) dyn `Any` event map — forbidden by AGENTS.md (no `TypeId` in plugin-extensible foundations).

## R2. Fire points (verified call sites)

| Event | Call site | Cancel effect |
|---|---|---|
| pre-login | `Server::try_reserve_player_join` path in `server/player_admission.rs` | disconnect with reason |
| join | `Server::broadcast_player_join_message` (`server/broadcasting.rs:84`) | message replace/suppress |
| quit | `Server::broadcast_player_leave_message` (`server/broadcasting.rs:105`) | message replace/suppress |
| chat | `Player::handle_chat` (`player/chat/mod.rs:240`) | message dropped; rendered override sent as system chat |
| command preprocess | player chat-command handler (`player/chat` / command queue entry) | command not executed |
| block break | `BlockBreakingManager::destroy_block` (`game_mode/block_breaking.rs:342`), after `block_action_restricted` | returns false → existing resync |
| block place | `BlockItem::place_with_policy` before `place_block` (`behavior/items/block_item.rs`) | `InteractionResult::Fail`; caller already sends block updates + inventory |
| interact (right, block) | `handle_use_item_on` after `may_interact` (`game_mode/block_interaction.rs:77`) | `send_block_updates` + inventory resync |
| interact (left, block) | `handle_player_action` `StartDestroyBlock` | send `CBlockUpdate`, ack sequence |
| interact (right, air) | `handle_use_item` | inventory resync |
| damage | `LivingEntity::hurt_server` (`entity/living_entity.rs:669`) start | return false (no damage) |
| death | `Player::die` (`player/mod.rs`) before death message broadcast | message replace/suppress (not cancellable — vanilla death is final) |
| move | `Player::handle_move_player` after accepting a position whose block position changed | teleport back to `from` |
| teleport | `Player::teleport` and `change_entity_world` for players | no movement |
| shutdown | `steel/src/lib.rs` stop path before world save | n/a |

- **Threading**: gameplay packets run on packet workers between ticks
  (`server/packet_processor.rs`), possibly in parallel for different players; damage/death also
  fire on world tick threads. Handlers therefore must be `Send + Sync`, and the API documents that
  handlers may run concurrently on different threads but never during another handler for the
  same event instance.

## R3. ABI shape for events, commands and closures

- **Decision**: Every event is an opaque borrowed handle (`#[opaque] pub struct ChatEvent<'a>(&'a
  mut CoreChatEvent)`) declared in a `#[stabby::host_api]` module with getters/setters. The
  native callback signature is `extern "C" fn(Event<'_>, u64)` where the `u64` is a plugin-chosen
  token. The plugin-side half of `steel-plugin-api` (compiled into each plugin, so statics are
  per plugin) stores Rust closures in a table and passes their index as the token, giving plugin
  authors `hooks.on_chat(Priority::Normal, |event| { ... })`.
- **Rationale**: Reuses the macro's checked bindings, `!Send` invariant lifetimes (handles can't
  escape), and keeps closures out of the ABI. One trampoline per event type.
- **Alternatives**: stabby `dyn` trait objects per handler (heavier, every event would need a
  trait); passing plain structs by value (can't mutate host-owned `TextComponent`s or entities).

## R4. Commands

- **Decision**: Plugins describe a command tree as a flat stabby `Vec<CommandNodeSpec>` (parent
  index, literal/argument, argument type + bounds, optional permission suffix, executes flag,
  optional suggestion flag). The host builds the real `steel_core::command::CommandNode` tree
  and registers it with `CommandRegistration` (`default_access` or op-only). Execution calls one
  trampoline with `(CommandInvocation<'_>, token)`; the plugin-side builder maps tokens to
  closures. Typed reads (`integer`, `word`, `players`, `block_pos`, `block_state`, `world`,
  `item`) go through `CommandContext` accessors. Missing core argument types (`block_pos`,
  `block_state`, `item`) are exposed by adding constructors to `steel-core/src/command/api.rs`
  backed by the existing `execution/coordinates.rs`, `execution/block.rs`, `execution/item.rs`.
- **Rationale**: The core command API already supports typed arguments, suggestions, aliases and
  derived permissions (`<namespace>.command.<path>[.<sub>]`); only the ABI bridge is missing.
- **Alternatives**: string-only argument parsing in plugins — loses client-side completion and
  argument validation, against vanilla UX.

## R5. Permissions

- **Decision**: Expose `Server::permission(player, node) -> PermissionValue {Allow, Deny,
  Unset}`, `permission_metadata(player, key) -> Option<MetadataValue>`, `permission_groups(player)`
  and `group_names()`, backed by `Player::permission_state`, `permission_metadata`,
  `permission_groups` and `PermissionGroupManager::group_names`. Plugins apply their own default
  for `Unset` (Bukkit's "default: op/true/false"). Command permissions keep Steel's derived keys.
- **Rationale**: Steel's permission model is tri-state with metadata (prefix, suffix, integer
  limits); Vault's Permission/Chat contracts map onto it without a second system.
- **Group mutation** (Vault `playerAddGroup`) is out of scope for this feature: the provider
  reports it unsupported. // TODO recorded in Vault port.

## R6. Cross-plugin services

- **Decision**: A host-side process-lifetime `ServiceRegistry` stores entries
  `{key, provider plugin, priority, report: &'static TypeReport, object: *const ()}`. The
  plugin-side generic helpers `services::register::<T: IStable + Send + Sync>(key, priority,
  value)` leak a `Box<T>` and pass its report; `services::get::<T>(key) -> Option<&'static T>`
  checks `T::REPORT.is_compatible(entry.report)` before casting. Contracts are ordinary stabby
  traits in small API crates (`vault-api`, `worldedit-api`) wrapped in
  `stabby::dynptr!(Box<dyn Trait + Send + Sync>)`.
- **Rationale**: Same safety argument as `stabby::host::Export` (report + pinned library);
  providers are never unloaded; contracts can be versioned by key (`vault:economy/v1`).
- **Alternatives**: routing through host-defined fixed traits (host would need to know every
  plugin contract — violates modding-grade extensibility).

## R7. Lifecycle v3 (dependencies, data directory, shutdown)

- **Decision**: Replace the `steel_plugin_v2` descriptor with `steel_plugin_v3` adding `version`,
  `depends` and `soft_depends` (stabby slices of `Str<'static>`). The loader reads all
  descriptors first, topologically sorts by dependencies (errors on missing hard dependency or
  cycle), then runs pre-init and init in that order. `init` receives `PluginContext<'_>` with
  `name()`, `config()` and `data_directory()` (`plugins/<name>/`, created on demand). Shutdown
  callbacks subscribed in pre-init run in reverse order from `SteelServer` before worlds save.
- **Rationale**: Dependency order must be known before any pre-init; a context handle avoids
  host-side caller identification. Constitution I requires a new key for semantic change.
  The repository is in early development, so v2 plugins (examples, fixture) are updated in place.

## R8. Text

- **Decision**: Styled text crosses the boundary as JSON (vanilla text component format),
  parsed on the host with `text_components` serde support (feature `serde` is enabled
  workspace-wide). The plugin-side `text` module provides a small builder plus a legacy
  `&`-code converter (`&a`, `&l`, `&#rrggbb`) producing that JSON.
- **Rationale**: JSON is the vanilla wire format, already validated by the host crate; no new
  ABI structure per style attribute.

## R9. World edits

- **Decision**: Add `World::set_block_with_flags`, `World::block_name` / `block_state_from_str`
  (via `execution/block.rs::parse_block_input`) and `World::height_bounds`. WorldEdit applies
  edits synchronously on the caller's thread (commands run where vanilla `/fill` runs), checks
  that every chunk is loaded before writing (atomic edit), and records `(pos, old, new)` for
  undo. A configurable limit (default 1,000,000 blocks) bounds work per command.
  Block entity contents are not captured by undo — // TODO.
- **Rationale**: Mirrors vanilla `/fill`, which writes synchronously; avoids async chunk loading
  (out of scope).

## R10. Persistence for ports

- **Decision**: TOML files under each plugin's data directory, using `serde` + `toml` (already
  used by the spleef example). Saved on change via a dirty flag flushed by the plugin scheduler
  every 100 ticks and on shutdown, writing to `*.tmp` then renaming for atomicity.
- **Rationale**: Human-editable like the Bukkit YAML files; atomic rename prevents corruption
  (SC-006). Files are small (per-player), so synchronous writes off the hot path are acceptable.
  // TODO: move writes to a background writer if profiling shows tick impact.
