# Implementation Plan: Plugin API Foundations and Reference Plugin Ports

**Branch**: `another-plugin-experiment` | **Date**: 2026-10-31 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/001-plugin-api-ports/spec.md`

## Summary

Turn the experimental native plugin boundary into an API real server plugins can target, then
prove it with native ports of Vault, EssentialsX, WorldEdit and WorldGuard. Core gains an
ABI-independent cancellable event bus fired at vanilla decision points; `steel-plugin-api` gains
event handles, a typed command tree bridge, player/world/permission/text/inventory access, a
cross-plugin service registry, and a v3 lifecycle (dependencies, data directory, shutdown). A
plugin-side helper layer turns trampolines + tokens into closures so plugins stay ergonomic.

## Technical Context

**Language/Version**: Rust nightly (`rust-toolchain.toml`), edition 2024

**Primary Dependencies**: stabby (fork submodule, `host_api`/`opaque`), libloading, text_components
(serde), toml/serde (ports only)

**Storage**: Per-plugin TOML files in `plugins/<name>/`

**Testing**: `cargo test` (core unit tests for event bus/fire points, plugin-api host tests,
native fixture integration tests), manual live-client scripts in each port README

**Target Platform**: Linux server (also builds on macOS/Windows)

**Project Type**: Library (plugin API) + native plugin cdylibs + server binary

**Performance Goals**: No measurable tick cost with zero handlers; region checks O(regions in
chunk); 100k-block edit without sustained TPS drop (SC-004)

**Constraints**: No panics across the boundary; handlers `Send + Sync`; no blocking on async in
ticks; plugins only use the public API (SC-002)

**Scale/Scope**: ~15 event kinds, ~60 API functions, 4 port plugins (~60 commands total)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evidence |
|---|---|---|
| I. Stable Native Boundary | PASS | All new boundary types are stabby; events are opaque borrowed handles; lifecycle bump to `steel_plugin_v3`; services checked by `TypeReport` |
| II. Modding-Grade Extensibility | PASS | Events/services/argument types keyed by identifier; damage cause and entity type are identifiers, not closed enums; services allow any plugin contract |
| III. Vanilla Parity First | PASS | Fire points at vanilla decision points; empty bus is a no-op; chat stays signed unless a plugin overrides rendering (documented deviation) |
| IV. Foundation Rigor | PASS | Event bus, services and command bridge complete for declared scope; out-of-scope items carry TODOs (undo of block entities, group mutation) |
| V. Explicit Failure | PASS | All API calls return `ApiResult`; handler errors are logged per plugin; `extern "C"` panics abort rather than unwind (documented) |
| VI. Justified Concurrency & Dependencies | PASS | No new threads or async; no new dependencies in core; handler threads documented |
| VII. Verified Behavior | PASS | Tests for bus ordering/cancel, service report checks, dependency sort, command tree build, WE undo, WG flag resolution, persistence round trip |

Post-design re-check: PASS (no violations; Complexity Tracking empty).

## Project Structure

### Documentation (this feature)

```text
specs/001-plugin-api-ports/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── events.md
│   ├── commands.md
│   ├── services.md
│   ├── lifecycle.md
│   └── ports.md
└── tasks.md
```

### Source Code (repository root)

```text
steel-core/src/extension/
├── mod.rs                # ExtensionHooks gains `events: Events`
└── events/
    ├── mod.rs            # EventBus, EventPriority, EventHandler, Events
    ├── player.rs         # pre-login, join, quit, chat, command, move, teleport, death
    ├── block.rs          # break, place, interact
    ├── entity.rs         # damage
    └── tests.rs

steel-core/src/...        # fire points listed in research R2 (one call each)

steel-plugin-api/src/
├── lib.rs                # re-exports, export_plugin! v3
├── entry.rs              # PluginDescriptor v3
├── events/               # opaque event handles + bindings (host_api)
├── command/              # CommandNodeSpec, CommandInvocation bindings, plugin-side builder
├── player.rs             # Server player accessors (host_api)
├── text.rs               # JSON text + legacy & codes (plugin side)
├── services.rs           # service registry bindings + typed helpers
├── scheduler.rs          # plugin-side tick scheduler
├── callbacks.rs          # plugin-side closure tables / trampolines
└── host/
    ├── events.rs         # core EventHandler impls calling native callbacks
    ├── commands.rs       # tree builder
    ├── services.rs       # ServiceRegistry
    ├── lifecycle.rs      # dependency sort, context, shutdown
    └── players.rs        # player/permission/inventory helpers

plugins/                  # standalone Cargo workspace (built separately from the server)
├── Cargo.toml
├── vault-api/            # rlib: Economy, Permission, Chat contracts
├── vault/                # cdylib: Steel-backed permission + chat providers, /vault-info
├── essentials/           # cdylib
├── worldedit-api/        # rlib: Selection service contract
├── worldedit/            # cdylib
└── worldguard/           # cdylib (depends on worldedit)
```

**Structure Decision**: Core keeps ABI-free events next to existing extension hooks; the plugin
API keeps one module per concern following its current `host_api` layout; ports live in their
own workspace like `examples/` so their builds never enable the `host` feature.

## Implementation Phases

1. **Foundation – lifecycle v3 + callbacks layer**: descriptor, dependency sort, context, data
   dir, shutdown; plugin-side closure tables; update examples/fixture.
2. **Foundation – core event bus + fire points**, with core tests.
3. **Foundation – plugin API surfaces**: event handles, players/permissions/text/inventory,
   world additions, typed commands, services, scheduler.
4. **Vault port** (contracts + Steel permission/chat providers).
5. **Essentials port**.
6. **WorldEdit port** (+ selection service).
7. **WorldGuard port**.
8. **Polish**: READMEs, docs, clippy/fmt, full build matrix.

## Complexity Tracking

No constitution violations.
