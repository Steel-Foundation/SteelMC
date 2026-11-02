# Data Model: Toxic Wasteland

## Platform: registry and core (`steel-registry`, `steel-core`)

### ClientFluid (new, `steel_registry::client_content`)

| Field | Type | Rule |
| --- | --- | --- |
| `source` | `FluidRef` | Custom source fluid. |
| `flowing` | `FluidRef` | Custom flowing fluid; `source.flowing_variant() == flowing`. |
| `block` | `BlockRef` | Liquid block family (16 `level` states). |
| `client_source` | `FluidRef` | `minecraft:water` or `minecraft:lava`. |
| `client_flowing` | `FluidRef` | The matching vanilla flowing fluid. |
| `tint` | `Option<FluidTint>` | Client water tint. Only valid when the client fluid is water. |
| `tint_index` | `Option<u16>` | Dense index among tinting fluids, used for variant ids. |

`ClientContent` gains `fluids: FxHashMap<usize, ClientFluid>`, keyed by both the source and
flowing fluid ids, and `vanilla_fluid_count`.

Projection: `client_fluid(fluid: FluidRef) -> FluidRef`. Vanilla fluids map to themselves.

### FluidTint

| Field | Type | Rule |
| --- | --- | --- |
| `water_color` | `i32` | 0x000000–0xFFFFFF |
| `water_fog_color` | `i32` | 0x000000–0xFFFFFF |

### ClientBiomes (new, `steel_registry::client_content`)

This is computed once at freeze from (the server biome registry × the tinting fluids). The server
never saves it or reads it during gameplay.

| Field | Type | Meaning |
| --- | --- | --- |
| `server_count` | `usize` | `REGISTRY.biomes.len()` |
| `variants` | `Vec<TintVariant>` | Ordered by `(tint_index, biome id)` |
| `client_count` | `usize` | `server_count + variants.len()` |

`TintVariant { key: Identifier, source: BiomeRef, tint: FluidTint }`.
`variant_id(biome_id, tint_index) = server_count + tint_index * server_count + biome_id`.

### Fluid (existing)

There are no new fields. Custom `Fluid` values are leaked statics, like custom blocks. Their
`block` and `bucket_item` keys point at the extension's entries, and their `source_fluid` and
`flowing_fluid` keys link the two variants. `Fluid::block` becomes authoritative for
`fluid_state_to_block`.

### ChunkSection (existing, extended)

| Field | Type | Rule |
| --- | --- | --- |
| `tint_cells` | `Option<Box<TintCells>>` | `None` until a tinting fluid state enters. Stays allocated afterwards; all-zero is valid. |

`TintCells` holds `counts: [[u16; 64]; N]`, one row per tinting fluid (N is small, normally 1). It
is stored sparsely as `SmallVec<(tint_index, [u16; 64])>`.

- **Cell index**: `(y >> 2) << 4 | (z >> 2) << 2 | (x >> 2)`, in vanilla quart order.
- **Invariant**: `counts[t][c]` equals the number of blocks in cell `c` whose fluid state is
  tinting fluid `t`. `recalculate_counts` rebuilds it, and a debug test checks it.
- **Transition**: a cell's dominant tint changes (including to and from none) → the section is
  marked biome-dirty.

### ChunkHolder (existing, extended)

`changed_biome_sections: SyncMutex<FixedBitSet>`, drained by `take_changed_biomes()` in the
broadcast loop.

### Biome (existing)

Custom biomes are ordinary `Biome` values registered before freeze. `features`, `carvers` and
`spawners` are empty for extension biomes in this iteration (generation has no decorators); a
`// TODO:` marks that.

## Platform: extension API (`steel-extension-api`, stabby)

### FluidDefinition (new)

| Field | Type | Default | Rule |
| --- | --- | --- | --- |
| `key` | `ApiString` | — | Owner namespace; the source fluid key. |
| `flowing_key` | `ApiString` | `<key>_flowing` | Distinct from `key`. |
| `block_key` | `ApiString` | `key` | The liquid block key. |
| `client` | `ClientFluidKind` | `Water` | Water or Lava; extendable (identifier-backed). |
| `tint` | `ApiOption<FluidTint>` | none | Requires `client = Water`. |
| `tick_delay` | `i32` | 5 | 1..=1200 |
| `drop_off` | `u8` | 1 | 1..=7 |
| `slope_find_distance` | `u8` | 4 | 0..=8 |
| `converts_to_source` | `bool` | false | — |
| `explosion_resistance` | `f32` | 100.0 | Finite, ≥ 0 |
| `randomly_ticking` | `bool` | false | — |
| `bucket` | `ApiOption<ItemDefinition>` | none | Registers the bucket item. |

`ContentRegistry::register_fluid(FluidDefinition) -> ApiResult<RegisteredFluid>`. The
`RegisteredFluid` value carries the `source_state` and the block key.

### DynFluidBehavior (new stabby trait)

Each hook gets borrowed handles (constitution I):
- `entity_inside(world, pos, state, entity)`
- `random_tick(world, pos, state, random)`
- `before_destroying_block(world, pos, replaced) -> bool` (`true` keeps the default drops)
- `react_with_neighbors(world, pos, state) -> bool` (`false` cancels spreading)

The hooks are installed during behavior registration with
`Registry::set_fluid_behavior(key, behavior)`.

### BiomeDefinition (new)

`key`, `has_precipitation`, `temperature`, `downfall`, and `effects: BiomeColors`:

- `fog_color`, `sky_color`, `water_color` and `water_fog_color` are required.
- `grass_color`, `foliage_color` and `dry_foliage_color` are optional.
- `grass_color_modifier`, `particle` (key and probability) and `music` (sound key) are optional.

### Entity handle (extended, versioned module `entity/v2`)

- `is_living() -> bool`
- `is_creative_or_spectator() -> bool`
- `hurt(damage_type: Str, amount: f32) -> ApiResult<bool>`
- `add_effect(effect: Effect) -> ApiResult<bool>`
- `position() -> Position`

### World (extended)

- `biome_at(pos) -> ApiString`
- `set_biomes(from: BlockPos, to: BlockPos, biome: Str) -> ApiResult<u64>` returns the number of
  changed cells.

## Extension: `toxic-wasteland`

### Config (`extensions/toxic-wasteland/config.toml`)

| Key | Type | Default |
| --- | --- | --- |
| `colors.water` / `colors.water_fog` | hex string | `"#4CBB17"` / `"#2E5E0E"` |
| `colors.sky` / `colors.fog` / `colors.grass` / `colors.foliage` | hex string | `"#9AA35A"` / `"#B4BA7A"` / `"#7C8A3A"` / `"#6B7A2E"` |
| `sludge.tick_delay` | int | 15 |
| `sludge.drop_off` | int | 2 |
| `sludge.converts_to_source` | bool | false |
| `effects.poison_seconds` | int | 5 |
| `effects.poison_amplifier` | int | 0 |
| `effects.contact_damage` | float | 1.0 |
| `effects.damage_interval_ticks` | int | 20 |
| `spread.corrupt_water_chance` | float (0–1 per random tick) | 0.25 |
| `spread.wither_ground_chance` | float | 0.15 |
| `world.lake_chance` | float (per chunk) | 0.12 |
| `world.base_height` / `world.relief` | int | 70 / 18 |
| `commands.max_volume` | int | 2,000,000 |

Every value is validated at startup. An error names the field, and the extension then fails to
load.

### Damage pacing

The interval is enforced per entity through vanilla's invulnerability timer plus a per-entity
last-hit tick map, keyed by entity UUID. Entries for entities that have left are pruned each
second.
