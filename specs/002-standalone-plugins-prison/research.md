# Research: Standalone Plugins, Full API Coverage and OP Prison Demo

Base: `another-plugin-experiment` after merging Steel-Foundation/SteelMC master (explosions/TNT,
vanilla hurt/death). Vanilla reference: `minecraft-src/` (26.3).

## R1. Drop-in loading

- **Decision**: At startup the server scans `<cwd>/plugins/` (non-recursive) for files ending in
  the platform library suffix (`.so`/`.dll`/`.dylib`), plus any explicit `[[plugins]]` paths.
  Each library is opened and its descriptor read; configuration comes from
  `plugins/<name>/config.toml` (created empty), overridden by an explicit `[plugins.config]`.
  Unopenable files, ABI-incompatible descriptors and duplicate names are skipped with warnings;
  plugins whose hard dependency is missing are dropped transitively. A failing pre-init or init
  still aborts startup, because native registrations cannot be rolled back.
- **Descriptor v4**: adds `api_version` (the `steel-plugin-api` package version the plugin was
  built with) so `/plugins` and diagnostics can show it. Key: `steel_plugin_v4`.
- **Rationale**: Mirrors Bukkit's `plugins/*.jar` + `plugins/<Name>/config.yml` layout.

## R2. Standalone projects

- **Decision**: Move the ports to `plugin-projects/` (own Cargo workspace, not a member of the
  server workspace) and add `plugin-template/` (own workspace). Both declare
  `steel-plugin-api = { git = "https://github.com/coco875/SteelMC", branch = "another-plugin-experiment" }`.
  `plugin-projects/Cargo.toml` (a manifest `[patch]`, since `.cargo/config.toml` is ignored for
  `--manifest-path` builds run from another directory) and the template's README show a `[patch."https://github.com/coco875/SteelMC"]`
  override to this checkout for local development; external authors simply omit it.
- **Rationale**: Cargo resolves the API from git like any library; the patch avoids pushing for
  local builds. Verified during implementation that the patched build does not need the network.

## R3. Event fire points (verified files)

| Event | Fire point |
|---|---|
| respawn | `player/lifecycle/respawn.rs` after the new position is chosen |
| world change | `player/lifecycle/world_transition.rs::change_world_within_domain` after success |
| game mode change | `player/game_mode/player_methods.rs::set_game_mode` |
| held slot | `player_inventory/player_handlers.rs` `SSetCarriedItem` handler |
| sneak/sprint | `movement/mod.rs` `SPlayerCommand` / player input handling |
| flight toggle | abilities packet handler |
| hand swap, item drop | `game_mode/block_interaction.rs` `PlayerAction::SwapItemWithOffhand` / `drop_from_selected` |
| item pickup | item entity `player_touch` path |
| entity interact | `game_mode/entity_interaction.rs::handle_interact` |
| inventory open/click/close | `player_inventory/player_handlers.rs` `open_menu_inner`, `handle_container_click`, close |
| sign change | `game_mode/block_interaction.rs::handle_sign_update` |
| experience / food | `player/experience.rs`, `player/food_data.rs` setters |
| entity spawn | `World::try_add_entity` for non-player entities |
| living death | `LivingEntity::die` |
| explosion | `world/explosion` block collection before destruction |
| piston | `behavior/blocks/redstone/piston/base.rs` before moving |
| chunk load/unload | chunk map full-chunk publication / unload |
| world load/unload | server world registration / temporary world retirement |
| weather | `world/weather.rs` state change |
| console command | server command queue for non-player senders |
| server list ping | `steel-login/src/handlers/status.rs` |
| plugin message | `handle_custom_payload` |

## R4. Items, menus, HUD

- **Item data**: `CUSTOM_DATA` (NBT compound) stores plugin data under
  `steel_plugins.<plugin>.<key>` (string values); `CUSTOM_NAME`, `LORE`,
  `ENCHANTMENT_GLINT_OVERRIDE`, `UNBREAKABLE`, `ENCHANTMENTS` for the rest. Plugins describe
  stacks with a plugin-side `ItemSpec` and receive the same form back.
- **Menus**: core `MenuBuilder` with `GENERIC_9X1..9X6` over a `SimpleContainer`; the plugin menu
  marks every slot read-only and routes clicks to the owning plugin callback with slot and click
  type (all moves cancelled, cursor and slots resynced).
- **Sidebar/boss bar**: add vanilla packets `CSetObjective`, `CSetDisplayObjective`, `CSetScore`,
  `CResetScore`, `CBossEvent` to steel-protocol (formats checked against vanilla 26.3 sources).
  Sidebars are per player (Paper-plugin style, not vanilla's global scoreboard; documented); boss
  bars follow vanilla `ServerBossEvent` semantics with an explicit viewer set.
- **Tab list**: per-player header/footer override that suppresses Steel's TPS footer for that
  player while set.

## R5. Bulk fill (Paper/FAWE approach)

- **Decision**: `World::fill_region(FillRequest)` in steel-core: for each loaded chunk and each
  section intersecting the clip box, take the section write lock once and write the chosen state
  for every position of the shape (cuboid, sphere, spherical shell, horizontal layer) — no
  `on_place`/neighbor/shape updates, no drops. Old block entities are detached normally.
  Heightmaps are recomputed per touched column, light changes are queued only where light
  properties differ (stone↔ore never relights; air↔stone does), and players tracking a chunk get
  either per-section `CSectionBlocksUpdate`s or a full chunk resend when most of it changed.
  Returns changed count, a histogram of replaced states, and elapsed time.
- **Rationale**: Fast prison mine plugins on Paper write `LevelChunkSection` palettes directly and
  resend chunks; FAWE additionally relights afterwards. Steel's per-block `set_block` locks a
  section, updates heightmaps and queues light per block, which dominates large resets.
- **Benchmark**: a `#[ignore]` core benchmark compares per-block `set_block(UPDATE_CLIENTS)` and the
  bulk path for 125k and 1M blocks; numbers recorded in quickstart.

## R6. Prison bundle

- `prison-api` (contract crate): `prison:mines/v1` service — `mine_at(world, pos)`, `info(name)`,
  `reset(server, name)`, `add_mined(name, count)`.
- `prison-mines` (plugin): mine storage (TOML), `/mine` commands, timed/threshold resets via the
  scheduler, players lifted above the mine, boss bar countdown, reset report (ms, blocks/s).
- `prison-enchants` (plugin, depends on prison-mines, soft-depends vault): pickaxe items with enchant
  levels in plugin item data, chest menu upgrades paid through `vault:economy/v1`, procs using the
  bulk fill clipped to the mine, auto-sell with Fortune, per-player sidebar, effects and sounds.
- Block drops are suppressed through a new `BlockBreakEvent::set_drop_items(false)` (vanilla
  Bukkit `setDropItems`).

## Benchmark (T019, 2026-10-31)

`cargo test --release -p steel-core --lib bulk_fill_benchmark -- --ignored --nocapture`
(9×9 loaded chunks, 50% diamond ore / 50% air mix over an existing stone volume):

| Blocks | Per-block `set_block` (clients only) | `World::fill_blocks` | Speed-up |
|--------|--------------------------------------|----------------------|----------|
| 125 000 | 72.6 ms (1.72 M blocks/s) | 11.3 ms (11.1 M blocks/s) | 6.4× |
| 1 000 000 | 641.8 ms (1.56 M blocks/s) | 94.2 ms (10.6 M blocks/s) | 6.8× |

Light propagation runs afterwards in the normal per-tick light pass and is not included.
