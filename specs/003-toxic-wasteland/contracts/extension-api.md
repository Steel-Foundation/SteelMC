# Contract: extension API additions

All additions are stabby-checked (constitution I). New bindings use new keys, and existing keys
keep their semantics.

## Content registration (pre-init, `Hooks::register_content_registration`)

```rust
impl ContentRegistry<'_> {
    /// Registers a fluid pair (source + flowing), its 16-state liquid block, its client
    /// projection and, optionally, its bucket item.
    fn register_fluid(&mut self, definition: FluidDefinition) -> ApiResult<RegisteredFluid>;
    /// Registers a biome that is sent to clients and saved in chunks by key.
    fn register_biome(&mut self, definition: BiomeDefinition) -> ApiResult<()>;
}
```

Errors: invalid or duplicate key, `minecraft` namespace, a tint on a lava-displayed fluid,
out-of-range numbers, an unknown sound or particle key, or registration after freeze.

## Behavior registration (`Hooks::register_block_registration`)

```rust
impl Registry<'_> {
    /// Installs gameplay hooks for a fluid registered by this extension.
    fn set_fluid_behavior(&mut self, fluid: Str<'_>, behavior: DynFluidBehavior) -> ApiResult<()>;
}
```

`DynFluidBehavior` methods run on the world tick thread, with borrowed, `!Send` handles:

| Hook | Called when | Return |
| --- | --- | --- |
| `entity_inside(world, pos, state, entity)` | An entity's bounding box is in the fluid (vanilla `entityInside` order) | — |
| `random_tick(world, pos, state, random)` | A random tick hits the fluid (when `randomly_ticking`) | — |
| `before_destroying_block(world, pos, replaced)` | The fluid flows into a replaceable block | `true` keeps the default drops |
| `react_with_neighbors(world, pos, state)` | On placement and neighbor change, before spreading | `false` means the fluid converted and must not spread |

## Runtime (any world callback)

```rust
impl World<'_> {
    fn biome_at(&self, pos: BlockPos) -> ApiString;
    /// Vanilla /fillbiome semantics over loaded chunks; returns changed cells.
    fn set_biomes(&self, from: BlockPos, to: BlockPos, biome: Str<'_>) -> ApiResult<u64>;
}

impl Entity<'_> {            // versioned module entity/v2
    fn is_living(&self) -> bool;
    fn is_creative_or_spectator(&self) -> bool;
    fn position(&self) -> Position;
    fn hurt(&self, damage_type: Str<'_>, amount: f32) -> ApiResult<bool>;
    fn add_effect(&self, effect: Effect) -> ApiResult<bool>;
}
```

`hurt` and `add_effect` return `Ok(false)` when vanilla rules refuse the action (invulnerability
timer, immunity, or a non-living entity). They return `Err` for an unknown damage type or effect
key.
