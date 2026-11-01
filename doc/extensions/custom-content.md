# Custom items and blocks for vanilla clients

Extensions can register real server item and block identities. Steel saves those
identities and translates them into vanilla items and block states when sending
packets. Clients need neither a mod nor a resource pack for the head-based model
in the [lucky-block example](../../extension-example/lucky-block/README.md).

This separation follows the approach used by
[Polymer](https://github.com/Patbox/polymer/tree/dev/26.3). Steel owns the network
translation and display tracking; extensions provide typed definitions.

## Register content

Subscribe through `Hooks::register_content_registration` during pre-init. The
callback receives `content::ContentRegistry<'_>` before registries freeze and
before block behaviors, commands, extension init, or worlds are initialized.
Global lookups remain unavailable during this phase. Resolve content by the keys
in your definitions, then use ordinary registry lookups in later callbacks.

For example, this registers a named paper token with its own server identity:

```rust
use steel_extension_api::{ApiResult, ExtensionContext, Hooks, content::{
    ContentRegistry, ItemAppearance, ItemDefinition,
}, stabby};

fn pre_init(mut hooks: Hooks<'_>) -> ApiResult<()> {
    hooks.register_content_registration(register_content)
}

#[stabby::export(callback)]
fn register_content(mut registry: ContentRegistry<'_>) -> ApiResult<()> {
    registry.register_item(ItemDefinition {
        key: "demo:token".into(),
        name: "Demo Token".into(),
        appearance: ItemAppearance {
            item: "minecraft:paper".into(),
            model: None.into(),
            profile: None.into(),
        },
        max_stack_size: 64,
    })
}

fn init(_context: ExtensionContext<'_>) -> ApiResult<()> { Ok(()).into() }

steel_extension_api::export_extension! {
    name: "token-example",
    version: "0.1.0",
    depends: [],
    soft_depends: [],
    pre_init: pre_init,
    init: init,
}
```

Build and package the crate as described in [Create an extension](getting-started.md).
After loading it, an operator can obtain its item with `/give @s demo:token`.

## Definitions and API

| Method | Behavior |
| --- | --- |
| `ContentRegistry::config()` | Returns this extension's TOML configuration. |
| `register_item(ItemDefinition)` | Registers an independent item with default item behavior. |
| `register_block(BlockDefinition)` | Registers a block with selected vanilla geometry, without a placing item or default drops. Returns its default `BlockState`. |
| `register_block_item(BlockItemDefinition)` | Registers the block, its placing item, and optional fixed self-drop together. Returns the block's default state. |
| `register_bed(BedDefinition)` | Registers a complete bed state family, its placing item, and state-selected models. Returns the bed's default state. |

All registration methods return `ApiResult`. Invalid keys, duplicate entries,
unsupported carriers, invalid profiles, and invalid transforms are errors.
Custom keys must use a namespace other than `minecraft`. Definitions use Stabby
strings, vectors, and options; collect owned model parts with
`parts.into_iter().collect()` to construct a Stabby vector.

`ItemDefinition` contains `key`, literal `name`, `appearance`, and
`max_stack_size` (1–99). `ItemAppearance` contains a vanilla `item` key and an
optional `HeadProfile`. A profile requires `minecraft:player_head`; its `id` is
the four signed integers used by vanilla NBT UUIDs, and `texture` is the base64
`textures` property. `ItemAppearance::model` optionally selects an independent
item-model key (the `minecraft:item_model` component). Existing vanilla models
work without a pack; custom keys require their assets on the client. Steel preserves the supplied property rather than resolving
an account name.

`BlockDefinition::new(key)` creates an independent server block. It uses native
configuration defaults and a full-cube shape. A vanilla block is not required as
a template. Define gameplay through `BlockProperties`, and geometry with
`VoxelShape::box_shape` and `.or(...)`:

```rust
use steel_extension_api::content::{BlockDefinition, BlockProperties, VoxelShape};

let block = BlockDefinition::new("demo:vertical_half")
    .properties(BlockProperties::new().strength(2.0, 3.0).sound("WOOD").no_occlusion())
    .shape(VoxelShape::box_shape(0.0, 0.0, 8.0, 16.0, 16.0, 16.0))
    .allow_approximation();
```

Shape coordinates use vanilla's **0–16** convention. This example occupies a
vertical half of the block. Display coordinates still use **0–1 block units**.
`.shape(...)` assigns collision, support, outline, occlusion, and visual shapes;
interaction geometry starts empty. For distinct channels, edit `block.shapes`
(`BlockShapes`). Boxes must have finite, increasing bounds. Empty collision is
allowed, but the targeting outline must be nonempty.

`BlockProperties` supports hardness, explosion resistance, sound type, friction,
movement/jump multipliers, correct-tool requirements, lava ignition,
replaceability, occlusion, and lighting. Unspecified fields retain native
`BlockConfig::new()` defaults. Sound names such as `WOOD` and `STONE` come from
extracted sound types. `BlockProperties::of_full_copy("minecraft:stone_slab")`
is an optional convenience: copy configuration defaults and inherit that block's
tool-rule membership, then apply overrides. It does not copy dynamic behavior,
tags, or loot. Neither copying properties nor copying geometry is required.

`vanilla_geometry(BlockStateDefinition)` optionally copies all extracted shape
channels from an existing dry, static vanilla state. Named `BlockProperty`
overrides resolve over its actual default state. This is useful for stairs or
fences without manually reconstructing their boxes. It rejects wet states,
fluids, air, block entities, and context-dependent geometry. It does not inherit
slab merging, waterlogging, redstone, facing, or connection updates.

`BlockItemDefinition` combines a `block`, an `item`, and `drop_self`. Their keys
may differ. Registration validates the whole definition before inserting either
entry. Placement uses Steel's ordinary block-item implementation, including
collision checks and survival consumption. Pick-block resolves the real item.

## Automatic vanilla representation

Steel chooses the client block state from its actual extracted vanilla shapes.
The block definition owns the server geometry and gameplay; its network carrier
is an implementation detail. Exact collision matching is the default and compares
box unions, rather than requiring identical box decomposition. Selection prefers
an explicitly copied geometry/material on a tie, followed by invisible carriers
and stable vanilla state order. Offset, dynamic, wet, and block-entity carriers
are excluded from automatic selection.

There is no vanilla block with exact vertical half-slab collision. Calling
`.allow_approximation()` explicitly permits a contained approximation. Steel
chooses the available collision with the least missing volume and ensures a
visible carrier's outline fits inside the custom outline. **The server retains
the exact authored collision.** Vanilla clients predict the selected carrier's
collision, so walking into the unmatched portion can cause movement corrections.
Resource packs change the appearance, not that collision limitation. Without
opt-in, unsupported shapes fail registration rather than silently approximating.

Attach `ItemDisplay` models to cover the carrier and show the intended block.
Additional display parts have no independent collision. The
[furniture example](../../extension-example/furniture/README.md) rotates vanilla
slab models for the default no-pack version. Its optional generated resource
pack adds dedicated vertical models and inventory icons.

Custom mining completes on the server using the authored block hardness and
Steel's existing tool calculation. An optional copied property source supplies
tool-rule membership. Abort packets, changing tools/components, leaving reach,
changing mode/world, or replacing the target cancel an active timer. Early stop
packets cannot bypass that timer. Vanilla blocks retain their normal mining path.
The normal mining calculation includes tool efficiency, haste/fatigue, the block
break speed attribute, and underwater/airborne penalties.
Client sounds, cracks, particles, and targeting still follow the carrier.

`drop_self: true` returns one placing item whenever drops are requested. It does
not inherit vanilla explosion-decay loot; game-mode and drop-gamerule checks
still apply. The lucky-block example uses `drop_self: false` with its own
`player_destroy` callback for random item rewards and block traps. Install block
callbacks during the later behavior registration hook using
`Registry::replace_block_behavior`.

## Directional placement

Call `.horizontal_facing(HorizontalPlacement::Player)` to add real `facing` states
for north, east, south, and west. Author the base shapes and display parts facing
**north**. Steel rotates all six shape channels, display origins, and display
quaternions around the block center and selects a client state for each facing.
The default state faces north. Facing is persisted and models are reconstructed
from the saved state after chunk loading.

`Player` uses the same horizontal placement direction as vanilla stairs. The
optional `AgainstClickedFace` rule instead faces the solid back toward a clicked
wall; floor and ceiling clicks still use player direction. These rules add
horizontal rotation; they do not add upside-down placement, stair corners, or
waterlogging. The furniture example uses `Player` for chairs and vertical slabs.

Callbacks can inspect `state.property("facing".into())`, which returns an owned
optional string. The chair uses it to rotate its seat offset with its model.
Extensions can still replace `get_state_for_placement` for their own rules.
Without `horizontal_facing`, an ordinary definition has one static state.
Ordinary bed state families use their separate registration API below.

## Functional furniture

The [furniture example](../../extension-example/furniture/README.md) provides a
usable chair, vertical half slabs for every vanilla slab material, and a custom
bed. The default version needs no resource pack. Slabs have exact server half-block
geometry and automatically selected, approximate client collision. An optional
resource pack supplies dedicated inventory icons.

In a block interaction callback, `Player::sit(anchor, offset)` creates a temporary
seat. `anchor` is a loaded, non-air block within interaction reach; `offset` is
a `Position` relative to its minimum corner and specifies the player's entity
origin, rather than the cushion's surface. Account for the seated player model
when choosing the offset. The result is `ApiResult<bool>`: `false` means boarding
was declined, including an occupied seat or an already-mounted player. Invalid
coordinates, reach, or world/chunk context return errors.

Seats use invisible, non-colliding interaction entities. Sneaking dismounts the
player through Steel's existing stand-up search. Empty seats and seats whose
anchor changes are removed during entity ticking. They are excluded from chunk
and passenger persistence. Only one temporary seat may occupy a block.

`BedDefinition` contains `key`, a vanilla `template` key, an `item`, and `models`.
The template must be an ordinary colored vanilla bed; straw beds have different
rules and are rejected. Unlike fixed geometry definitions, this registers all
of the template's `facing`, `occupied`, and `part` states, with their extracted
shapes. Core installs its ordinary bed behavior: two-half placement, orientation,
sleeping, occupancy, waking, respawn selection, and dimension restrictions,
including bad-respawn-point explosions.

Each `BlockModel` contains `properties: Vec<BlockProperty>` and `displays`.
Properties select matching states; omitted properties match any value. Selectors
must be valid and must not overlap. Unmatched states have no additional displays.
The example selects each facing and half, sharing models across occupancy values.
Changing state refreshes the client model.

Mining either half drops exactly one custom bed item. This fixed self-drop policy
does not inherit vanilla explosion-decay loot. Custom tags and village POI
membership are not inherited; existing missing villager sleeping and advancement
mechanics in Steel remain outside this API. Additional frame displays have no
independent collision.

## Models and their lifetime

Each `ItemDisplay` supplies an appearance, XYZ `translation`, positive XYZ
`scale`, and an XYZW `rotation` quaternion. The quaternion must be finite and
unit-length; use `[0.0, 0.0, 0.0, 1.0]` for identity. Scale applies before
rotation. The origin is the block's minimum corner and the display context is vanilla
`none`. Animation is not exposed. A model can contain
several parts, including heads with different skin profiles.

The tracker compensates the entity origin so vanilla samples model lighting
above the opaque carrier. Model coordinates remain relative to the block's
minimum corner. Selection outlines, mining effects, and particles use the
vanilla carrier's representation.

Steel uses generated vanilla item-display metadata. It creates the parts after
the client receives their chunk, updates them alongside block packets, and
removes them on replacement, chunk unload, or world detachment. Parts are
client-only attachments. They have no gameplay collision and are not saved as
independent world entities. Reloading a chunk reconstructs the model from its
custom block state.

The lucky-block model uses a
[yellow question-mark skin](https://minecraft-heads.com/custom-heads/head/4663-lucky-block-yellow)
by Luck for both inventory and placement. One head is enlarged to one block, so
each face shows the whole design. A 0.2% margin covers the opaque planks surface
without coincident faces; collision remains exactly one full block. Minecraft
obtains the skin through its normal texture mechanism; no server resource pack
is required. Multi-part models remain supported by supplying several displays.

## Translation and persistence

Inventory translation rebases the complete component patch onto the vanilla
item's defaults. A transport-only custom-data envelope retains the real item
identity and components for creative copies. Decoding validates the represented
item and matching vanilla carrier; raw server-only numeric IDs are rejected.
Normal creative permissions and inventory checks still apply.

Nested templates in bundles, containers, projectiles, remainders, pot
decorations, and sulfur-cube contents are translated too. Inventory prediction
hashes compare the same representation the client received. Chunk palettes,
block changes, block-state entity metadata, particles, falling blocks, and
block-related events use vanilla state IDs. Server-only entries are omitted
from client registry tags and statistics, while server statistics retain their
real identities.

Saved inventories and chunks contain the custom registry keys and original
components. Transport envelopes and display entity IDs are not persistent data.
Keep the registering extension installed when loading worlds containing its
content; removing or renaming registry entries is not a content migration API.
