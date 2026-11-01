---
description: "Task list for Plugin API Foundations and Reference Plugin Ports"
---

# Tasks: Plugin API Foundations and Reference Plugin Ports

**Input**: Design documents from `specs/001-plugin-api-ports/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Required by constitution principle VII for advanced systems (event ordering, service
report checks, dependency sort, command tree build, undo, region resolution, persistence).

**Organization**: Grouped by user story. Lifecycle v3 (part of US7) is pulled into the
Foundational phase because every port needs data directories and dependency ordering.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Create ports workspace `plugins/Cargo.toml` (members vault-api, vault, essentials, worldedit-api, worldedit, worldguard; `[workspace.dependencies]` steel-plugin-api path, serde, toml) and add `plugins/target` to `.gitignore`
- [X] T002 [P] Add `steel-plugin-api/src/callbacks.rs`: plugin-side closure tables (`Registry<F>` storing `Box<F>` indexed by `u64` token) used by events, commands, scheduler; not compiled with `host`

## Phase 2: Foundational (blocking)

### Lifecycle v3

- [X] T003 Replace `PluginDescriptor` with v3 in `steel-plugin-api/src/entry.rs`: fields name, version, depends, soft_depends (`Slice<'static, Str<'static>>`), pre_init, init; keys `steel:plugin/pre_init/v3`, `steel:plugin/init/v3`; init signature `extern "C" fn(PluginContext<'_>) -> ApiResult<()>`
- [X] T004 Rewrite `export_plugin!` in `steel-plugin-api/src/lib.rs` to the keyword form in contracts/lifecycle.md, exporting `steel_plugin_v3`, with closure-friendly `pre_init(&mut Hooks)`/`init(&PluginContext)` returning `Result<(), String>`
- [X] T005 Add `PluginContext` opaque handle (name, version, config, data_directory) and `log(plugin, level, message)` binding in `steel-plugin-api/src/bindings.rs`
- [X] T006 Split loading in `steel-plugin-api/src/host/mod.rs` and `host/lifecycle.rs`: `load_all(paths)` reads descriptors, validates unique names, topologically sorts (missing hard dependency / cycle → error), then pre-inits in order; `initialize_all` creates `plugins/<name>/` and passes `PluginContext`; `shutdown_all` runs shutdown callbacks in reverse order
- [X] T007 Add dependency-sort unit tests (order, soft missing ok, hard missing error, cycle error) in `steel-plugin-api/src/host/lifecycle.rs`
- [X] T008 Update `steel/src/lib.rs` to use `load_all`, keep the plugin list, and call shutdown before world save on stop; update `steel/src/config/mod.rs` docs
- [X] T009 Migrate `examples/exotic-plugin/src/lib.rs`, `examples/spleef-plugin/src/lib.rs`, `steel-plugin-api/tests/fixture/src/lib.rs` and README snippets to v3

### Core event bus

- [X] T010 Create `steel-core/src/extension/events/mod.rs`: `EventPriority`, `EventHandler<E>` trait, `EventBus<E>` (sorted insert, `fire(&mut E)` honoring `Cancellable::is_cancelled` and ignore_cancelled, monitor flag), `Events` struct with one bus per event, accessor `Server::events()`
- [X] T011 [P] Define player events in `steel-core/src/extension/events/player.rs` (PreLogin, Join, Quit, Chat, Command, Move, Teleport, Death) per data-model.md
- [X] T012 [P] Define block/interact events in `steel-core/src/extension/events/block.rs` and damage in `steel-core/src/extension/events/entity.rs`
- [X] T013 Event bus tests (priority order, cancellation visibility, ignore_cancelled skip, monitor) in `steel-core/src/extension/events/tests.rs`

**Checkpoint**: plugins load with v3 and dependency order; core can fire events.

## Phase 3: User Story 1 – react to and veto gameplay (P1) 🎯 MVP

**Goal**: Plugins subscribe to and cancel/modify gameplay events.
**Independent test**: fixture plugin cancels stone breaking and prefixes chat (spec US1).

- [X] T014 [US1] Fire Join/Quit in `steel-core/src/server/broadcasting.rs` (message override/suppress)
- [X] T015 [US1] Fire PreLogin in `steel-core/src/server/player_admission.rs` (deny → disconnect with reason)
- [X] T016 [US1] Fire Chat in `steel-core/src/player/chat/mod.rs::handle_chat` (cancel; message edit; rendered override broadcast as system chat)
- [X] T017 [US1] Fire Command preprocess where player chat commands enter the command queue (`steel-core/src/player/` command packet handler)
- [X] T018 [US1] Fire BlockBreak in `steel-core/src/player/game_mode/block_breaking.rs::destroy_block`
- [X] T019 [US1] Fire BlockPlace in `steel-core/src/behavior/items/block_item.rs::place_with_policy` before `place_block`
- [X] T020 [US1] Fire Interact (left block, right block, right air) in `steel-core/src/player/game_mode/block_interaction.rs` and the use-item handler, resyncing on cancel
- [X] T021 [US1] Fire Damage in `steel-core/src/entity/living_entity.rs::hurt_server` and Death in `steel-core/src/player/mod.rs::die`
- [X] T022 [US1] Fire Move in `steel-core/src/player/movement/mod.rs::handle_move_player` (block-position change; cancel → teleport back) and Teleport in `Player::teleport` / `change_entity_world` for players
- [X] T023 [US1] Plugin event handles + `Hooks::subscribe_*` bindings in `steel-plugin-api/src/events/` (host_api modules) and host `EventHandler` adapters in `steel-plugin-api/src/host/events.rs`
- [X] T024 [US1] Plugin-side ergonomic `Hooks::on_<event>(priority, closure)` wrappers in `steel-plugin-api/src/events/handlers.rs` using callbacks.rs
- [X] T025 [US1] Player/world access bindings in `steel-plugin-api/src/player.rs` (lookup by name/uuid, name, display name get/set, world, position, rotation, health/food get/set, game mode, allow-flight/flying, invulnerable, teleport across worlds, kick, send message/action bar/title JSON, broadcast, permission/metadata/groups, give item with name and count, clear/held item)
- [X] T026 [P] [US1] World additions in `steel-plugin-api/src/bindings.rs` (`set_block_with_flags`, `block_state_from_str`, `block_state_to_string`, `block_key`, `min_y`/`max_y`, `has_block_entity`)
- [X] T027 [P] [US1] Plugin-side `text` module (JSON builder, legacy `&` codes incl. `&#rrggbb`) with tests in `steel-plugin-api/src/text.rs`
- [X] T028 [US1] Integration test covering cancelled breaks and real commands — implemented as `steel-plugin-api/tests/ports.rs` (all four ports in one server)

## Phase 4: User Story 2 – rich commands (P1)

**Independent test**: `/greet <player> [message...]` with completion and permissions.

- [X] T029 [US2] Add `block_pos`, `block_state`, `item` argument constructors and accessors to `steel-core/src/command/api.rs`
- [X] T030 [US2] `CommandNodeSpec`/`ArgumentSpec` stabby types + `Commands::register_tree` + `CommandInvocation` accessors in `steel-plugin-api/src/command/`
- [X] T031 [US2] Host tree builder with validation (root first, parents before children, unique sibling names) in `steel-plugin-api/src/host/commands.rs` + unit tests
- [X] T032 [US2] Plugin-side `CommandBuilder` (closures, aliases, default_access, sub-permissions, suggestions) in `steel-plugin-api/src/command/builder.rs`
- [X] T033 [US2] Keep `register_player_command` working as a thin wrapper (spleef example) or migrate spleef to the builder

## Phase 5: User Story 3 – shared services + Vault (P1)

**Independent test**: third plugin deposits via `vault:economy/v1`; `/balance` reflects it.

- [X] T034 [US3] Host `ServiceRegistry` + bindings (`register_service`, `service`, `service_providers`) in `steel-plugin-api/src/host/services.rs` and `steel-plugin-api/src/services.rs`, with typed helpers checking `TypeReport` compatibility
- [X] T035 [US3] Service tests (priority selection, incompatible report rejected, none → error) in `steel-plugin-api/src/host/services.rs`
- [X] T036 [P] [US3] `plugins/vault-api/src/lib.rs`: Economy, Permission, Chat stabby traits, `EconomyResponse`, keys `vault:economy/v1`, `vault:permission/v1`, `vault:chat/v1`, helper getters
- [X] T037 [US3] `plugins/vault/src/lib.rs`: Steel-backed Permission and Chat providers (metadata `vault:prefix`, `vault:suffix`), `/vault-info`, README

## Phase 6: User Story 4 – Essentials (P2)

**Independent test**: home persists across restart; `/pay` + `/baltop`.

- [X] T038 [US4] Scaffold `plugins/essentials` (Cargo, lib.rs, config.rs with defaults from contracts/ports.md, messages)
- [X] T039 [P] [US4] Storage (`storage.rs`): User/Warps/Spawn TOML, atomic write via tmp+rename, dirty flush every 100 ticks and on shutdown; names `[a-z0-9_-]{1,32}`
- [X] T040 [P] [US4] Economy (`economy.rs`): cents i64, validation (finite, ≥0, ≤max_money, ≤2 decimals), `vault:economy/v1` provider, `/balance /pay /baltop /eco`
- [X] T041 [US4] Teleport features (`teleport.rs`): homes with limit, warps, spawn, tpa/tpahere/tpaccept/tpdeny with expiry, `/back` via Teleport + Death events, `/tp /tphere`
- [X] T042 [US4] Player utilities (`player_cmds.rs`): god (Damage event), fly, heal, feed, gamemode shortcuts, nick, broadcast, msg/r
- [X] T043 [US4] AFK (`afk.rs`): activity from Move/Chat/Command events, timeout via scheduler, broadcast
- [X] T044 [US4] Kits (`kits.rs`) with cooldown + per-kit permission
- [X] T045 [US4] Chat format, join/quit messages (`chat.rs`) using Vault chat service when present
- [X] T046 [US4] Tests for economy parsing/validation and storage round trip; README with manual test script

## Phase 7: User Story 5 – WorldEdit (P2)

**Independent test**: 10×10×10 `//set stone`, `//undo`, `//redo`.

- [X] T047 [P] [US5] `plugins/worldedit-api/src/lib.rs`: `Selection` trait, `Cuboid`, key `worldedit:selection/v1`
- [X] T048 [US5] `plugins/worldedit`: sessions, wand via Interact events (cancel break on left click), pos/hpos commands, selection service provider
- [X] T049 [US5] Patterns/masks parsing (`pattern.rs`) with weighted mixes + tests
- [X] T050 [US5] Edit operations (`edit.rs`): set, replace, walls, count, expand/contract, copy/cut/paste, undo/redo, block limit, loaded-chunk precheck; tests for undo/redo and clipboard offsets
- [X] T051 [US5] README with manual test script

## Phase 8: User Story 6 – WorldGuard (P2)

**Independent test**: build-deny region blocks non-members only; greeting on entry.

- [X] T052 [US6] `plugins/worldguard`: region model, per-world chunk index, TOML persistence (refuse to overwrite unreadable files)
- [X] T053 [US6] Flag resolution (priority, parent inheritance, membership, global region, bypass) + tests
- [X] T054 [US6] `/rg` commands using `worldedit:selection/v1`
- [X] T055 [US6] Enforcement handlers: break/place/interact/chest-access/pvp/mob-damage/invincible/entry/exit/greeting/farewell
- [X] T056 [US6] README with manual test script

## Phase 9: User Story 7 – plugin hygiene (P3)

- [X] T057 [US7] Plugin-side `scheduler.rs` (run_later, run_repeating, cancel) driven by server tick
- [X] T058 [US7] `log` helpers and documentation of threading model in `steel-plugin-api/README.md`

## Phase 10: Polish

- [X] T059 Update `steel-plugin-api/README.md` (events, commands, services, lifecycle v3, hello-world ≤50 lines)
- [X] T060 Run `cargo fmt`, `cargo clippy` for steel-core, steel-plugin-api (host + plugin), plugins workspace; fix warnings
- [X] T061 Run full verification: core tests touching extension/player paths, plugin-api tests, build examples and plugins; record results in quickstart

## Dependencies

- Phase 2 blocks everything. US1 (events + player access) blocks US4–US6. US2 blocks all ports.
  US3 blocks US4 (economy provider) and the WorldEdit selection service pattern.
- US5 blocks US6 (selection service). US7 scheduler is needed by US4 (AFK, saves); implement T057
  before T043 if working sequentially.

## Parallel opportunities

- T011/T012, T026/T027, T036 with T034, T039/T040, T047 with T048 prep.

## Implementation strategy

MVP = Phases 1–4 (lifecycle, events, commands). Then services + Vault, then Essentials,
WorldEdit, WorldGuard, each independently testable in a live server.
