# Steel extension API

For extension authors, start with [Create an extension](../doc/extensions/getting-started.md)
and the [API reference](../doc/extensions/api.md). This README also covers the
native binding implementation and its validation commands.

One crate defines both sides of the native extension boundary. Extensions use the default
features; the server enables `host`, which adds the optional Steel implementation
dependencies. Build the server and extension libraries in separate Cargo invocations.

The host passes one bootstrap pointer to the extension pre-init entrypoint. The generated
binding callback validates every required function key and complete stabby
signature report before publishing the cached pointers. Calls then go directly
through those pointers. The extension never links against the Steel executable's
symbols. Libraries remain loaded for the process lifetime, including when startup
fails after loading; unload and reload are unsupported.

## Load and package extensions

Steel takes explicitly configured `[[extensions]]` paths first, then every remaining `*.zip`
package and native library in `./extensions` (created on first start) in filename order.
Configured paths may select a raw native library or a ZIP. Every source is opened and its
descriptor checked before any extension code runs: files that are not extensions, were built
for an older API, declare a name another file also declares, or miss a hard dependency
(transitively) are skipped with a warning. The rest are ordered so dependencies come first
and pre-initialized in that order. A dependency cycle, a pre-init, registration or init
failure, or loading one name twice in a process stops startup before worlds begin ticking,
because native registrations cannot be rolled back.

Each archive contains `extension.toml` and its platform libraries:

```toml
[package]
name = "my-extension"
version = "0.1.0"

[targets]
linux-x86_64 = "libmy_extension.so"
macos-aarch64 = "libmy_extension.dylib"
windows-x86_64 = "my_extension.dll"
```

The package name must equal the name passed to `export_extension!`. Target keys
use Rust's operating system and architecture names. Only the selected library is
extracted; absolute paths, parent traversal, and symlink members are rejected.
Libraries and their extraction directories remain pinned for the process lifetime,
including after native initialization fails. There is no unloading or hot reload.

Build and package an isolated extension using Python 3.11 or newer:

```sh
python3 package-extension.py extension-example/wool-shearing --debug
```

This writes `extensions/steel-wool-shearing-extension.zip` with only the target actually
built. `--target` selects a Rust cross-compilation target; `--output` changes the
archive destination. The builder invokes Cargo separately from the host, preventing
accidental host-feature unification. Wool shearing needs no configuration. The
[example index](../extension-example/README.MD) links separate examples for block
behavior, Game of Life, terrain generation, and Spleef, each with build and
configuration instructions.

## Ordered actions and filters

Actions borrow an ABI-stable payload shared; filters borrow one exclusively and
apply handlers sequentially. Register during pre-init through `Hooks`, then emit
with `hooks::do_action` or `hooks::apply_filters` after registration freezes.
Extension authors use safe APIs; pointer erasure and restoration stay inside the
boundary implementation.

```rust
use steel_extension_api::{ApiResult, Hooks, hooks::{Filter, apply_filters}, stabby};

#[stabby::stabby]
struct Score { value: i32 }
impl Filter for Score { const TAG: &'static str = "example:score/v1"; }

#[stabby::export(callback)]
fn bonus(score: &mut Score) { score.value += 10; }

fn pre_init(mut hooks: Hooks<'_>) -> ApiResult<()> {
    hooks.add_filter::<Score>(bonus).after("base-scores").register()
}

fn resolve_score() -> ApiResult<()> {
    let mut score = Score { value: 0 };
    apply_filters(&mut score)
}
```

Constraints use descriptor names, apply within each event, and address all handlers
owned by the named extension. Absent targets impose no constraint. Eligible extensions use their first registration order as the tie-breaker; each
extension's handlers retain declaration order. Duplicate constraints are harmless; cycles,
including self-ordering, fail initialization rather than choosing an arbitrary
order. Payload reports are checked both at subscription and dispatch, before any
handler sees an incompatible value. No-handler events succeed without mutation.
The defining extension owns each tag; semantic changes require a versioned tag or
Stabby invariant version even when the physical layout stays the same.

Dispatch never holds a registry lock across extension callbacks, so nested events
are supported. Subscribing after freeze or emitting during pre-init returns an
error. `hooks::ServerTickAction` runs before world workers on each normal server
tick; frozen ticks omit it. `hooks::ServerStopAction` runs once before orderly
shutdown saves. Like other native callbacks, handlers must not panic or retain
borrowed payloads.

The design combines the checked bindings and lifecycle from
[`another-plugin-experiment`](https://github.com/coco875/SteelMC/tree/another-plugin-experiment/steel-plugin-api)
with the actions, filters, ordering, and archive packaging from
[`try-plugin`](https://github.com/coco875/SteelMC/tree/try-plugin). The gameplay API (events,
commands, players, items, menus, HUD, effects, worlds, bulk fill, services, scheduler), the
dependency-aware loader and the ports in `extension-example/` come from the plugin platform
iterations in [OPPrisonSteelMCExperiment](https://github.com/tangjuyo/OPPrisonSteelMCExperiment),
whose `specs/` are kept in this repository.

See the runnable [actions/filters example](../extension-example/hooks-and-filters/README.md)
for two native libraries using a shared payload contract, real cross-extension
ordering, and observable action callbacks. The current API does not port every
feature from the experiments; item behavior registration is not exposed.

## Define bindings

`#[stabby::host_api]` on an inline module collects public functions and public
inherent methods. The annotated crate's `host` feature retains their original
bodies. Without that feature, the macro removes those bodies and generates
forwarders. Private helpers remain host-only. Use `#[cfg(feature = "host")]` on
imports that name host dependencies.

Inside the module, `#[opaque] pub struct World<'a>(&'a HostWorld);` declares a
borrowed host object. The macro replaces its backing type with a private pointer
and lifetime marker in both modes. Host-only `from_host`, `host`, and `host_mut`
accessors control conversion and reborrowing. Extensions cannot construct handles,
inspect Steel layouts, or move the borrowed handles to another thread.
Handle lifetimes are invariant, preserving any exclusive borrows inside the hidden
host type.
Exclusive handles contain a zero-sized non-copyable token. Opaque declarations
accept documentation and `must_use` attributes; field-access derives are rejected
to keep the private representation inaccessible to generated user code.
The handle and token use transparent representations to preserve the pointer's
layout and calling convention. See the [Rust representation rules](https://doc.rust-lang.org/reference/type-layout.html#the-transparent-representation).

The initial macro supports synchronous, safe functions and concrete inherent
methods, with lifetime generics. Type generics, async functions, conditional
exports, and trait implementations are rejected inside a binding module. Extension
trait implementations instead use `#[stabby::export(callback)]`. All boundary
values, including callbacks, must have stabby-compatible layouts. Spell callback
signatures explicitly in binding declarations so lifetime canonicalization can
inspect them.

Each generated module exposes `register_exports` in host mode and `bind` in
non-host mode. All API binding must finish before extension callbacks run. Calling an
unbound API method is a fatal programming error. ABI reports validate type layouts,
not arbitrary native machine code; configured extensions must be trusted.
The registration runtime requires stabby's `std` feature. Changes to borrowing or
other semantic invariants require a versioned module/key or bootstrap version,
even when the physical layout remains compatible.

## Write an extension

Depend on `steel-extension-api` without enabling `host`, build a `cdylib`, and declare
both lifecycle entrypoints with `export_extension!` (see [Lifecycle](#lifecycle)). The
macro exports one `steel_extension_v3` bootstrap symbol containing the extension's name,
version, dependencies and checked entrypoint signatures. Rebuild extension libraries when
moving to this lifecycle contract; older libraries are skipped with a rebuild message.

Startup runs these stages for all configured extensions:

1. **Pre-init:** bind the host API and call `pre_init(Hooks)`. Only subscribe to
   registration and runtime events here; registries do not exist yet. Registry
   lookups return an error.
2. **Registration events:** the existing registry constructors build vanilla
   entries and invoke the subscribed callbacks with their actual mutable registry.
   Each setter edits that registry immediately. The constructor then publishes it.
3. **Init:** after all registration events succeed and the registries are frozen,
   call `init(context)` in dependency order. `ExtensionContext` exposes the extension's TOML
   configuration and its data directory. Resolve static values, load data and register
   services here.

Use the re-exported `stabby` macro to prepare callback functions. The callback mode
checks ABI-safe argument and return types and generates the calling convention
without exporting a separate named dynamic symbol. Registration stays explicit:

```rust
use steel_extension_api::{stabby, ApiResult, ExtensionContext, Hooks, Registry, World};

#[stabby::export(callback)]
fn register_blocks(mut registry: Registry<'_>) -> ApiResult<()> {
    // Read and edit this registry now, before it freezes.
    Ok(()).into()
}

#[stabby::export(callback)]
fn tick(world: World<'_>, tick: u64) {
    // Perform synchronous world work here.
}

fn pre_init(mut hooks: Hooks<'_>) -> ApiResult<()> {
    hooks.register_world_tick(tick);
    hooks.register_block_registration(register_blocks)
}

fn init(context: ExtensionContext<'_>) -> ApiResult<()> {
    // Parse context.config() and resolve frozen registry values here.
    Ok(()).into()
}

steel_extension_api::export_extension!("example", pre_init, init);
```

`ApiResult` uses stabby's result and owned error string. Convert into a normal Rust
`Result` or use `.match_owned(Ok, Err)?` inside ordinary fallible code.

The registration events replace complete block behaviors and install terrain-only
generators. `Hooks` subscribes to world ticks. World callbacks run once per normal
world tick on the existing world thread; frozen ticks do not invoke them. World
reads reject unavailable chunks, and writes use normal gameplay updates.

## Replace block behavior and choose delegation

`block_behavior::BlockBehavior` covers every core block behavior method, including
placement, collisions, ticks, interactions, block entities, and capability accessors.
One declaration generates its checked trait, original-call bindings, and native
adapter. A drift test rejects core methods missing from this declaration.

Capture the current implementation with
`OriginalBlockBehavior::get(&registry, block_key)` before changing that entry. The
copyable handle keeps that particular implementation, including if another extension
registered it earlier. Later registry edits do not change the captured handle.
Each extension implementation supplies
`original()`; omitted methods call that handle, and overrides can explicitly call
its methods or return their own result. Subscribe in `pre_init` and make changes
in the registration callback:

```rust
#![recursion_limit = "512"]
use steel_extension_api::{stabby, ApiResult, Hooks, Registry};
use steel_extension_api::block_behavior::{BlockBehavior, OriginalBlockBehavior};

struct MyBehavior { original: OriginalBlockBehavior }

#[stabby::export(callback)]
impl BlockBehavior for MyBehavior {
    fn original(&self) -> OriginalBlockBehavior { self.original }
    // Override whichever methods this extension needs.
}

#[stabby::export(callback)]
fn register_blocks(mut registry: Registry<'_>) -> ApiResult<()> {
    let result = (|| {
        let original = OriginalBlockBehavior::get(&registry, "minecraft:white_wool".into())
            .match_owned(Ok, Err)?;
        registry.replace_block_behavior(
            "minecraft:white_wool".into(),
            stabby::boxed::Box::new(MyBehavior { original }).into(),
        ).match_owned(Ok, Err)
    })();
    result.into()
}

fn pre_init(mut hooks: Hooks<'_>) -> ApiResult<()> {
    hooks.register_block_registration(register_blocks)
}
```

The recursion limit accommodates Stabby's complete vtable type. The export macro
checks argument and return layouts and supplies the calling convention for the
implementation; private extension fields stay in the extension library.

Delegation invokes the original receiver. For example, the original
`get_collision_boxes` uses its original shape implementation. Override that method
as well when it should use a extension's new collision shape.

Capability accessors follow the same rule. A extension may delegate to the original,
return `stabby::option::Option::None()`, or return its own registered implementation.
`Registry::create_bonemealable`, `create_fallable`, `create_growing_plant_head`, and
`create_rail` accept checked implementations of the corresponding traits under
`block_behavior::capabilities`. Their copyable registration handles have `borrow()`
methods for use in the accessors. Capability implementations remain pinned for the
process lifetime.

After Minecraft's data registry is available, `init_behaviors()` builds the block
behavior registry and triggers its hook with `&mut BlockBehaviorRegistry` before
publication. Callbacks edit this same registry through
`Registry::replace_block_behavior`, which calls the existing `set_behavior`
directly. Reassigning a block overwrites its current entry. Generator callbacks
similarly insert into the actual registry used to create worlds through
`GeneratorRegistry::register_terrain_generator`. `Extensions` holds runtime event
subscriptions only.

Registration callbacks return `ApiResult<()>`. The host records a failure and
skips that extension's remaining registration callbacks. Native constructors still
finish and freeze their registries once; startup then fails before any extension's
init or world creation. Registration has no rollback or retry. Subscriptions made
after the corresponding registration event return an error.

There is one block behavior registry. Only implementations explicitly captured by
extensions are retained for delegation. Captures and their capability views are pinned
for the process lifetime, matching the runtime's lack of unloading. Normal gameplay
uses one vector lookup.
Block replacements are process-wide and immutable after startup. The wool example
replaces the whole behavior and explicitly delegates non-shears interactions to
its original. Native embedders subscribe with
`BlockBehaviorRegistry::on_registration` before calling the unchanged
`steel_core::bootstrap::init_globals`. The normal initializer fires the event just
before freezing the behavior registry.

`ItemStack::from_block(state)` creates one host-owned item from the registered
block-to-item association. `World::pop_resource(pos, stack)` consumes that stack
and uses normal block drops, including the game rule and pickup delay. The wool
example creates its stack before replacing the block to preserve the wool color.

## Block entity implementations and host-owned values

`block_behavior::block_entity::BlockEntityBehavior` supplies entity data callbacks.
`BlockEntity::new` creates extension data for an existing registered entity type after
checking that its type accepts the supplied block state. Adding new registry entity
types is outside the current API.

An original factory result exposes `into_created()`. Its entity handle supports
`share()` and `borrow()` for explicit delegation. `with_behavior()` wraps that entity
while retaining its common metadata. Load and save callbacks are mandatory; extensions
choose which original callbacks to invoke. Steel continues to own lifecycle
orchestration, component storage, and world persistence.

The native [boundary fixture](tests/fixture/src/lib.rs) demonstrates capability
inheritance, removal, a custom rail capability, and a comparator entity wrapper
that delegates original serialization while saving its own counter.

Complex core values use nominal opaque handles. Their payloads and allocations
stay on the host; a paired host destructor frees each value or transfers it back
exactly once. Borrowed payloads retain invariant lifetimes and cannot escape their
callback or move to another thread. Shared block entity handles follow the actual
host `Arc` ownership and are explicitly checked for `Send + Sync` on the host.
Complex payload transfers currently allocate host memory; scalar values and the
borrowed `World` handle stay inline.

Terrain generators declare a vanilla dimension type and uniform biome, and do not
run structures or decorators. Their chunk writes are checked as a complete batch
before going through Steel's terrain-stage capability. Seeded positional sampling
uses Steel's vanilla xoroshiro implementation. Generator registration occurs before
world configuration validation and uses the persisted world seed.

See [wool shearing](../extension-example/wool-shearing/README.md) for block
replacement, [Game of Life](../extension-example/game-of-life/README.md) for
world ticks, and [strange terrain](../extension-example/strange-terrain/README.md)
for generator registration. Each builds and loads independently.

## Commands and temporary minigame worlds

Subscribe with `Hooks::register_commands` during pre-init. Its callback receives
`Commands` and registers commands directly in the startup command registry. The
host freezes the dispatcher before invoking extension init. Literal player commands
use the existing namespace, permissions, and Brigadier paths.

`Hooks::register_server_tick` supplies borrowed `Server` access on the server's
existing tick thread before world workers run. `Server::create_temporary_world`
starts preparation with a registered generator and explicit `WorldSettings` for
seed, spawn, default game mode, and difficulty. A primary world creates an
isolated temporary domain; additional worlds share that domain's clock.
`operation_status` reports completion and consumes the terminal result. CPU work
uses the existing generation pool; publication runs in the server job stage.

`switch_domain` saves and restores normal player state. Temporary-domain player
state routes to RAM, and reconnects keep the last lasting domain. Retired domain
identities reject stale async writes rather than falling back to disk. World and
player data never get a temporary save directory. `destroy_temporary_domain`
rejects new entries, waits for players and relocation jobs, drains background work,
and retires the actual worlds and chunk tickets.

Player snapshots contain a UUID plus actor ID, so a reconnected incarnation does
not inherit stale game membership. Bot controls expire after one actor tick;
movement, gravity, reach, and tool speed use the regular core implementations.
Bots are actual tracked players with a server-owned connection, without an external
client or account.

See the [Spleef example](../extension-example/spleef/README.md) for a lobby, countdown,
fresh arena, bots, elimination, player restoration, and complete world retirement.

## Lifecycle

```rust
steel_extension_api::export_extension! {
    name: "worldguard",          // unique; also the data directory name
    version: "0.1.0",
    depends: ["worldedit"],      // must be installed; initialized first
    soft_depends: [],            // initialized first when installed
    pre_init: pre_init,          // fn(Hooks<'_>) -> ApiResult<()> or Result<(), impl Display>
    init: init,                  // fn(ExtensionContext<'_>) -> same
}
```

The short form `export_extension!("name", pre_init, init)` declares version `0.0.0` and no
dependencies. Loading follows [Load and package extensions](#load-and-package-extensions).
Configuration comes from the `[extensions.config]` table of an `[[extensions]]` entry, or
otherwise from `extensions/<name>/config.toml`, created empty on first start. Each extension
gets `extensions/<name>/` (created before init) through `ExtensionContext::data_directory`.
The descriptor records `API_VERSION`; `/extensions` (alias `/plugins`) lists the loaded
extensions and `/extensions <name>` shows version, API version, dependencies and source.
`hooks.on_shutdown(|server| ...)` runs once on a clean stop, in reverse order, before worlds
are saved; the `ServerStopAction` is emitted at the same point.
`steel_extension_api::log::{info, warn, error}` write to the server log tagged with the
extension name.

## Hello world

```rust
use steel_extension_api::{Hooks, ExtensionContext, command::{Arg, CommandBuilder, argument},
    events::EventPriority, text::Text};

fn pre_init(mut hooks: Hooks<'_>) -> Result<(), String> {
    hooks.on_commands(|commands| {
        commands.register(CommandBuilder::new("hello:greet").default_access().then(
            argument("target", Arg::player()).executes(|ctx| {
                let target = ctx.player_arg("target")?;
                let message = Text::new("Hello!").color("gold").to_json();
                let _ = ctx.server().send_message(target, message.as_str().into());
                Ok(1)
            }),
        ))
    })?;
    hooks.on_block_break(EventPriority::Normal, |event| {
        if steel_extension_api::block_key(event.state()).as_str() == "minecraft:bedrock" {
            event.set_cancelled(true);
        }
    });
    Ok(())
}

fn init(_context: ExtensionContext<'_>) -> Result<(), String> {
    Ok(())
}

steel_extension_api::export_extension!("hello", pre_init, init);
```

## Events

`steel_core::extension::events` defines owned, ABI-free events fired at vanilla decision
points; with no subscribers gameplay is unchanged. Plugins subscribe during pre-init with
closures (`hooks.on_chat(priority, |event| ...)`, `on_*_with(priority, ignore_cancelled, ..)`)
or the native `subscribe_*(priority, .., extern fn, token)` form.

| Event | Fired | Mutable / cancellable |
|---|---|---|
| `PreLoginEvent` | after authentication, before admission | `deny(reason)` |
| `JoinEvent`, `QuitEvent` | join / leave broadcast | message |
| `ChatEvent` | player chat | message, `set_rendered`, cancel |
| `CommandEvent` | typed command before parsing | command line, cancel |
| `BlockBreakEvent`, `BlockPlaceEvent` | before the block changes | cancel |
| `InteractEvent` | left/right click on a block, right click in air | cancel |
| `DamageEvent` | any living entity, before cooldowns and armor | amount, cancel |
| `DeathEvent` | player death | message |
| `MoveEvent` | player enters a different block | cancel (teleports back) |
| `TeleportEvent` | `/tp`, extension and other server teleports | destination, cancel |
| `RespawnEvent`, `WorldChangeEvent` | after respawn / world transition | — |
| `GameModeChangeEvent`, `ItemHeldEvent`, `SwapHandsEvent` | before the change | cancel |
| `ToggleEvent` | sneak, sprint, flight | cancel (flight is resynced) |
| `DropItemEvent`, `PickupItemEvent` | item leaves / enters the inventory | cancel |
| `InteractEntityEvent`, `SignChangeEvent` | entity click / sign edit | lines, cancel |
| `InventoryOpenEvent`, `InventoryClickEvent`, `InventoryCloseEvent` | container menus | cancel open/click |
| `ExpChangeEvent`, `FoodLevelChangeEvent` | experience gain / hunger change | amount, cancel |
| `EntitySpawnEvent`, `EntityDeathEvent` | any entity added / living death | cancel, drops |
| `ExplosionEvent`, `PistonEvent` | before blocks are destroyed / moved | block list, cancel |
| `ChunkLoadEvent`, `ChunkUnloadEvent`, `WorldLoadEvent`, `WorldUnloadEvent`, `WeatherChangeEvent` | world lifecycle | weather cancel |
| `ServerCommandEvent`, `ServerListPingEvent`, `PluginMessageEvent` | console/extension commands, status ping, custom payload | command, MOTD, cancel |

`BlockBreakEvent::set_drop_items(false)` keeps the break but suppresses its loot (auto-sell
plugins). Respawning creates a new player entity, so key per-player state by UUID.

Handlers run in priority order (`Lowest` … `Highest`, then `Monitor`, whose setters do nothing)
on the thread processing the action: packet workers for player actions and world threads for
damage and death. Handlers for different players may run concurrently. `set_rendered` sends a
complete component as system chat instead of the signed message, like Paper chat renderers;
editing the message text relays it unsigned.

## Commands, players and text

`command::CommandBuilder` builds typed trees (literal, boolean, integer, long, double, word,
string, greedy, player(s), block position, block state, item, world) with aliases, per-node
permissions and closure suggestions. Root permissions derive from the key
(`essentials:home` → `essentials.command.home`) unless `.permission(...)` replaces them;
`.default_access()` allows everyone unless explicitly denied.

`Server` methods address players by `PlayerId` and re-check the session on every call:
lookup by name/UUID, location and cross-world teleport (firing `TeleportEvent`), health, food,
flight, game mode, kick, chat/action bar/title, broadcast, permissions with tri-state results,
permission metadata and groups, and giving named items. Text accepts vanilla JSON components or
plain text; `text::Text` builds components and `text::legacy` converts `&` color codes.
`World` adds height bounds and `set_block_with_flags`; `parse_block_state`,
`block_state_string` and `block_key` convert states.

`scheduler::run_later`/`run_repeating` run closures on the server tick after
`hooks.enable_scheduler()`.

## Items

`items::ItemSpec` describes a stack as plain data: item key, count, name, lore (JSON or `&`
codes through `name_legacy`/`lore_legacy`), glint, unbreakable, vanilla enchantments and
private extension data (`with_data("myextension:key", value)`, stored in `minecraft:custom_data`, so
it survives restarts and chests). `Server::slot`, `set_slot`, `held_slot`, `give` and
`set_item_data` read and write vanilla inventory slots (0–8 hotbar, 9–35 main, 36–39 armor,
40 off hand).

## Game API

| Module | |
|---|---|
| `effects` | `World::spawn_particle(Particle)` with dust/color/block options and `Particle::note(pos, pitch)`; `World::play_sound`; per-player `play_sound_to`, `add_effect`/`remove_effect`, `give_experience`/`give_levels`, `set_walk_speed`/`set_fly_speed` |
| `hud` | per-player sidebar (diffed, up to 15 lines), boss bars by extension-chosen UUID, tab list header/footer override |
| `menus` | read-only chest GUIs: `menus::handler(\|server, click\| ..)` once, then `server.open_menu_with(player, title, rows, items, handler)`; `set_menu_item`, `close_menu`. Clicks never move items and are routed before any container lock, so handlers may change inventories |
| `worlds` | `Server::with_world(key, \|world\| ..)` from scheduled tasks, day time and weather, `drop_item`, `remove_entity`, `strike_lightning_effect`, `send_plugin_message` |
| `fill` | `World::fill(FillRequest)` below |

## Bulk fill

`World::fill` writes cuboids, layers, spheres and shells (optionally clipped, with weighted
patterns and `All`/`OnlyAir`/`NonAir` modes) one chunk section at a time without physics or
block callbacks, the way Paper prison and editing plugins do. Block entities, heightmaps, light
(propagated by the next light pass), points of interest, saving and client sync stay correct.
The `FillReport` returns counts, timing and a histogram of replaced states, so "break this
layer and sell it" is one call. A fill touching an unloaded chunk is refused without writing.
In a release build, one million blocks take about 94 ms (10.6 M blocks/s, 6.8× faster than
per-block `set_block`); see `cargo test --release -p steel-core --lib bulk_fill_benchmark --
--ignored --nocapture`.

## Services

`services::register::<T>(&context, key, priority, value)` publishes a stabby value, usually a
`stabby::dynptr!(Box<dyn Contract + Send + Sync>)`; `services::get::<T>(key)` returns the
highest-priority provider after checking that its stabby layout report matches `T`, so
incompatible contract versions are errors rather than undefined behavior. Contracts live in
small shared crates such as [`vault-api`](../extension-example/vault-api/src/lib.rs); keys carry
a version suffix (`vault:economy/v1`). See [the examples](../extension-example/README.MD).
Services complement [actions and filters](#ordered-actions-and-filters): a service is one
provider object queried on demand, while actions and filters fan a payload out to every
subscribed handler.

## Check the boundary

```sh
cargo check -p steel-extension-api
cargo check -p steel-extension-api --features host
cargo tree -p steel-extension-api --no-default-features
cargo test --manifest-path stabby/Cargo.toml -p stabby-abi host::tests
cargo test -p steel-extension-api --features host --lib
cargo test -p steel-extension-api --doc
cargo build --manifest-path steel-extension-api/tests/fixture/Cargo.toml
cargo build --manifest-path steel-extension-api/tests/event-probe/Cargo.toml
cargo build --manifest-path extension-example/wool-shearing/Cargo.toml
cargo build --manifest-path extension-example/game-of-life/Cargo.toml
cargo build --manifest-path extension-example/strange-terrain/Cargo.toml
cargo test -p steel-extension-api --features host --test behavior --test native --test game_of_life --test strange_terrain --test registration_failure --test package -- --include-ignored
cargo test -p steel-extension-api --features host --test ports --test events --test folder --test prison -- --include-ignored
cargo build --manifest-path stabby/tests/host-api/Cargo.toml -p host-api-fixture-plugin
cargo run --manifest-path stabby/tests/host-api/Cargo.toml -p host-api-fixture-host -- stabby/tests/host-api/target/debug/libhost_api_fixture_plugin.so
```

The fixture commands above use Linux's library name. Use the corresponding `.dll`
or `.dylib` path on Windows or macOS. The fixture deliberately builds its extension
and host separately and verifies both function calls and opaque method calls.
