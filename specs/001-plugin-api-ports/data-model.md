# Data Model

## Core / API entities

### EventPriority
`Lowest < Low < Normal < High < Highest < Monitor`. Monitor handlers observe the final outcome;
setters called from a monitor handler are ignored (host checks the handler's priority).

### Event subscription
`{ priority, ignore_cancelled: bool, handler }`. Handlers with equal priority run in subscription
order (plugin load order, then call order).

### Events (mutable fields in **bold**)
| Event | Read | Mutable | Cancellable |
|---|---|---|---|
| PreLogin | uuid, name | **deny reason** | yes (deny) |
| Join | player | **message** (Option) | no |
| Quit | player | **message** (Option) | no |
| Chat | player | **message**, **rendered override** | yes |
| CommandPreprocess | player, command line | **command line** | yes |
| BlockBreak | player, world, pos, state | **drop experience/items flag** | yes |
| BlockPlace | player, world, pos, placed state, against pos | — | yes |
| Interact | player, world, action (left/right × block/air), hand, pos?, face?, held item key | — | yes |
| Damage | victim entity info, attacker info?, cause key, world | **amount** | yes |
| Death | player, world, position | **message** (Option) | no |
| Move | player, world, from, to | — | yes |
| Teleport | player, from world+pos, to world+pos+rotation, cause | **destination** | yes |

### EntityInfo
`{ id: i32, kind: Identifier string, player: Option<PlayerId>, position }`.

### CommandNodeSpec
`{ parent: u32 (u32::MAX = root), kind: Literal | Argument, name, argument: ArgumentSpec,
executes: bool, permission: Option<String>, suggest: bool }`.
`ArgumentSpec` = kind identifier (`brigadier:integer`, `steel:players`, …) + `min`/`max` bounds.
Validation: first node is the root literal; parents precede children; names unique among
siblings; argument names unique along a path.

### Service entry
`{ key: "ns:name/vN", provider: plugin name, priority: i32, report, object }`. Lookup returns
highest priority; ties by registration order. Report incompatibility → "incompatible provider"
error, never a cast.

### Plugin
`{ name, version, depends[], soft_depends[], config (TOML), data_directory, state:
Loaded → PreInitialized → Registered → Initialized → ShutDown }`. Missing hard dependency or
cycle → startup error before any pre-init.

## Port entities

### Vault
- **Economy contract** (`vault:economy/v1`): name, currency names, fractional digits, has
  account, balance, has, withdraw, deposit, create account (by player UUID + name).
  `EconomyResponse { amount, balance, kind: Success|Failure|NotImplemented, error }`.
- **Permission contract** (`vault:permission/v1`): has(player, node), groups(player),
  primary group, in group, group list. Mutations return NotImplemented.
- **Chat contract** (`vault:chat/v1`): prefix, suffix, metadata string/int lookup.

### Essentials
- **User** (`userdata/<uuid>.toml`): last name, nickname?, money (i64 cents), homes
  `{name → Location}`, last location (for `/back`), kit cooldowns `{kit → unix seconds}`,
  god, ignore none. Location = `{world, x, y, z, yaw, pitch}`.
- **Warps** (`warps.toml`): `{name → Location}`.
- **Spawn** (`spawn.toml`): Location.
- **Runtime only**: pending teleport requests `{target → (requester, kind, created_tick)}`,
  reply targets, AFK state + last activity tick.
- Validation: names `[a-z0-9_-]{1,32}`, money ≥ 0 and ≤ configured max, amounts finite with ≤2
  decimals.

### WorldEdit
- **Session** (per player, runtime): pos1, pos2 (world + BlockPos), clipboard
  `{origin offset, blocks: Vec<(offset, state)>}`, history `Vec<ChangeSet>` (max 15 by config),
  redo stack.
- **ChangeSet**: world, `Vec<(pos, before, after)>`. Undo applies `before` in reverse order.
- **Pattern**: weighted list of block states. **Mask**: set of block states or block keys
  (any properties).

### WorldGuard
- **Region** (`regions/<world>.toml`): id `[a-z0-9_-]+` unique per world, min/max BlockPos,
  priority i32, parent id?, owners/members (UUID sets), flags `{flag → value}`.
- **Global region** `__global__`: flags only, priority lowest, applies everywhere in world.
- **Flag values**: state flags (`allow`/`deny`), string flags (greeting, farewell).
- **Resolution**: collect regions containing the point; for each flag take the value from the
  highest-priority region that sets it (directly or via parent chain); members/owners of a
  region bypass `build`-group flags of that region; permission `worldguard.region.bypass.<world>`
  bypasses everything. Unset `build` defaults to deny for non-members inside any region (vanilla
  WorldGuard behavior), allow outside regions.
