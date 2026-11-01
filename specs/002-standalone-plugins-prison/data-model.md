# Data Model

## Plugin discovery
- **PluginFile**: path, descriptor `{name, version, api_version, depends, soft_depends}`, config
  (TOML text), status `Loaded | Skipped(reason)`.
- Rules: names unique; a plugin is dropped if any hard dependency is missing or dropped; order is
  dependency-first, then file-name order.

## ItemSpec (API value)
`{ item: key, count: 1..=99, name: Option<json>, lore: Vec<json>, glint: Option<bool>,
unbreakable: bool, enchantments: Vec<(key, level)>, data: Vec<(key, value)> }`. Data keys are
namespaced by the calling plugin (`<plugin>:<key>`) and stored in `minecraft:custom_data` under
`steel_plugins`.

## Menu
`{ id (u64, unique per server), owner plugin, viewer PlayerId, title json, rows 1..=6, slots:
Vec<Option<ItemSpec>> }`. Click: `{ menu id, slot (-999 outside), button, click type }`.

## HUD
- **Sidebar** per player: `{ title json, lines: Vec<json> (≤15) }`; replacing lines diffs scores.
- **BossBar**: `{ uuid, title json, progress 0..=1, color (pink, blue, red, green, yellow, purple,
  white), style (progress, notched_6/10/12/20), viewers }`.

## FillRequest / FillReport
- Request: `{ shape: Cuboid{min,max} | Sphere{center, radius} | Shell{center, inner, outer} |
  Layer{min, max, y}, clip: Option<Cuboid>, pattern: Vec<(weight, BlockState)>, replace_air: bool,
  max_blocks }`.
- Report: `{ changed: u64, replaced: Vec<(BlockState, u64)>, micros: u64 }`.

## Prison
- **Mine** (`plugins/prison-mines/mines.toml`): `{ name [a-z0-9_-]{1,32}, world, min, max,
  mix: Vec<(weight, block)>, reset_seconds (≥10, default 300), reset_percent (1..=100, default
  80), mined (runtime), last_reset (runtime) }`; mines may not overlap.
- **Enchant** (config): `{ id, display, max_level, base_cost, cost_growth, chance_base,
  chance_per_level }`; levels live on the pickaxe as item data `prison-enchants:<id>`.
- **Player stats** (`plugins/prison-enchants/players.toml`): `{ uuid → blocks_mined }`.
- **Block values** (config): `{ block key → value }`, default for unlisted blocks.
