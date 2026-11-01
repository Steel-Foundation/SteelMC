---
description: "Task list for Standalone Plugins, Full API Coverage and OP Prison Demo"
---

# Tasks: Standalone Plugins, Full API Coverage and OP Prison Demo

**Input**: `specs/002-standalone-plugins-prison/` (plan, spec, research, data-model, contracts)

**Tests**: Required by constitution VII for discovery, events, item data, packets, bulk fill and
prison logic.

## Phase 1: Setup

- [X] T001 Move `plugins/` to `plugin-projects/` (own workspace); switch dependencies to the git URL `https://github.com/coco875/SteelMC` branch `another-plugin-experiment`; add `plugin-projects/.cargo/config.toml` `[patch]` to `../steel-plugin-api`; update `.gitignore`, tests and docs paths
- [X] T002 [P] Create `plugin-template/` standalone workspace (Cargo.toml git dependency, `src/lib.rs` hello plugin, README with build/install and optional `[patch]`)

## Phase 2: Foundational

- [X] T003 Descriptor v4 with `api_version` in `steel-plugin-api/src/entry.rs` and `export_plugin!` (`steel_plugin_v4`); old symbols reported as "rebuild"
- [X] T004 Folder discovery + skip diagnostics in `steel-plugin-api/src/host/mod.rs`: scan `plugins/`, config from `plugins/<name>/config.toml` (created empty) unless overridden, skip unopenable/incompatible/duplicate/missing-dependency plugins transitively; unit tests for the skip/ordering logic
- [X] T005 `steel/src/lib.rs`: load folder + explicit entries; `/plugins` and `/version <plugin>` commands (core registration from `InitializedPlugins` metadata)

## Phase 3: US1 – drop-in plugins (P1)

- [X] T006 [US1] Build `plugin-projects` and `plugin-template` against the patch; verify a template build in a temp dir outside the repo with only the git dependency + patch; integration test loading a directory of libraries (`steel-plugin-api/tests/folder.rs`)

## Phase 4: US2 – events (P1)

- [X] T007 [P] [US2] Core events in `steel-core/src/extension/events/{player,inventory}.rs`: respawn, world change, game mode, held slot, sneak/sprint/flight, swap hands, drop, pickup, entity interact, inventory open/click/close, sign change, experience, food; fire points per research R3
- [X] T008 [P] [US2] Core events in `steel-core/src/extension/events/{entity,world,server}.rs`: entity spawn, living death (drops/xp), explosion (block list), piston, chunk load/unload, world load/unload, weather, server command, server list ping (`steel-login/src/handlers/status.rs`), plugin message; `BlockBreakEvent` drop/xp control in `block_breaking.rs`
- [X] T009 [US2] Plugin handles + closure helpers for all new events in `steel-plugin-api/src/events/` and `host/events.rs`
- [X] T010 [US2] Tests: core bus tests for new cancellation semantics; integration test firing a representative set from a test peer

## Phase 5: US3 – game API (P1)

- [X] T011 [P] [US3] Protocol packets `CSetObjective`, `CSetDisplayObjective`, `CSetScore`, `CResetScore`, `CBossEvent` in `steel-protocol/src/packets/game/` verified against vanilla sources + encoding tests
- [X] T012 [P] [US3] Items: `ItemSpec` (name, lore, glint, unbreakable, enchantments, namespaced plugin data in `custom_data`) in `steel-plugin-api/src/items.rs` + host conversion + round-trip test; inventory slot/held/cursor/give/remove API
- [X] T013 [US3] Plugin chest menus (core read-only plugin menu over `SimpleContainer`, click routing) in `steel-core/src/inventory/menu/` + `steel-plugin-api/src/menus.rs`
- [X] T014 [P] [US3] Particles, sounds, potion effects, experience, walk/fly speed in `steel-plugin-api/src/effects.rs`
- [X] T015 [P] [US3] Entities: drop item, visual lightning, remove entity
- [X] T016 [US3] HUD: per-player sidebar, boss bars, tab list override (`steel-core/src/player/hud.rs`, `steel-plugin-api/src/hud.rs`)
- [X] T017 [P] [US3] Worlds: `Server::with_world`, time/weather get/set; plugin messaging channels + send

## Phase 6: US4 – bulk fill (P1)

- [X] T018 [US4] `steel-core/src/world/bulk_fill.rs`: shapes, clip, pattern, section-batched writes, block entity detach, heightmaps, selective light queueing, client resend; correctness tests (states, heightmaps, block entities, unloaded refusal)
- [X] T019 [US4] Ignored benchmark per-block vs bulk for 125k and 1M blocks; record numbers
- [X] T020 [US4] API `World::fill(FillRequest) -> FillReport` with replaced-state histogram

## Phase 7: US5 – prison mines (P2)

- [X] T021 [P] [US5] `plugin-projects/prison-api`: `prison:mines/v1` contract
- [X] T022 [US5] `plugin-projects/prison-mines`: storage, `/mine` commands, reset (lift players, bulk fill, report ms + blocks/s), timer + threshold via scheduler, boss bar countdown, service provider; tests for mix parsing, overlap, threshold math, persistence

## Phase 8: US6 + US7 – enchants, menu, auto-sell, HUD (P2/P3)

- [X] T023 [US6] `plugin-projects/prison-enchants`: pickaxe item + lore rendering, enchant config, menu upgrade flow via Vault economy
- [X] T024 [US6] Procs: Jackhammer, Explosive, Nuke, Soundwave (multi-tick shell + note particles), Haste/Speed effects, Fortune; particles and sounds
- [X] T025 [US7] Auto-sell (drops off, histogram × values × fortune → deposit), blocks-mined stats, sidebar
- [X] T026 [US6] Tests: enchant math (cost/chance), lore/data round trip, sell totals; README demo script

## Phase 9: Polish

- [X] T027 Update `steel-plugin-api/README.md`, plugin-projects READMEs, quickstart with benchmark numbers
- [X] T028 fmt, clippy (core, api host+plugin, plugin-projects, template); full test matrix; live release smoke run with the bundle in `plugins/`

## Dependencies

Phase 2 → everything. US2 (events) and US3 (items, menus, HUD) and US4 (fill) precede US5–US7.
US5 precedes US6/US7 (mine service). Commit after each phase.
