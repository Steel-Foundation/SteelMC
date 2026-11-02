# Implementation Plan: Toxic Wasteland

**Branch**: `003-toxic-wasteland` | **Date**: 2026-11-01 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/003-toxic-wasteland/spec.md`

## Summary

Add two reusable platform capabilities to SteelExtensionMC, then build the `toxic-wasteland`
extension on them. Both capabilities follow coco's model: the server stores the real identity, and
clients receive a vanilla projection.

1. **Custom fluids** ([research R1–R3](research.md)):
   - Fluid pairs, a 16-state liquid block projected onto vanilla water or lava states, and
     host-run vanilla spreading driven by definition data, with optional extension hooks.
   - A `client_fluid` projection that entity movement uses, so physics match client prediction
     without tagging the fluid as water.
2. **Custom biomes and tint projection** ([R5–R8](research.md)):
   - Registered biomes are synced and saved by key.
   - Client-only tinted variants of every biome. Per-cell counters project the variants wherever
     a tinting fluid sits.
   - Live refreshes go through the new `CChunksBiomes` packet.
   - A runtime `set_biomes` (`/fillbiome` semantics).

The extension uses both to add the sludge, the biome, a bucket, the gameplay hazards, a seeded
wasteland generator with sludge lakes, and the `/toxic` admin command.

## Technical Context

**Language/Version**: Rust (workspace `rust-toolchain.toml`, nightly with `-Zthreads=0`)

**Primary Dependencies**: existing only: `stabby` (ABI), `steel-registry`, `steel-core`,
`steel-protocol`, `steel-extension-api`. The extension uses `serde` and `toml`, like the other
examples. No new crates (constitution VI).

**Storage**: Anvil-style chunk saves (biome and block palettes by key) and the extension's
`config.toml`.

**Testing**: `cargo test` unit tests in the touched crates, plus a native integration test
(`steel-extension-api/tests/toxic_wasteland/`) that loads the built extension, like
`lucky_block.rs`.

**Target Platform**: Linux server and vanilla Java clients for protocol 777.

**Project Type**: server platform (host crates) plus a native extension (cdylib).

**Performance Goals**: painting 256×256×384 in under 2 s; pouring 65k blocks in about 6 ms; zero
extra bytes per chunk when no tinting fluid is present (SC-005, SC-006).

**Constraints**:
- No mod and no resource pack.
- Byte-identical vanilla output without extensions (FR-022).
- Callbacks run on the world tick thread.

**Scale/Scope**: about 65 vanilla biomes, giving about 66 client-only variants per tinting fluid.
One tinting fluid in this iteration, with the design supporting N.

## Constitution Check

| Principle | Status | How |
| --- | --- | --- |
| I. Stable Native Boundary | Pass | `FluidDefinition`, `BiomeDefinition` and `DynFluidBehavior` are stabby types. The new `Entity` methods are under a new versioned module; existing keys are unchanged. Handles stay borrowed and `!Send`. |
| II. Modding-Grade Extensibility | Pass | Fluids and biomes are generic registry entries. `ClientFluidKind` is identifier-backed, and N tinting fluids are supported. Projection is separate from tags, as in NeoForge's `FluidType`. |
| III. Vanilla Parity First | Pass | `fluid_state_to_block` via `Fluid::block` keeps vanilla's output (tests). `react_with_neighbors` defaults to vanilla lava rules. Packets are unchanged without content. Deviations (variant keys visible in F3, sludge not waterlogging) are documented. |
| IV. Foundation Rigor | Pass | Fluid and biome projection is complete for its declared scope: chunk sends, updates, tags, registry and saves. Out-of-scope items (biome decorators, waterlogging with custom fluids, dispensers) carry `// TODO:` comments. |
| V. Explicit Failure | Pass | Every registration and runtime call returns `ApiResult`. Unloaded chunks and over-limit regions are errors, not silent skips. |
| VI. Concurrency & Dependencies | Pass | No new threads or crates. The biome broadcast reuses the existing per-tick loop. |
| VII. Verified Behavior | Pass | Tests cover the projection, counters, packet, persistence round-trips, gameplay rates and generator determinism ([quickstart](quickstart.md)). |

No violations; Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/003-toxic-wasteland/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── extension-api.md
│   ├── network.md
│   └── commands.md
└── tasks.md            # /speckit-tasks
```

### Source Code

```text
steel-registry/src/
├── client_content/
│   ├── fluid.rs                 # NEW ClientFluid, register_client_fluid, client_fluid()
│   ├── biome.rs                 # NEW register_client_biome, ClientBiomes variant table
│   └── mod.rs / registration.rs # wiring, freeze builds the variant table
└── fluid/mod.rs                 # FluidState::client_fluid(), fluid key helpers

steel-protocol/src/packets/game/chunk/
└── biomes.rs                    # NEW CChunksBiomes

steel-core/src/
├── fluid/
│   ├── state.rs                 # fluid_state_to_block via Fluid::block
│   ├── fluid_behavior.rs        # react_with_neighbors hook
│   └── fluids/extension.rs      # NEW ExtensionFluid (FluidBehavior + FlowingFluid)
├── behavior/blocks/fluid/liquid_block.rs  # calls react_with_neighbors
├── behavior/items/bucket.rs     # bucket generalized to any fluid
├── entity/fluid_contact.rs, living_entity.rs, entity/mod.rs  # movement uses client_fluid
├── chunk/section.rs             # tint_cells counters, projected biome write
├── chunk/paletted_container.rs  # client biome bit width, projected write helper
├── chunk/chunk_holder.rs        # changed_biome_sections
├── chunk/chunk_map/mod.rs       # CChunksBiomes broadcast
├── world/biomes.rs              # NEW fill_biome / biome_at
└── server/registry_cache.rs     # variants in registry + tags, fluid tag filter

steel-extension-api/src/
├── content/fluid.rs, content/biome.rs   # NEW definitions + register_fluid/register_biome
├── host/content/fluid.rs, biome.rs      # NEW host construction and validation
├── fluid_behavior.rs                    # NEW DynFluidBehavior + host bridge
├── worlds.rs / bindings.rs              # biome_at, set_biomes
└── block_behavior/entity_v2.rs          # NEW living-entity operations
steel-extension-api/tests/toxic_wasteland/  # NEW native integration test

extension-example/toxic-wasteland/      # NEW extension crate
├── Cargo.toml, extension.toml, clippy.toml, README.md
└── src/{lib.rs, config.rs, content.rs, sludge.rs, hazards.rs, generator.rs, commands.rs}

doc/extensions/
├── custom-content.md            # + Custom fluids, Custom biomes, Tint projection sections
└── api.md                       # + reference entries
```

**Structure Decision**: platform work goes into the existing crates, next to coco's
`client_content` and content API, because it extends them. The extension is a standalone crate in
`extension-example`, like every other example, and packaged with `package-extension.py`.

## Complexity Tracking

None.
