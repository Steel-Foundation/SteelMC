# Extension API

This reference describes the extension-facing API in `steel-extension-api`.
Start with [Create an extension](getting-started.md) for a complete crate and
installation instructions.

## Imports and generated API documentation

Common entry points are re-exported at the crate root:

```rust
use steel_extension_api::{ApiResult, BlockPos, BlockState, Hooks, Registry, World};
use steel_extension_api::stabby;
```

Other public modules are `hooks`, `runtime`, `block_behavior`,
`block_behavior::types`, `block_behavior::capabilities`, and
`block_behavior::block_entity`. Methods on the opaque handles are generated from
the binding declarations. Generate full Rust documentation, including complete
method signatures, from the Steel checkout:

```sh
cargo doc -p steel-extension-api --no-deps
```

The output starts at `target/doc/steel_extension_api/index.html`. The optional
`host` module is server-side loading and adaptation code; extension crates use
the default features.

## Boundary types, errors, and ownership

| Type | Use |
| --- | --- |
| `ApiResult<T>` | ABI-stable result with an owned Stabby string error. |
| `stabby::str::Str<'a>` | Borrowed string; use `"text".into()` or `Str::new(...)`. |
| `stabby::string::String` | Owned string that can cross the boundary. |
| `stabby::slice::Slice<'a, T>` | Borrowed ABI-stable slice. |
| `stabby::vec::Vec<T>` | Owned ABI-stable sequence. |
| `stabby::option::Option<T>` | ABI-stable optional value. |
| `BlockPos` | Public integer `x`, `y`, `z` block coordinates. |
| `Position` | Public `f64` `x`, `y`, `z` world coordinates. |
| `BlockState` | Copyable state resolved by Steel; its numeric ID is private. |
| `BlockChange` | A `pos: BlockPos` and `state: BlockState` terrain write. |

Use ordinary Rust types inside extension-owned state. Boundary arguments and
results must use ABI-stable types. Define custom payload layouts with
`#[stabby::stabby]`; use `#[stabby::export(callback)]` on callbacks and extension
trait implementations.

Convert fallible host calls to ordinary Rust results to use `?`:

```rust
use steel_extension_api::{ApiResult, BlockState, default_block_state, stabby};
use stabby::string::String as ApiString;

fn resolve_stone() -> ApiResult<BlockState> {
    let result = (|| -> Result<BlockState, ApiString> {
        let state = default_block_state("minecraft:stone".into()).match_owned(Ok, Err)?;
        Ok(state)
    })();
    result.into()
}
```

Borrowed host handles such as `World<'_>`, `Server<'_>`, `Command<'_>`, and
`Registry<'_>` remain valid only for their borrow. They cannot escape into static
state or move to another thread. Owned host values use paired host destructors;
moving an `ItemStack` into a host call consumes that handle. Store extension-owned
configuration and resolved copyable values separately from callback handles.
Callbacks must not panic across the native boundary or block tick threads while
waiting for I/O or background work.

## Lifecycle and subscriptions

Declare both entry points, in the short form (version `0.0.0`, no dependencies)
or the long form:

```rust
steel_extension_api::export_extension!("example", pre_init, init);

steel_extension_api::export_extension! {
    name: "example",
    version: "1.0.0",
    depends: ["worldedit"],   // must be present; initialized first
    soft_depends: ["vault"],  // initialized first when present
    pre_init: pre_init,
    init: init,
}
```

`pre_init` has the Rust signature `fn(Hooks<'_>) -> R` and `init` has
`fn(ExtensionContext<'_>) -> R`, where `R` is `ApiResult<()>` or any
`Result<(), E: Display>`. `ExtensionContext` gives `name()`, `version()`,
`config()` and `data_directory()`, an absolute path to `extensions/<name>/`
created before init.

| Stage | Available work |
| --- | --- |
| Pre-init | Subscribe through `Hooks`. Registry lookups and action/filter emission are unavailable. |
| Registration | Block, generator, and command callbacks edit their actual registries before publication. |
| Init | All registration events succeeded and registries are frozen. Parse configuration and resolve static values. |
| Runtime | Commands, ticks, and explicitly emitted actions/filters call the installed handlers. |

The configuration returned by `ExtensionContext::config()` is the TOML
serialization of that extension's `[extensions.config]` table or, without one,
the contents of `extensions/<name>/config.toml`. A block registration callback
can access the same document through `Registry::config()`.

Steel opens every configured and discovered package or library before running any
extension code. Files that are not extensions, were built for an older API,
declare a name another file also declares, or miss a hard dependency are skipped
with a warning; the rest are pre-initialized and initialized in dependency order.
A dependency cycle or a lifecycle failure stops startup.

`Hooks` provides these subscriptions:

| Method | Callback signature | When it runs |
| --- | --- | --- |
| `register_block_registration` | `extern "C" fn(Registry<'_>) -> ApiResult<()>` | During block behavior registry initialization. |
| `register_generator_registration` | `extern "C" fn(GeneratorRegistry<'_>) -> ApiResult<()>` | During generator registry initialization. |
| `register_commands` | `extern "C" fn(Commands<'_>) -> ApiResult<()>` | Before the command dispatcher freezes. |
| `register_world_tick` | `extern "C" fn(World<'_>, u64)` | At the end of each normal world tick, on that world's thread. |
| `register_server_tick` | `extern "C" fn(Server<'_>, u64)` | On each normal server tick before world workers run. |
| `add_action::<A>` | `extern "C" fn(&A)` | When code emits the typed action. Returns an ordering builder. |
| `add_filter::<F>` | `extern "C" fn(&mut F)` | When code applies the typed filter. Returns an ordering builder. |

`Hooks` also has closure helpers that need no exported callback: `on_commands`
for typed command trees, `on_*`/`on_*_with` for every gameplay event (join,
chat, block break, damage, inventory clicks, explosions and about sixty more),
`enable_scheduler` for `scheduler::run_later`/`run_repeating`, and
`on_shutdown`. The [API crate README](../../steel-extension-api/README.md#events)
lists them.

The three registration subscriptions return `ApiResult<()>`; the two direct tick
subscriptions return `()`. Frozen ticks omit these gameplay callbacks.
Registration failures prevent extension init and world creation. There is no
rollback: earlier registry edits remain in the startup process that is failing.

## Actions, filters, and ordering

An action implements `hooks::Action`; a filter implements `hooks::Filter`. Both
traits require `stabby::IStable + 'static` and an owner-scoped `TAG`, for example
`"example:score/v1"`. The `'static` payload type bound does not require the value
passed to a dispatch call to live forever; dispatch borrows that value for the
synchronous call.

```rust
use steel_extension_api::{ApiResult, Hooks, hooks::{Filter, apply_filters}, stabby};

#[stabby::stabby]
struct Score { value: i32 }

impl Filter for Score {
    const TAG: &'static str = "example:score/v1";
}

#[stabby::export(callback)]
fn bonus(score: &mut Score) {
    score.value += 10;
}

fn register_bonus(mut hooks: Hooks<'_>) -> ApiResult<()> {
    hooks.add_filter::<Score>(bonus).after("base-scores").register()
}

fn apply_bonus(score: &mut Score) -> ApiResult<()> {
    apply_filters(score)
}
```

`hooks::do_action(&payload)` shares a payload. `hooks::apply_filters(&mut payload)`
passes exclusive reborrows through the filter chain. Both return `ApiResult<()>`
and succeed without side effects when no handlers are registered.

Builders support `.before("descriptor-name")`, `.after("descriptor-name")`, and
`.register()`. Constraints apply within one action or filter tag. An extension's
constraints are combined for that event, and its handlers run together in
declaration order. Eligible extensions use first registration order as the
tie-breaker. Missing targets impose no constraint; duplicate constraints are
harmless. Cycles, including self-ordering, fail startup.

Ordering builders apply to `add_action` and `add_filter`. Direct block, command,
generator, and tick subscription methods do not expose these builders.

Payload ABI reports must match both at subscription and dispatch. Extensions
sharing an event should depend on a common crate defining its payload type and
tag. Use a new tag or Stabby invariant version when semantics change, even if the
physical layout stays compatible. Dispatch releases registry locks before
callbacks, so handlers may emit nested events.

| Built-in action | Fields | Host emission |
| --- | --- | --- |
| `hooks::ServerTickAction` | `tick_count: u64` | Each normal server tick, before world workers. |
| `hooks::ServerStopAction` | None | Once before orderly shutdown saves. |

`ServerTickAction` is a notification containing the tick count. Use
`register_server_tick` when the callback needs a borrowed `Server` handle.

The runnable [actions/filters example](../../extension-example/hooks-and-filters/README.md)
uses a shared payload contract, two separately loaded libraries, and a command
that emits both a filter and an action. Its `before`/`after` constraints produce
`25` from an initial score of `10`, even when the libraries load in the opposite
order. It also subscribes to server tick and stop actions.

## Registry lookup and world access

Root lookup functions:

| Function | Result |
| --- | --- |
| `default_block_state(key: Str<'_>)` | `ApiResult<BlockState>` for a registered block's default state. |
| `blocks_in_tag(key: Str<'_>)` | `ApiResult<stabby::vec::Vec<stabby::string::String>>` of block keys. |

Lookups are valid after the data registry initializes, including block
registration and extension init. Resolve known states once and retain those
copyable values rather than inventing numeric state IDs.

`World<'_>` methods:

| Method | Result and behavior |
| --- | --- |
| `key()` | Owned world key such as `minecraft:overworld`. |
| `is_chunk_loaded(chunk_x, chunk_z)` | Checks full-chunk readiness without requesting generation. |
| `block(pos)` | `ApiResult<BlockState>`; unavailable chunks and invalid positions are errors. |
| `set_block(pos, state)` | `ApiResult<()>`; uses normal gameplay block updates. |
| `pop_resource(pos, stack)` | `ApiResult<bool>`; consumes the stack and uses normal block-drop rules and pickup delay. |

`pop_resource` returns false for an empty stack or disabled block drops. Reads
and writes do not implicitly load missing chunks. `World` is borrowed per
callback; retain its key or your own state between ticks.

## Block behaviors and capabilities

The public contract is `block_behavior::BlockBehavior`; its ABI-safe owning type
is `block_behavior::DynBlockBehavior`. Register a complete replacement during a
block registration callback with `Registry::replace_block_behavior(key, behavior)`.
Capture the previous implementation before replacing it:

```rust
#![recursion_limit = "512"]
use steel_extension_api::{ApiResult, Hooks, Registry, stabby};
use steel_extension_api::block_behavior::{BlockBehavior, OriginalBlockBehavior};
use stabby::string::String as ApiString;

struct DelegatingBehavior { original: OriginalBlockBehavior }

#[stabby::export(callback)]
impl BlockBehavior for DelegatingBehavior {
    fn original(&self) -> OriginalBlockBehavior { self.original }
}

#[stabby::export(callback)]
fn register_blocks(mut registry: Registry<'_>) -> ApiResult<()> {
    let result = (|| -> Result<(), ApiString> {
        let original = OriginalBlockBehavior::get(&registry, "minecraft:white_wool".into())
            .match_owned(Ok, Err)?;
        registry.replace_block_behavior(
            "minecraft:white_wool".into(),
            stabby::boxed::Box::new(DelegatingBehavior { original }).into(),
        ).match_owned(Ok, Err)
    })();
    result.into()
}

fn pre_init(mut hooks: Hooks<'_>) -> ApiResult<()> {
    hooks.register_block_registration(register_blocks)
}
```

This example deliberately delegates all behavior. Override selected methods in
the trait implementation to change it. `original()` is required; omitted methods
call that captured implementation. Captures can refer to an earlier extension's
replacement and remain valid after later registry edits. Replacements are global
and immutable after startup.

Delegation invokes the captured receiver. Its internal calls therefore use its
own methods. For example, replacing a collision shape does not change an original
collision-box method that you still delegate; override both when needed.

The contract covers placement, interaction, neighbor updates, redstone, collision
and movement, scheduled/random ticks, projectiles, explosions, drops, fluids,
block entities, and capability queries. See the
[complete method declaration](../../steel-extension-api/src/block_behavior/schema.rs)
for every method's arguments and result. Public payloads live in
`block_behavior::types`; many are opaque host-owned values with a deliberately
limited method surface.

| Capability trait in `block_behavior::capabilities` | Registry factory | Behavior accessor |
| --- | --- | --- |
| `Bonemealable` | `create_bonemealable` | `as_bonemealable` |
| `Fallable` | `create_fallable` | `as_fallable` |
| `GrowingPlantHeadBehavior` | `create_growing_plant_head` | `as_growing_plant_head` |
| `RailBehavior` | `create_rail` | `as_rail` |

Factories accept the corresponding `Dyn*` implementation and return a registered
handle. Call its `borrow()` method from the behavior accessor. An accessor can
delegate, return its own registered capability, or return
`stabby::option::Option::None()` to remove that capability. Implementations require
`Send + Sync` and remain pinned for the process lifetime.

Selected value helpers:

| Type | Public operations |
| --- | --- |
| `types::ItemStack` | `from_block(state)` creates one host-resolved block item. |
| `types::Inventory<'_>` | `held_item_is(key)` and fallible `damage_item(amount, infinite_materials)`. |
| `types::Player<'_>` | `has_infinite_materials()`. This is a borrowed behavior context, distinct from `PlayerId`. |
| `types::Explosion<'_>` | World, damage source, block interaction, credited/direct entities, radius, center, and block-like-entity policy getters. |
| `types::ExplosionDropConsumer<'_>` | `accept(stack, pos)` transfers a drop to the host collector. |
| `types::BlockInteraction` | `Keep`, `Destroy`, `DestroyWithDecay`. |

`block_behavior::InteractionResult` preserves all interaction outcomes:
`Success`, `SuccessWithoutItem`, `SuccessServer`, `Consume`, `Fail`, `Pass`, and
`TryEmptyHandInteraction`. `MovementResult` carries a host-owned velocity value
and a `bounced` flag.

## Block entities and persistence

Implement `block_behavior::block_entity::BlockEntityBehavior` with
`#[stabby::export(callback)]`. Its `load_additional(BorrowedNbt<'_>)` and
`save_additional(NbtMut<'_>)` callbacks are required. Optional callbacks cover
state/removal changes, block events, pre-removal effects, ticking, update tags,
containers, game-event listeners, and item components.

| Type in `block_behavior::types` | Operations |
| --- | --- |
| `BlockEntity` | `new(key, level, pos, state, implementation)` for an existing registered entity type; `share()`, `borrow()`, `with_behavior(implementation)`. |
| `BlockEntityCreation` | `created(entity)`, `no_entity()`, `into_created()`. |
| `BlockEntityTicker` | `for_entity_tick(key)` resolves a compatible registered entity ticker. |
| `BorrowedNbt<'_>` | `int(key)` reads an optional integer. |
| `NbtMut<'_>` | `put_int(key, value)` writes an integer. |
| `Nbt` | `new()` or `default()`, then `put_int(key, value)`. |

`BlockEntity::new` checks that the registered entity type accepts the supplied
block state. The API does not register new block entity types. To wrap an
existing entity, capture its original factory result, extract it with
`into_created()`, retain a shared handle, and use `with_behavior(...)` for the
replacement. Delegate load/save explicitly when original serialization must be
preserved. Steel owns lifecycle orchestration, components, and persistence.

The [native fixture](../../steel-extension-api/tests/fixture/src/lib.rs) includes
a comparator wrapper that saves its own counter while preserving original data.

## Terrain generators

In a generator registration callback, call:

```text
GeneratorRegistry::register_terrain_generator(
    key, dimension_type, biome, sea_level, callback
) -> ApiResult<()>

callback: extern "C" fn(Terrain<'_>)
```

Keys are `Str<'_>`; `sea_level` is `i32`. The host resolves the registered
dimension type and biome. These generators have a uniform biome and fill terrain
only; they do not run structures or decorators and accept no per-world generator
configuration. Choose the registered generator key in `worlds.toml` or use it for
a temporary world.

| `Terrain<'_>` method | Use |
| --- | --- |
| `min_x()`, `min_z()` | Current chunk's minimum world X/Z. |
| `min_y()`, `height()` | Dimension build bounds. |
| `random_at(salt: u64, pos: BlockPos)` | Seeded positional vanilla xoroshiro sampling returning `u64`. |
| `write_blocks(Slice<'_, BlockChange>)` | Validates the entire batch before writing through the terrain phase capability. |

The persisted world seed drives positional sampling. Do not substitute an
unseeded RNG for repeatable terrain. Restrict writes to the generation capability
and use host-resolved block states. Terrain callbacks have no result channel;
handle fallible writes without panicking. See the
[terrain example](../../extension-example/strange-terrain/src/terrain.rs).

## Commands

`Commands::register_player_command(key, actions, callback) -> ApiResult<()>`
registers one namespaced command root with literal subcommands. `key` is
`Str<'_>`, `actions` is `Slice<'_, Str<'_>>`, and the callback is:

```text
extern "C" fn(Command<'_>, Str<'_>) -> ApiResult<i32>
```

The callback receives the selected literal action. The action list must be
nonempty and contain distinct nonempty literals without whitespace. Commands
require a player source, permit access by default, and respect explicit
permission denials. A key such as `example:arena` uses `example.command.arena`.
The host's normal command registration detects namespace and root collisions.
For argument trees, console sources, aliases, per-node permissions and
suggestions, use `command::CommandBuilder` through `Hooks::on_commands`; see the
[API crate README](../../steel-extension-api/README.md#commands-players-and-text).

| `Command<'_>` method | Use |
| --- | --- |
| `server()` | Borrow the owning `Server`. |
| `player()` | Resolve `ApiResult<PlayerId>` for the execution player. |
| `reply(message)` | Send plain success feedback to the command sender. |

## Server, players, temporary worlds, and bots

Borrow `Server<'_>` in a direct server-tick callback or through
`Command::server()`. Runtime identities such as `PlayerId` and `Operation` are
copyable opaque values, not borrowed host objects.

| `Server<'_>` method | Result and constraints |
| --- | --- |
| `players()` | Owned `Vec<PlayerInfo>` snapshot of connected resident players, including bots. |
| `is_connected(player)` | Checks UUID and actor ID for the same connected incarnation, including transfers. |
| `player_ready(player)` | Checks relocation and client loading completion. |
| `switch_domain(player, domain)` | Fallible queued domain switch through normal player save/restore. |
| `select_world(player, key)` | Fallible queued selection of a loaded world. |
| `teleport(player, position)` | Fallible current-world teleport; position must be finite and the destination loaded. |
| `set_game_mode(player, mode)` | Fallible game-mode update. |
| `clear_inventory(player)` | Fallible clearing of the current domain inventory. |
| `equip_item(player, key)` | Fallible equipping of one registered item in the selected hand. |
| `message(player, text)` | Fallible plain system-chat message. |
| `block(world_key, pos)` | Fallible loaded-world block read, without generation. |
| `create_temporary_world(key, generator, settings, primary)` | `ApiResult<Operation>` for world preparation and publication. |
| `destroy_temporary_domain(domain)` | `ApiResult<Operation>`; rejects new entries and retires worlds after players and relocation work leave. |
| `operation_status(operation)` | `ApiResult<OperationStatus>`; consumes a completed result. |
| `domain_is_temporary(domain)` | Whether the domain uses temporary player-state routing. |
| `spawn_bot(world_key, name, position)` | `ApiResult<PlayerId>`; requires a prepared temporary world. |
| `despawn_bot(player)` | Fallible removal through normal disconnect handling. |
| `control_bot(player, input)` | Fallible intent for the bot's next actor tick. |

`PlayerInfo` contains `id`, owned `name` and `world` strings, `position`, `bot`,
and `health`. `PlayerId` includes a UUID and actor ID internally; an old identity
does not select a reconnecting player's new incarnation.

`WorldSettings` supplies `seed: i64`, `spawn: BlockPos`, `default_game_mode`, and
`difficulty`. `GameMode` has `Survival`, `Creative`, `Adventure`, and `Spectator`;
`Difficulty` has `Peaceful`, `Easy`, `Normal`, and `Hard`. A primary temporary
world creates an isolated domain; further worlds share its clock. World and
player storage for temporary domains stays in memory.

Keep the returned `Operation` in extension-owned state. On later server ticks,
call `operation_status` once to inspect it, then return to the tick loop when it
is pending. `OperationState` is `Pending`, `Ready`, or `Failed`.
`OperationStatus::value` is empty while pending, contains a key on success, or a
diagnostic on failure. Ready and failed results are consumed; querying that
operation again returns an error. Do not sleep or spin while waiting for work.

Before retiring a temporary domain, relocate its players back to a lasting
domain and track the destruction operation to completion. Normal domain switching
restores player state, and reconnects retain the last lasting domain. See the
[Spleef example](../../extension-example/spleef/README.md) for the full flow.

`BotInput` contains `yaw`, `pitch`, `movement: PlayerControls`, and an optional
`dig: BlockPos`. `PlayerControls` has `forward`, `backward`, `left`, `right`,
`jump`, `sneak`, and `sprint` booleans. Controls expire after one actor tick;
reapply active intent on subsequent ticks. Movement, reach, gravity, and digging
use the host's regular implementations.

## Gameplay API

Beyond the boundary described above, `steel-extension-api` exposes a gameplay
layer built on the same checked bindings. The
[API crate README](../../steel-extension-api/README.md) documents it in detail:

| Module | Contents |
| --- | --- |
| `events` | Cancellable and observable events for players, blocks, inventories, entities, worlds and the server. |
| `command` | Typed command trees with arguments, aliases, permissions and suggestions. |
| `players` | Lookup, teleport, health, food, flight, game mode, messages, titles, permissions. |
| `items` | `ItemSpec` stacks with names, lore, enchantments and private data; inventory slots. |
| `menus` | Read-only chest GUIs with click handlers. |
| `hud` | Per-player sidebar, boss bars and tab list. |
| `effects` | Particles, sounds, potion effects, experience and speeds. |
| `worlds` | `Server::with_world`, time, weather, item drops, entities, plugin messages. |
| `fill` | Section-batched region fills (about ten million blocks per second). |
| `services` | Versioned provider objects shared between extensions (see `extension-example/vault-api`). |
| `scheduler`, `text`, `log` | Tick tasks, text components with `&` codes, tagged logging. |

The ports in `extension-example` (Vault, Essentials, WorldEdit, WorldGuard) and
the OP Prison extensions use only this public API.

## Scope and source map

The combination takes checked ABI bindings, opaque handles, lifecycle stages,
and runtime capabilities from `another-plugin-experiment`, custom actions,
filters, handler ordering, and archive packaging from `try-plugin`, and the
gameplay API, dependency-aware loader and ports from the plugin platform
iterations documented in `specs/`. It adapts
those ideas to the current Steel source; it does not port every API from both
branches. In particular, `try-plugin`'s item behavior registration is not exposed
by the current extension API.

The API supports replacement of existing block behaviors and custom terrain
generators. It does not currently expose new block/item registry entries, item
behavior registration, new block entity types, runtime datapack loading, or
extension unloading/reloading.

| Area | Source |
| --- | --- |
| Entry points, shared values, re-exports | [lib.rs](../../steel-extension-api/src/lib.rs) |
| Registry, world, terrain bindings | [bindings.rs](../../steel-extension-api/src/bindings.rs) |
| Actions, filters, ordering builders | [hooks](../../steel-extension-api/src/hooks/mod.rs) |
| Complete block callback contract | [schema.rs](../../steel-extension-api/src/block_behavior/schema.rs) |
| Capability traits | [capabilities.rs](../../steel-extension-api/src/block_behavior/capabilities.rs) |
| Block entity callbacks | [block_entity](../../steel-extension-api/src/block_behavior/block_entity/mod.rs) |
| Commands and server operations | [runtime bindings](../../steel-extension-api/src/runtime/bindings.rs) |
| Player, world, and operation value types | [runtime types](../../steel-extension-api/src/runtime/types.rs) |

For changes to the native binding machinery itself, see the
[API crate's implementation notes](../../steel-extension-api/README.md#define-bindings).
