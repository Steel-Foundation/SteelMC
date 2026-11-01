//! Startup registration of independent server items and blocks for vanilla clients.

mod bindings;
mod definition;
mod shapes;
pub use bindings::api;
pub use bindings::api::ContentRegistry;
pub use definition::{BlockDefinition, BlockProperties, HorizontalPlacement, SlabMaterial};
pub use shapes::{BlockShapes, ShapeBox, VoxelShape};

use stabby::{option::Option as ApiOption, string::String as ApiString, vec::Vec as ApiVec};

/// A static player-head profile; `texture` is the base64 textures property.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct HeadProfile {
    /// UUID in vanilla's four-signed-integer NBT representation.
    pub id: [i32; 4],
    /// Base64-encoded `textures` property supplied by the extension.
    pub texture: ApiString,
}

/// Vanilla item appearance. A profile is meaningful only for a player head.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct ItemAppearance {
    /// Vanilla registry key, for example `minecraft:player_head`.
    pub item: ApiString,
    /// Optional item model key supplied by a resource pack or existing vanilla assets.
    pub model: ApiOption<ApiString>,
    /// Static skin profile when the appearance is a player head.
    pub profile: ApiOption<HeadProfile>,
}

/// An independent server item. Use `BlockItemDefinition` for placement behavior.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct ItemDefinition {
    /// Owner-namespaced server identity, retained in saved inventories.
    pub key: ApiString,
    /// Literal item name displayed to the client.
    pub name: ApiString,
    /// The vanilla item and optional skin used on the client.
    pub appearance: ItemAppearance,
    /// Maximum stack size, from 1 through 99.
    pub max_stack_size: i32,
}

/// One client-only item display. Coordinates are relative to the placed block's
/// minimum corner; item display context is vanilla `none`.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct ItemDisplay {
    /// Item rendered by this display.
    pub appearance: ItemAppearance,
    /// XYZ translation from the block's minimum corner, in blocks.
    pub translation: [f32; 3],
    /// Positive XYZ model scale.
    pub scale: [f32; 3],
    /// Left rotation quaternion in XYZW order.
    pub rotation: [f32; 4],
}

/// One named vanilla state property, such as `type=top` on a slab.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BlockProperty {
    /// Vanilla property name, such as `type` or `facing`.
    pub name: ApiString,
    /// Vanilla property value, such as `top` or `east`.
    pub value: ApiString,
}

/// A vanilla block and property overrides over its registered default state.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BlockStateDefinition {
    /// Registered vanilla block key.
    pub block: ApiString,
    /// Named overrides; unspecified properties retain their vanilla defaults.
    pub properties: ApiVec<BlockProperty>,
}

impl From<&str> for BlockStateDefinition {
    fn from(block: &str) -> Self {
        Self {
            block: block.into(),
            properties: ApiVec::new(),
        }
    }
}

/// Registers a block and its placing item together, resolving their association
/// and optional self-drop without forward references to unregistered content.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BlockItemDefinition {
    /// Block identity, physical carrier, and visual model.
    pub block: BlockDefinition,
    /// Placing item identity and inventory appearance.
    pub item: ItemDefinition,
    /// Drops exactly one placing item when loot is requested. This fixed policy
    /// does not inherit the carrier's loot table or explosion decay.
    pub drop_self: bool,
}

/// A model selected by named bed-state properties. Selectors must not overlap.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BlockModel {
    /// Partial selector, for example `part=head,facing=north`.
    pub properties: ApiVec<BlockProperty>,
    /// Additional model parts around the vanilla bed.
    pub displays: ApiVec<ItemDisplay>,
}

/// A real two-part bed with ordinary sleeping, occupancy, and respawn behavior.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BedDefinition {
    /// Extension-owned block key.
    pub key: ApiString,
    /// Ordinary vanilla colored bed whose state family and appearance are used.
    /// Straw beds have different rules and are not accepted here.
    pub template: ApiString,
    /// Placing item; mining either half returns one of these items.
    pub item: ItemDefinition,
    /// Optional model additions for particular parts and orientations.
    pub models: ApiVec<BlockModel>,
}
