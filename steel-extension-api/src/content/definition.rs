use stabby::{option::Option as ApiOption, string::String as ApiString, vec::Vec as ApiVec};

use super::{BlockShapes, BlockStateDefinition, ItemDisplay, VoxelShape};

/// Placement for a block authored facing north, rotated with its shapes/models.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum HorizontalPlacement {
    /// Use the player's horizontal direction, as vanilla stairs do.
    Player,
    /// Wall clicks attach the solid back to that face; floor/ceiling clicks use player direction.
    AgainstClickedFace,
}

/// Overrides over `BlockConfig::new()`, or over an explicitly copied block.
/// Copying is optional and also inherits that block's tool-rule membership.
#[stabby::stabby]
#[derive(Debug, Clone, Default)]
pub struct BlockProperties {
    /// Optional registered block supplying configuration defaults and tool membership.
    pub copy_from: ApiOption<ApiString>,
    /// Mining hardness; -1 makes the block unbreakable.
    pub destroy_time: ApiOption<f32>,
    /// Explosion resistance.
    pub explosion_resistance: ApiOption<f32>,
    /// Extracted vanilla sound type name, for example `WOOD` or `STONE`.
    pub sound: ApiOption<ApiString>,
    /// Surface friction.
    pub friction: ApiOption<f32>,
    /// Movement speed multiplier.
    pub speed_factor: ApiOption<f32>,
    /// Jump height multiplier.
    pub jump_factor: ApiOption<f32>,
    /// Whether item drops require a matching tool.
    pub requires_correct_tool_for_drops: ApiOption<bool>,
    /// Whether lava can ignite this block.
    pub ignited_by_lava: ApiOption<bool>,
    /// Whether ordinary placement may replace this block.
    pub replaceable: ApiOption<bool>,
    /// Whether this block participates in face occlusion.
    pub can_occlude: ApiOption<bool>,
    /// Emitted light, from 0 to 15.
    pub light_emission: ApiOption<u8>,
    /// Light absorption, from 0 to 15.
    pub light_dampening: ApiOption<u8>,
}

impl BlockProperties {
    /// Starts with the native block configuration defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Copies a registered block’s configuration defaults and tool membership.
    #[must_use]
    pub fn of_full_copy(block: &str) -> Self {
        Self {
            copy_from: Some(block.into()).into(),
            ..Self::new()
        }
    }

    /// Overrides mining hardness and explosion resistance.
    #[must_use]
    pub fn strength(mut self, destroy_time: f32, explosion_resistance: f32) -> Self {
        self.destroy_time = Some(destroy_time).into();
        self.explosion_resistance = Some(explosion_resistance).into();
        self
    }

    /// Selects a sound type by its extracted name.
    #[must_use]
    pub fn sound(mut self, sound: &str) -> Self {
        self.sound = Some(sound.into()).into();
        self
    }

    /// Disables face occlusion and light absorption.
    #[must_use]
    pub fn no_occlusion(mut self) -> Self {
        self.can_occlude = Some(false).into();
        self.light_dampening = Some(0).into();
        self
    }
}

/// An independent server block. Steel selects its vanilla client state from
/// the registered shapes; a vanilla block is not required to define this block.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BlockDefinition {
    /// Independent, owner-namespaced server identity.
    pub key: ApiString,
    /// Server gameplay and lighting configuration.
    pub properties: BlockProperties,
    /// Independent server geometry channels.
    pub shapes: BlockShapes,
    /// Optional convenience for copying extracted shape channels only.
    pub vanilla_geometry: ApiOption<BlockStateDefinition>,
    /// Opt in to an approximate client collision when no exact projection exists.
    /// The server always uses the authored shape. Movement corrections are possible.
    pub allow_approximation: bool,
    /// Optional real `facing` state family, with automatic geometry/model rotation.
    pub horizontal: ApiOption<HorizontalPlacement>,
    /// Additional client-only models.
    pub displays: ApiVec<ItemDisplay>,
}

impl BlockDefinition {
    /// Creates a full-cube block using native configuration defaults and exact projection.
    #[must_use]
    pub fn new(key: &str) -> Self {
        Self {
            key: key.into(),
            properties: BlockProperties::new(),
            shapes: BlockShapes::new(VoxelShape::block()),
            vanilla_geometry: None.into(),
            allow_approximation: false,
            horizontal: None.into(),
            displays: ApiVec::new(),
        }
    }

    /// Replaces the property overrides.
    #[must_use]
    pub fn properties(mut self, properties: BlockProperties) -> Self {
        self.properties = properties;
        self
    }

    /// Uses the supplied geometry for every main channel, with empty interaction geometry.
    #[must_use]
    pub fn shape(mut self, shape: VoxelShape) -> Self {
        self.shapes = BlockShapes::new(shape);
        self.vanilla_geometry = None.into();
        self
    }

    /// Copies extracted geometry as a convenience; gameplay remains independent.
    #[must_use]
    pub fn vanilla_geometry(mut self, state: BlockStateDefinition) -> Self {
        self.vanilla_geometry = Some(state).into();
        self
    }

    /// Allows Steel to choose a contained client collision when an exact match is absent.
    #[must_use]
    pub const fn allow_approximation(mut self) -> Self {
        self.allow_approximation = true;
        self
    }

    /// Adds four horizontal facing states. Author the shape/model facing north.
    #[must_use]
    pub fn horizontal_facing(mut self, placement: HorizontalPlacement) -> Self {
        self.horizontal = Some(placement).into();
        self
    }

    /// Sets the model attachments sent with this block.
    #[must_use]
    pub fn displays(mut self, displays: impl IntoIterator<Item = ItemDisplay>) -> Self {
        self.displays = displays.into_iter().collect();
        self
    }
}

/// A slab discovered from the real block registry, without a material allowlist.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct SlabMaterial {
    /// Vanilla slab block key.
    pub block: ApiString,
    /// The slab’s associated placing item key.
    pub item: ApiString,
}
