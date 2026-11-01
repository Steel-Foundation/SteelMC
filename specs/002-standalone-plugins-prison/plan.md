# Implementation Plan: Standalone Plugins, Full API Coverage and OP Prison Demo

**Branch**: `another-plugin-experiment` | **Date**: 2026-10-31 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/002-standalone-plugins-prison/spec.md`

## Summary

Make plugins drop-in files built from independent projects, widen the event and game API to
everything Steel's core supports after the upstream merge (including explosions), add a
Paper/FAWE-style bulk block fill to steel-core, and ship an OP Prison bundle (mines + enchants)
that showcases mine regeneration speed and custom enchant effects.

## Technical Context

**Language/Version**: Rust nightly (workspace toolchain), edition 2024

**Primary Dependencies**: stabby fork, libloading, text_components, simdnbt; plugin projects use
serde/toml

**Storage**: Plugin TOML files under `plugins/<name>/`

**Testing**: core unit tests (events, bulk fill correctness, protocol packet encoding), plugin-api
host tests, native integration tests loading the plugin projects, ignored benchmark for bulk fill

**Target Platform**: Linux server (portable library suffix handling)

**Project Type**: Server + plugin API library + independent plugin workspaces

**Performance Goals**: 125k-block reset < 100 ms, 1M-block reset < 1 s, 20 TPS during
continuous Jackhammer procs (SC-003, SC-004)

**Constraints**: No physics in bulk fills; no unloaded chunk loading; events zero-cost when
unsubscribed; plugins only use the public API

**Scale/Scope**: ~25 new events, ~60 new API functions, 5 protocol packets, 2 demo plugins + 1
contract crate, template project

## Constitution Check

| Principle | Status | Evidence |
|---|---|---|
| I. Stable Native Boundary | PASS | Descriptor bump to v4 with API version; new handles opaque; item specs/menus as stabby structs |
| II. Modding-Grade Extensibility | PASS | Particles, sounds, effects, entity types and enchantments by identifier; services for mines |
| III. Vanilla Parity First | PASS | Events at vanilla decision points; packets checked against vanilla sources; bulk fill is an explicit plugin opt-in, documented as non-vanilla (no physics) |
| IV. Foundation Rigor | PASS | Bulk fill handles block entities, heightmaps, light and client sync; skipped-plugin diagnostics |
| V. Explicit Failure | PASS | All new calls return results; bad plugin files skipped, not panicking |
| VI. Concurrency & Deps | PASS | Fills run on the caller's serialized mutation thread; no new threads or deps in core |
| VII. Verified Behavior | PASS | Tests for discovery/skipping, every event, item data round trip, packet encodings, fill correctness, prison logic; benchmark |

Post-design re-check: PASS. Per-player sidebars deviate from vanilla's global scoreboard by design
(documented in the module).

## Project Structure

```text
steel-core/src/
├── extension/events/{player,block,entity,world,server,inventory}.rs   # new events
├── world/bulk_fill.rs                                                 # fill_region
├── player/hud.rs                                                      # sidebar/boss bar/tab overrides
└── (fire points per research R3)
steel-protocol/src/packets/game/
├── c_set_objective.rs, c_set_display_objective.rs, c_set_score.rs,
│   c_reset_score.rs, c_boss_event.rs
steel-login/src/handlers/status.rs                                     # ping event
steel-plugin-api/src/
├── events/ (new handles + handlers)
├── items.rs, menus.rs, effects.rs (particles, sounds, potions), hud.rs, worlds.rs (with_world,
│   fill, time/weather), messaging.rs
└── host/{loader.rs (folder scan), items.rs, menus.rs, hud.rs, worlds.rs}
steel/src/lib.rs                                                       # folder loading, /plugins
plugin-template/                                                       # standalone workspace
plugin-projects/                                                       # moved ports + prison
├── Cargo.toml ([patch] to this checkout)
├── vault-api, vault, essentials, worldedit-api, worldedit, worldguard
├── prison-api, prison-mines, prison-enchants
└── README.md (demo script)
```

## Phases (one local commit each)

1. Drop-in loading, descriptor v4, `/plugins` `/version`, standalone workspaces + template.
2. Events: player, inventory, entity, world, server (+ explosion, piston) with tests.
3. Game API: items/data, inventories, menus, particles/sounds/effects/xp, entities, HUD packets
   (sidebar, boss bar, tab list), time/weather, plugin messaging, world access by key.
4. Bulk fill in core + API + benchmark.
5. Prison mines (+ prison-api).
6. Prison enchants, menu, auto-sell, sidebar.
7. Polish: docs, clippy/fmt, full test matrix, live smoke run.

## Complexity Tracking

No violations.
