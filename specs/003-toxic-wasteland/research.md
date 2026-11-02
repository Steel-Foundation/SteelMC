# Research: Toxic Wasteland

Each entry records a decision, why it was made, and what was rejected. All findings are from the
code at `master` `a90670df` (coco's custom-content commit on top of the plugin platform merge).

## R1. How a custom fluid gets water physics without becoming water

**Decision**: add a **client fluid projection**, the fluid analogue of coco's `client_block_state`.
Every custom fluid declares the vanilla fluid that clients see it as (`minecraft:water` or
`minecraft:lava`). Entity movement code asks for the *predicted* fluid
(`FluidState::client_fluid()`), while chemistry and identity code keep using the real fluid and its
tags. The sludge is **not** added to `#minecraft:water`.

**Rationale**:
- About 60 call sites in `steel-core` use `is_water()` or `is_lava()`. They split into two kinds:
  - **Movement and prediction** (what the client simulates on its own): `entity/fluid_contact.rs`
    (fluid heights, currents, eye-in-fluid), `living_entity.rs` (`jump_in_liquid`), and the
    `block_fluid_is_water` flag in `entity/entity/mod.rs`. These must match the client, which only
    sees water.
  - **Gameplay and identity**: lava+water to obsidian, sponges, concrete powder, kelp survival,
    worldgen, `fluid_state_to_block`. These must follow the real fluid. Tagging sludge as water
    would make sponges eat it, make lava turn it into obsidian (contradicting FR-014), and make
    `fluid_state_to_block` turn every sludge spread into vanilla water.
- This mirrors NeoForge's `FluidType`, which split movement properties from tags for the same
  reason. It meets constitution principle II (modding-grade).

**Alternatives considered**:
- *Tag sludge as `#minecraft:water`*: the simplest option, but it breaks identity in
  `fluid_state_to_block` and the chemistry in about 50 places. Rejected.
- *A full `FluidType` struct with every physics property (motion scale, drowning, swim speed)*:
  over-designed for vanilla clients, which can only predict water or lava. The projection keeps
  room for it (`ClientFluid` can grow fields), but the API exposes only what clients can display.

## R2. The liquid block and its client carrier

**Decision**: register the fluid's block as a **state family with the same layout as
`minecraft:water`**: the `level` property and 16 states, with each state's `StateFluidData`
pointing at the custom source or flowing fluid. Its client carrier is the water state with the
same offset, using the same offset-based template projection that `ClientBed` uses for bed
families. The host installs `LiquidBlock::new(block, fluid)` as its behavior.

**Rationale**: `LiquidBlock` is already generic over `(block, fluid)`. The bed path proves that
per-state projections of a whole family work everywhere `client_block_state` is consulted:
chunk palettes, block updates, particles, falling blocks. Saving keys the block by its own key,
and the `level` property round-trips with no extra work.

**Required generalization**: `fluid_state_to_block` must resolve the block through the fluid
(`Fluid::block`) instead of the water/lava `if`-chain. That is vanilla's
`FluidState.createLegacyBlock()`, which each fluid owns. The vanilla output stays identical, which
the existing state tests verify.

## R3. Fluid behavior across the ABI

**Decision**: a host-side `ExtensionFluid` implements `FluidBehavior` and `FlowingFluid`.
- Its *numbers* come from a `FluidDefinition` value: tick delay, drop-off, slope distance, source
  conversion, explosion resistance, randomly ticking.
- Its *hooks* call an optional stabby `DynFluidBehavior` trait object: `entity_inside`,
  `random_tick`, `before_destroying_block`, `can_be_replaced_with`, and `react_with_neighbors`.

Spreading itself always runs the host's vanilla `FlowingFluid` algorithm.

**Rationale**: spreading is hot (it runs every fluid tick). Its parameters are plain data, so
passing them once avoids crossing the ABI on every tick. Hooks are rare and gameplay-specific.
This follows coco's split between definitions and behaviors.

**New core hook**: `FluidBehavior::react_with_neighbors(world, pos) -> bool`, called from
`LiquidBlock::should_spread_liquid`. The default keeps lava's vanilla reactions (obsidian,
cobblestone, basalt), so behavior is unchanged with no extension present. It lets sludge
implement FR-014 (lava to coarse dirt or cobblestone) at the same vanilla decision point.

## R4. Damage and poison for any living entity

**Decision**: extend the borrowed `Entity` handle in block and fluid callbacks with versioned
methods: `is_living`, `is_creative_or_spectator`, `hurt(damage_type, amount) -> bool`,
`add_effect(Effect) -> ApiResult<bool>`, and `can_be_affected(effect)`. They use the core's
existing `LivingEntity::hurt` and `add_effect`, so vanilla immunity, armor and the invulnerability
timer apply.

**Rationale**: the existing `effects.rs` API targets players only (`PlayerId`). The spec covers
every living entity, including undead immunity. Damage types come from the registry
(`minecraft:magic`, and so on), so extensions can also register their own later.

## R5. Custom biomes

**Decision**: `ContentRegistry::register_biome(BiomeDefinition)` builds a `steel_registry::biome::Biome`
before the registries freeze, through coco's content registration hook. The biome joins the
synced `minecraft:worldgen/biome` registry, so `RegistryCache` sends it with its NBT like any
vanilla entry. Chunks already save biomes **by key** (`chunk_saver/storage/chunk.rs` resolves
`id_from_key` and fails on unknown keys), so persistence and the missing-extension error come for
free.

**Validation**: colors are 24-bit RGB values, temperature and downfall are finite, and music and
particle keys must resolve. A duplicate key is an error.

**Generators**: the existing `register_terrain_generator(key, dimension, biome, ...)` already
resolves the biome by key at registration time. Content registration runs before generator
registration, so a custom biome key works as is (FR-011). A test confirms the order.

## R6. Tint follows the fluid: client biome projection

**Decision**:
1. **Client-only variants.** For each fluid with a tint, the client biome registry is
   `server biomes ++ variants`. There is one variant per (tinting fluid, server biome), identical
   to its source except for `water_color` and `water_fog_color`. Variants are *not* server
   registry entries, so commands, `/locate`, worldgen and saves never see them. They are keyed
   `<fluid namespace>:<fluid path>_tint/<biome namespace>/<biome path>`.
2. **Tags.** In the tags packet, each variant joins every biome tag its source biome is in. This
   keeps client behavior driven by biome tags (water fog distance, underwater music) identical.
3. **Network bit width.** `biome_global_bits()` uses the *client* biome count. The server's local
   palettes don't change.
4. **Per-cell tracking.** `ChunkSection` keeps a `tint_cells: Option<Box<[u16; 64]>>` count of
   tinting fluid blocks per 4×4×4 cell. `set_block_state` updates it like the existing
   `fluid_count` counters, and `recalculate_counts` rebuilds it. It is `None` (zero cost) in
   sections that never held a tinting fluid.
5. **Projection at write time.** `ChunkSection::write` sends `biomes` unchanged when no cell is
   tinted. Otherwise it writes a temporary projected palette in which each tinted cell carries its
   variant id. If several tinting fluids share a cell, the fluid with the most blocks wins, and
   ties go to the lowest fluid id (deterministic).
6. **Live updates.** When a cell goes between 0 and more than 0 tinting blocks, or a biome is
   painted, the section is marked in a new per-holder `changed_biomes` section set. The existing
   `broadcast_changed_chunks` loop, which already drains `take_changed_blocks`, also drains it and
   sends **one `CChunksBiomes` packet per tick** to the tracking players. That packet carries
   every changed chunk for each player, sent right after the block updates of the same tick.

**Rationale**: this keeps server truth separate from client presentation, which is coco's model.
Vanilla clients with no tinting fluid receive byte-identical data (FR-022 and SC-006). Write-time
projection costs nothing when no cell is tinted.

**Alternatives considered**:
- *Change the real biome where sludge flows*: corrupts gameplay (spawns, temperature, snow) and
  saved data. Rejected; the user picked "tint follows sludge".
- *Rescan the section at send time instead of keeping counters*: 4,096 lookups on every chunk send
  for sludge sections, and no cheap way to detect a 0/1 change for live updates. Rejected.
- *Register variants in the server registry*: they would leak into `/locate` and suggestions and
  inflate the server palette width. Rejected.

## R7. Changing biomes at runtime (paint)

**Decision**: add a new `World::set_biomes(region, biome) -> ApiResult<u64>` with a core
`World::fill_biome`. It is vanilla's `/fillbiome` semantics: whole 4×4×4 cells whose quart
coordinates intersect the region, and loaded chunks only. Painting a box over unloaded chunks
returns an error that lists how many chunks are unloaded, rather than skipping them silently.
Changed chunks are marked unsaved, and changed sections go into `changed_biomes`.

**Rationale**: it matches vanilla's `fillbiome` (`BiomeResolver` per quart). Section palettes
already support `set` on `BiomePalette`.

## R8. The `CChunksBiomes` packet

**Decision**: add `steel-protocol::packets::game::chunk::CChunksBiomes { chunks: Vec<ChunkBiomeData> }`
with `ChunkBiomeData { pos: ChunkPos, buffer: Vec<u8> }`. The buffer holds the biome container of
each section in order, using the same projected writer as the full chunk packet. Its ID is
`play::C_CHUNKS_BIOMES` (13), which is already generated.

**Rationale**: this is vanilla's `ClientboundChunksBiomesPacket`, used by `/fillbiome`. The client
re-tints the affected sections without reloading.

## R9. Bucket

**Decision**: generalize `behavior/items/bucket.rs` from `fluid_block == WATER` checks to the
bucket's fluid. "Water-like" behavior (nether evaporation, the water sound, cauldron and
waterlogging rules) comes from `client_fluid().is_water()`, except **waterlogging**, which stays
real-water-only (spec assumption). Pick-up works through `LiquidBlock::pickup_block`, which
returns the fluid's `bucket_item`. A custom fluid declaring a bucket registers a placing item with
the core `BucketItem` behavior for that fluid.

## R10. Fluid destroying plants

**Decision**: `FluidBehavior::before_destroying_block` already exists, and water uses
`world.drop_resources`. `ExtensionFluid` uses the same drops by default, and the extension's hook
may add effects. Which blocks get destroyed is decided by the core's `can_hold_fluid` and
`can_pass_through_wall` rules, which treat any non-waterlogging fluid like water for replaceable
blocks. Vanilla's `FlowingFluid.canHoldFluid` is fluid-agnostic apart from waterlogging, so this
is parity.

## R11. Random ticks: corruption and withering

**Decision**: the fluid definition's `randomly_ticking` flag sets the custom fluids'
`is_randomly_ticking`, and the section's `ticking_fluid_count` already drives random fluid ticks.
`ExtensionFluid::random_tick` calls the extension. The extension rolls the configured
probabilities with the world's random source (exposed through the existing `Random` handle).

## R12. Wasteland generator and lakes

**Decision**: use the existing terrain generator API (`register_terrain_generator` plus a
`Terrain` callback writing block states), with toxic biome cells everywhere.
- The terrain is a seeded 2D value-noise heightmap with three octaves, implemented in the
  extension. It needs no new dependency (constitution VI).
- Lakes are placed per chunk from a hash of the seed and chunk position. Each one is a basin
  carved below the local minimum height and filled with sludge sources up to its rim. Its
  footprint stays inside the chunk, so generation never writes into neighbors.

**Rationale**: it stays deterministic (SC-007) and matches the "terrain-only generator" contract
(no decorators). Chunk-local lakes avoid cross-chunk writes, which the terrain phase doesn't
allow.

## R13. Admin commands and the WorldEdit selection

**Decision**: one `/toxic` command tree:
- `paint <biome> [from] [to]`
- `pour [from] [to]`
- `drain [from] [to]`

Without coordinates, it resolves the `worldedit:selection/v1` service (soft dependency). Pour and
drain use the existing bulk `World::fill` with a replace filter (pour: air and replaceable blocks
become a sludge source; drain: sludge states become air). After pouring, a fluid tick is
scheduled on the border blocks so the sludge spreads. The volume limit comes from the
configuration.

## R14. Performance targets

- **Painting** a 256×256×384 region is 64×64×96 = 393,216 cells. That's one palette `set` each,
  plus one packet per chunk. Well under 2 s (SC-005); a benchmark test asserts it.
- **Pouring** 64×64×16 is 65,536 blocks through the bulk filler, which is about 94 ms per million
  blocks, so roughly 6 ms.
- **Tint counters** cost one extra fluid lookup per `set_block_state`, already loaded through the
  state's ticking metadata.
