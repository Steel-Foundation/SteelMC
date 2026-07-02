pub mod behavior;
pub mod block_state_ext;
pub mod properties;
pub mod shapes;

use std::sync::OnceLock;

use glam::DVec3;
use rustc_hash::FxHashMap;

use crate::blocks::behavior::BlockConfig;
use crate::blocks::properties::{DynProperty, Property};
use crate::blocks::shapes::ShapeChannel;
use crate::{RegistryExt, TaggedRegistryExt};
use steel_utils::{BlockPos, BlockStateId};

/// Function type for shape lookups. Takes a state offset and returns the shape.
pub type ShapeFn = fn(u16) -> shapes::VoxelShape;
/// Function type for light-property lookups. Takes a state offset and returns extracted vanilla properties.
pub type LightPropertiesFn = fn(u16) -> BlockLightProperties;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockLightProperties {
    pub light_emission: u8,
    pub light_dampening: u8,
    pub use_shape_for_light_occlusion: bool,
}

impl BlockLightProperties {
    pub const OPAQUE_FULL_BLOCK: Self = Self {
        light_emission: 0,
        light_dampening: 15,
        use_shape_for_light_occlusion: false,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct StateBooleanOverwrite {
    pub offset: u16,
    pub value: bool,
}

impl StateBooleanOverwrite {
    pub const fn new(offset: u16, value: bool) -> Self {
        Self { offset, value }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StateBooleanData {
    pub default: bool,
    pub overwrites: &'static [StateBooleanOverwrite],
}

impl StateBooleanData {
    pub const TRUE: Self = Self::new(true, &[]);
    pub const FALSE: Self = Self::new(false, &[]);

    pub const fn new(default: bool, overwrites: &'static [StateBooleanOverwrite]) -> Self {
        Self {
            default,
            overwrites,
        }
    }

    pub fn value(self, offset: u16) -> bool {
        self.overwrites
            .iter()
            .find(|overwrite| overwrite.offset == offset)
            .map_or(self.default, |overwrite| overwrite.value)
    }
}

pub struct Block {
    pub key: Identifier,
    pub config: BlockConfig,
    pub properties: &'static [&'static dyn DynProperty],
    pub default_state_offset: u16,
    /// Vanilla `BlockState.isSuffocating` values indexed by block-local state offset.
    pub suffocating: StateBooleanData,
    /// Extracted vanilla light properties indexed by block-local state offset.
    pub light_properties: LightPropertiesFn,
    /// Function to get collision shape for a state offset
    pub collision_shape: ShapeFn,
    /// Function to get block support shape for a state offset
    pub support_shape: ShapeFn,
    /// Function to get outline shape for a state offset
    pub outline_shape: ShapeFn,
    /// Function to get occlusion shape for a state offset
    pub occlusion_shape: ShapeFn,
    /// Function to get interaction shape for a state offset
    pub interaction_shape: ShapeFn,
    /// Function to get visual shape for a state offset
    pub visual_shape: ShapeFn,
    /// Shape channels whose extracted boxes are normalized and need positional offset.
    pub shape_offsets: shapes::ShapeOffsetFlags,
    /// Cached registry ID, set during registration for O(1) lookup on hot paths.
    pub id: OnceLock<usize>,
}

impl std::fmt::Debug for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Block")
            .field("key", &self.key)
            .field("config", &self.config)
            .field("properties", &self.properties)
            .field("default_state_offset", &self.default_state_offset)
            .finish_non_exhaustive()
    }
}

/// Default shape function that returns a full block.
const fn full_block_shape(_offset: u16) -> shapes::VoxelShape {
    shapes::VoxelShape::FULL_BLOCK
}

const fn opaque_full_block_light_properties(_offset: u16) -> BlockLightProperties {
    BlockLightProperties::OPAQUE_FULL_BLOCK
}

/// Default interaction shape function that returns an empty shape.
const fn empty_shape(_offset: u16) -> shapes::VoxelShape {
    shapes::VoxelShape::EMPTY
}

impl Block {
    pub const fn new(
        key: Identifier,
        config: BlockConfig,
        properties: &'static [&'static dyn DynProperty],
    ) -> Self {
        Self {
            key,
            config,
            properties,
            default_state_offset: 0,
            suffocating: StateBooleanData::TRUE,
            light_properties: opaque_full_block_light_properties,
            collision_shape: full_block_shape,
            support_shape: full_block_shape,
            outline_shape: full_block_shape,
            occlusion_shape: full_block_shape,
            interaction_shape: empty_shape,
            visual_shape: full_block_shape,
            shape_offsets: shapes::ShapeOffsetFlags::NONE,
            id: OnceLock::new(),
        }
    }

    /// Sets the shape functions for this block.
    pub const fn with_shapes(
        mut self,
        collision: ShapeFn,
        support: ShapeFn,
        outline: ShapeFn,
        occlusion: ShapeFn,
        interaction: ShapeFn,
        visual: ShapeFn,
    ) -> Self {
        self.collision_shape = collision;
        self.support_shape = support;
        self.outline_shape = outline;
        self.occlusion_shape = occlusion;
        self.interaction_shape = interaction;
        self.visual_shape = visual;
        self
    }

    /// Sets the extracted vanilla `BlockState.isSuffocating` values for this block.
    pub const fn with_suffocating(mut self, suffocating: StateBooleanData) -> Self {
        self.suffocating = suffocating;
        self
    }

    /// Sets the extracted vanilla light properties for this block.
    pub const fn with_light_properties(mut self, light_properties: LightPropertiesFn) -> Self {
        self.light_properties = light_properties;
        self
    }

    /// Sets which shape channels use the block state's positional offset.
    pub const fn with_shape_offsets(mut self, offsets: shapes::ShapeOffsetFlags) -> Self {
        self.shape_offsets = offsets;
        self
    }

    /// Gets the collision shape for a given state offset.
    #[inline]
    pub fn get_collision_shape(&self, offset: u16) -> shapes::VoxelShape {
        (self.collision_shape)(offset)
    }

    /// Gets the block support shape for a given state offset.
    #[inline]
    pub fn get_support_shape(&self, offset: u16) -> shapes::VoxelShape {
        (self.support_shape)(offset)
    }

    /// Gets the outline shape for a given state offset.
    #[inline]
    pub fn get_outline_shape(&self, offset: u16) -> shapes::VoxelShape {
        (self.outline_shape)(offset)
    }

    /// Gets the occlusion shape for a given state offset.
    #[inline]
    pub fn get_occlusion_shape(&self, offset: u16) -> shapes::VoxelShape {
        (self.occlusion_shape)(offset)
    }

    /// Gets the interaction shape for a given state offset.
    #[inline]
    pub fn get_interaction_shape(&self, offset: u16) -> shapes::VoxelShape {
        (self.interaction_shape)(offset)
    }

    /// Gets the visual shape for a given state offset.
    #[inline]
    pub fn get_visual_shape(&self, offset: u16) -> shapes::VoxelShape {
        (self.visual_shape)(offset)
    }

    #[inline]
    pub fn get_light_properties(&self, offset: u16) -> BlockLightProperties {
        (self.light_properties)(offset)
    }

    /// Returns the vanilla block-state positional offset for this block.
    #[must_use]
    pub fn offset_at(&self, pos: BlockPos) -> DVec3 {
        self.config.offset_at(pos)
    }

    /// Sets the default state offset for this block.
    /// The offset is relative to the block's base state ID.
    ///
    /// For easier usage, consider using `with_default_state_from_indices` or the
    /// `default_state!` macro instead of calculating the offset manually.
    ///
    /// # Example
    /// ```ignore
    /// const REPEATER: Block = Block::new("repeater", props, &[...])
    ///     .with_default_state(4);
    /// ```
    pub(crate) const fn with_default_state(mut self, offset: u16) -> Self {
        self.default_state_offset = offset;

        self
    }

    /// Const helper to calculate state offset from property indices and counts.
    /// Properties are processed in reverse order to match Minecraft's encoding
    /// (last property = inner loop with multiplier 1).
    #[must_use]
    pub const fn calculate_offset(property_indices: &[usize], property_counts: &[usize]) -> u16 {
        let mut offset = 0u16;
        let mut multiplier = 1u16;
        let len = property_indices.len();

        // Iterate in reverse order: last property first (inner loop)
        let mut i = len;
        while i > 0 {
            i -= 1;
            offset += property_indices[i] as u16 * multiplier;
            multiplier *= property_counts[i] as u16;
        }

        offset
    }

    #[must_use]
    pub fn default_state(&'static self) -> BlockStateId {
        crate::REGISTRY.blocks.get_default_state_id(self)
    }

    /// Total number of distinct states (the product of every property's value count).
    #[must_use]
    pub fn state_count(&self) -> u16 {
        self.properties
            .iter()
            .map(|p| p.get_possible_values().len() as u16)
            .product()
    }

    /// Returns `true` if this block is tagged with the given tag.
    pub fn has_tag(&'static self, tag: &Identifier) -> bool {
        crate::REGISTRY.blocks.is_in_tag(self, tag)
    }
}

pub type BlockRef = &'static Block;

// The central registry for all blocks.
pub struct BlockRegistry {
    blocks_by_id: Vec<BlockRef>,
    blocks_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
    pub state_to_block_lookup: Vec<BlockRef>,
    /// Maps state IDs to block IDs (parallel to `state_to_block_lookup` for O(1) lookup)
    pub state_to_block_id: Vec<usize>,
    /// Maps block IDs to their base state ID
    pub block_to_base_state: Vec<u16>,
    /// The next state ID to be allocated
    pub next_state_id: u16,
}

impl Default for BlockRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockRegistry {
    // Creates a new, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            blocks_by_id: Vec::new(),
            blocks_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
            state_to_block_lookup: Vec::new(),
            state_to_block_id: Vec::new(),
            block_to_base_state: Vec::new(),
            next_state_id: 0,
        }
    }

    pub fn register(&mut self, block: BlockRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register blocks after the registry has been frozen"
        );

        let id = self.blocks_by_id.len();
        let base_state_id = self.next_state_id;

        let cached = block.id.get_or_init(|| id);
        assert_eq!(*cached, id, "block registered with conflicting id");
        self.blocks_by_key.insert(block.key.clone(), id);
        self.blocks_by_id.push(block);
        self.block_to_base_state.push(base_state_id);

        let state_count = block.state_count();
        for _ in 0..state_count {
            self.state_to_block_lookup.push(block);
            self.state_to_block_id.push(id);
        }

        self.next_state_id += state_count;

        id
    }

    fn try_block_index(&self, block: BlockRef) -> Option<usize> {
        if let Some(id) = block.id.get().copied()
            && self
                .blocks_by_id
                .get(id)
                .is_some_and(|registered| *registered == block)
        {
            return Some(id);
        }

        self.blocks_by_key.get(&block.key).copied()
    }

    fn block_index(&self, block: BlockRef) -> usize {
        let Some(id) = self.try_block_index(block) else {
            panic!("Block not found");
        };
        id
    }

    #[must_use]
    pub fn get_base_state_id(&self, block: BlockRef) -> BlockStateId {
        let id = self.block_index(block);
        BlockStateId(self.block_to_base_state[id])
    }

    /// Gets the default state ID for a block (base state + default offset)
    #[must_use]
    pub fn get_default_state_id(&self, block: BlockRef) -> BlockStateId {
        let id = self.block_index(block);
        let base = self.block_to_base_state[id];
        BlockStateId(base + block.default_state_offset)
    }

    #[must_use]
    pub fn by_state_id(&self, state_id: BlockStateId) -> Option<BlockRef> {
        self.state_to_block_lookup.get(state_id.0 as usize).copied()
    }

    #[must_use]
    pub fn get_properties(&self, id: BlockStateId) -> Vec<(&'static str, &'static str)> {
        let block = self.by_state_id(id).expect("Invalid state ID");

        // If block has no properties, return empty vec
        if block.properties.is_empty() {
            return Vec::new();
        }

        // Get the base state ID for this block (O(1) lookup)
        let block_id = self.state_to_block_id[id.0 as usize];
        let base_state_id = self.block_to_base_state[block_id];

        // Calculate the relative state index
        let relative_index = id.0 - base_state_id;

        Self::decode_property_indices(block, relative_index)
            .into_iter()
            .zip(block.properties)
            .map(|(value_index, prop)| (prop.get_name(), prop.get_possible_values()[value_index]))
            .collect()
    }

    /// Gets the state ID for a block with the given properties.
    ///
    /// Returns `None` if the block key is unknown or if any property name/value is invalid.
    ///
    /// Properties can be provided in any order. Missing properties will use the block's
    /// default values (typically index 0 for each property).
    #[must_use]
    pub fn state_id_from_properties(
        &self,
        key: &Identifier,
        properties: &[(&str, &str)],
    ) -> Option<BlockStateId> {
        let block = self.by_key(key)?;
        self.state_id_from_block_properties(block, properties)
    }

    /// Gets the state ID for a block with the given properties.
    ///
    /// Returns `None` if the block is not registered or if any property
    /// name/value is invalid.
    #[must_use]
    pub fn state_id_from_block_properties(
        &self,
        block: BlockRef,
        properties: &[(&str, &str)],
    ) -> Option<BlockStateId> {
        let block_id = self.try_block_index(block)?;
        let base_state_id = self.block_to_base_state[block_id];

        let mut property_indices = vec![0usize; block.properties.len()];
        Self::apply_property_overrides(block, &mut property_indices, properties.iter().copied())?;

        Some(BlockStateId(
            base_state_id + Self::encode_property_indices(block, &property_indices),
        ))
    }

    /// Gets the state ID for a block by applying properties over that block's
    /// registered default state.
    ///
    /// Returns `None` if the block is not registered or if any property
    /// name/value is invalid.
    #[must_use]
    pub fn state_id_from_block_defaulted_properties<'a>(
        &self,
        block: BlockRef,
        properties: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Option<BlockStateId> {
        let block_id = self.try_block_index(block)?;
        let base_state_id = self.block_to_base_state[block_id];

        let mut property_indices = Self::decode_property_indices(block, block.default_state_offset);
        Self::apply_property_overrides(block, &mut property_indices, properties)?;

        Some(BlockStateId(
            base_state_id + Self::encode_property_indices(block, &property_indices),
        ))
    }

    /// Returns every state id of `block` whose properties match all `(name, value)` pairs
    /// in `filter`. An empty filter yields all states of the block (vanilla's
    /// `getStatesOfBlock`); a non-empty filter keeps only matching states (vanilla's
    /// `BED_HEADS`-style predicate).
    #[must_use]
    pub fn matching_states(&self, block: BlockRef, filter: &[(&str, &str)]) -> Vec<BlockStateId> {
        let block_id = self.block_index(block);
        let base_state_id = self.block_to_base_state[block_id];

        (0..block.state_count())
            .map(|offset| BlockStateId(base_state_id + offset))
            .filter(|&state_id| {
                let properties = self.get_properties(state_id);
                filter
                    .iter()
                    .all(|(name, value)| properties.iter().any(|(n, v)| n == name && v == value))
            })
            .collect()
    }

    fn decode_property_indices(block: BlockRef, mut offset: u16) -> Vec<usize> {
        let mut property_indices = vec![0; block.properties.len()];

        for (i, prop) in block.properties.iter().enumerate().rev() {
            let count = prop.get_possible_values().len() as u16;
            property_indices[i] = (offset % count) as usize;
            offset /= count;
        }

        property_indices
    }

    fn apply_property_overrides<'a>(
        block: BlockRef,
        property_indices: &mut [usize],
        properties: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Option<()> {
        for (prop_name, prop_value) in properties {
            let prop_idx = block
                .properties
                .iter()
                .position(|p| p.get_name() == prop_name)?;

            let prop = block.properties[prop_idx];
            let value_idx = prop
                .get_possible_values()
                .iter()
                .position(|v| *v == prop_value)?;

            property_indices[prop_idx] = value_idx;
        }

        Some(())
    }

    fn encode_property_indices(block: BlockRef, property_indices: &[usize]) -> u16 {
        let mut offset = 0u16;
        let mut multiplier = 1u16;
        for (idx, prop) in property_indices.iter().zip(block.properties.iter()).rev() {
            offset += *idx as u16 * multiplier;
            multiplier *= prop.get_possible_values().len() as u16;
        }

        offset
    }

    // Panics if that property isn't supposed to be on this block.
    pub fn get_property<T, P: Property<T>>(&self, id: BlockStateId, property: &P) -> T {
        self.try_get_property(id, property)
            .expect("Property not found on this block")
    }

    /// Gets the value of a property, returning `None` if the block doesn't have this property.
    #[must_use]
    pub fn try_get_property<T, P: Property<T>>(&self, id: BlockStateId, property: &P) -> Option<T> {
        let block = self.by_state_id(id).expect("Invalid state ID");

        // Find the property index in the block's property list
        let property_index = block
            .properties
            .iter()
            .position(|prop| prop.get_name() == property.as_dyn().get_name())?;

        // Get the base state ID for this block (O(1) lookup)
        let block_id = self.state_to_block_id[id.0 as usize];
        let base_state_id = self.block_to_base_state[block_id];

        // Calculate the relative state index
        let relative_index = id.0 - base_state_id;

        let property_indices = Self::decode_property_indices(block, relative_index);
        let block_property = block.properties[property_index];
        let block_values = block_property.get_possible_values();
        let block_value = block_values[property_indices[property_index]];

        property.get_value(block_value)
    }

    // Panics if that property isn't supposed to be on this block.
    pub fn set_property<T, P: Property<T>>(
        &self,
        id: BlockStateId,
        property: &P,
        value: T,
    ) -> BlockStateId {
        let block = self.by_state_id(id).expect("Invalid state ID");

        // Find the property index in the block's property list
        let property_index = block
            .properties
            .iter()
            .position(|prop| prop.get_name() == property.as_dyn().get_name())
            .unwrap_or_else(|| {
                panic!(
                    "Property {} not found on block {}",
                    property.as_dyn().get_name(),
                    block.key
                )
            });

        // Get the base state ID for this block (O(1) lookup)
        let block_id = self.state_to_block_id[id.0 as usize];
        let base_state_id = self.block_to_base_state[block_id];

        // Calculate the relative state index
        let relative_index = id.0 - base_state_id;

        // Decode all property indices from the relative state index.
        // Properties are decoded in reverse order (last property = inner loop).
        let mut index = relative_index;
        let mut property_indices = vec![0usize; block.properties.len()];

        for (i, prop) in block.properties.iter().enumerate().rev() {
            let count = prop.get_possible_values().len() as u16;
            property_indices[i] = (index % count) as usize;
            index /= count;
        }

        let caller_value_index = property.get_internal_index(&value);
        let caller_values = property.as_dyn().get_possible_values();
        let value_name = caller_values[caller_value_index];
        let block_values = block.properties[property_index].get_possible_values();
        let Some(new_value_index) = block_values.iter().position(|v| *v == value_name) else {
            panic!(
                "Value {} for property {} not found on block {}",
                value_name,
                property.as_dyn().get_name(),
                block.key
            );
        };
        property_indices[property_index] = new_value_index;

        // Re-encode the property indices back to a state ID.
        // Properties are processed in reverse order (last property = inner loop).
        let mut new_relative_index = 0u16;
        let mut multiplier = 1u16;
        for (i, prop) in block.properties.iter().enumerate().rev() {
            let count = prop.get_possible_values().len() as u16;
            new_relative_index += property_indices[i] as u16 * multiplier;
            multiplier *= count;
        }

        BlockStateId(base_state_id + new_relative_index)
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, BlockRef)> + '_ {
        self.blocks_by_id
            .iter()
            .enumerate()
            .map(|(id, &block)| (id, block))
    }
}

crate::impl_registry_ext!(BlockRegistry, Block, blocks_by_id, blocks_by_key);

crate::impl_registry_entry_eq!(Block);

impl crate::RegistryEntry for Block {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}
crate::impl_tagged_registry!(BlockRegistry, blocks_by_key, "block");

// Shape lookup methods
impl BlockRegistry {
    fn block_and_state_offset(&self, state_id: BlockStateId) -> Option<(BlockRef, u16)> {
        let block = self
            .state_to_block_lookup
            .get(state_id.0 as usize)
            .copied()?;
        let block_id = self
            .state_to_block_id
            .get(state_id.0 as usize)
            .copied()
            .unwrap_or(0);
        let base_state = self.block_to_base_state.get(block_id).copied().unwrap_or(0);
        let offset = state_id.0.saturating_sub(base_state);
        Some((block, offset))
    }

    fn static_shape_for_state(
        &self,
        state_id: BlockStateId,
        shape: fn(&Block, u16) -> shapes::VoxelShape,
    ) -> shapes::VoxelShape {
        let Some((block, offset)) = self.block_and_state_offset(state_id) else {
            return shapes::VoxelShape::FULL_BLOCK;
        };
        shape(block, offset)
    }

    fn offset_shape_for_state(
        &self,
        state_id: BlockStateId,
        pos: BlockPos,
        channel: ShapeChannel,
        shape: fn(&Block, u16) -> shapes::VoxelShape,
    ) -> shapes::OffsetVoxelShape {
        let Some((block, offset)) = self.block_and_state_offset(state_id) else {
            return shapes::OffsetVoxelShape::without_offset(shapes::VoxelShape::FULL_BLOCK);
        };

        let shape = shape(block, offset);
        let offset = if block.shape_offsets.uses_offset(channel) {
            block.offset_at(pos)
        } else {
            DVec3::ZERO
        };
        shapes::OffsetVoxelShape::new(shape, offset)
    }

    /// Gets the collision shape for a block state.
    ///
    /// For simple blocks this is typically a single full-block box.
    /// For complex blocks like fences, this may be multiple boxes.
    #[must_use]
    pub fn get_static_collision_shape(&self, state_id: BlockStateId) -> shapes::VoxelShape {
        self.static_shape_for_state(state_id, Block::get_collision_shape)
    }

    /// Returns vanilla `BlockState.isSuffocating`.
    #[must_use]
    pub fn is_suffocating(&self, state_id: BlockStateId) -> bool {
        let Some((block, offset)) = self.block_and_state_offset(state_id) else {
            return false;
        };
        block.suffocating.value(offset)
    }

    #[must_use]
    pub fn get_collision_shape_at(
        &self,
        state_id: BlockStateId,
        pos: BlockPos,
    ) -> shapes::OffsetVoxelShape {
        self.offset_shape_for_state(
            state_id,
            pos,
            ShapeChannel::Collision,
            Block::get_collision_shape,
        )
    }

    /// Gets the block support shape for a block state.
    ///
    /// Vanilla support checks use `BlockState.getBlockSupportShape`, not collision shape,
    /// for `isFaceSturdy` and multiface side attachment.
    #[must_use]
    pub fn get_static_support_shape(&self, state_id: BlockStateId) -> shapes::VoxelShape {
        self.static_shape_for_state(state_id, Block::get_support_shape)
    }

    #[must_use]
    pub fn get_support_shape_at(
        &self,
        state_id: BlockStateId,
        pos: BlockPos,
    ) -> shapes::OffsetVoxelShape {
        self.offset_shape_for_state(
            state_id,
            pos,
            ShapeChannel::Support,
            Block::get_support_shape,
        )
    }

    /// Gets the outline shape for a block state.
    ///
    /// This is the shape shown when the player targets the block.
    /// Often the same as collision shape, but can differ (e.g., fences).
    #[must_use]
    pub fn get_static_outline_shape(&self, state_id: BlockStateId) -> shapes::VoxelShape {
        self.static_shape_for_state(state_id, Block::get_outline_shape)
    }

    #[must_use]
    pub fn get_outline_shape_at(
        &self,
        state_id: BlockStateId,
        pos: BlockPos,
    ) -> shapes::OffsetVoxelShape {
        self.offset_shape_for_state(
            state_id,
            pos,
            ShapeChannel::Outline,
            Block::get_outline_shape,
        )
    }

    /// Gets the occlusion shape for a block state.
    ///
    /// Vanilla caches this as `BlockState.getOcclusionShape()` and uses it for
    /// `isSolidRender`, light occlusion, and face occlusion.
    #[must_use]
    pub fn get_occlusion_shape(&self, state_id: BlockStateId) -> shapes::VoxelShape {
        self.static_shape_for_state(state_id, Block::get_occlusion_shape)
    }

    /// Gets the interaction shape for a block state.
    ///
    /// Vanilla uses this as an interaction hit override after the primary raycast
    /// shape has already hit.
    #[must_use]
    pub fn get_static_interaction_shape(&self, state_id: BlockStateId) -> shapes::VoxelShape {
        self.static_shape_for_state(state_id, Block::get_interaction_shape)
    }

    #[must_use]
    pub fn get_interaction_shape_at(
        &self,
        state_id: BlockStateId,
        pos: BlockPos,
    ) -> shapes::OffsetVoxelShape {
        self.offset_shape_for_state(
            state_id,
            pos,
            ShapeChannel::Interaction,
            Block::get_interaction_shape,
        )
    }

    /// Gets the visual shape for a block state.
    ///
    /// Vanilla uses this for visual raycasts; it defaults to collision shape but
    /// differs for a few blocks such as fences, mud, soul sand, and powder snow.
    #[must_use]
    pub fn get_static_visual_shape(&self, state_id: BlockStateId) -> shapes::VoxelShape {
        self.static_shape_for_state(state_id, Block::get_visual_shape)
    }

    #[must_use]
    pub fn get_visual_shape_at(
        &self,
        state_id: BlockStateId,
        pos: BlockPos,
    ) -> shapes::OffsetVoxelShape {
        self.offset_shape_for_state(state_id, pos, ShapeChannel::Visual, Block::get_visual_shape)
    }

    /// Gets all static shape channels for a block state.
    #[must_use]
    pub fn get_static_shapes(&self, state_id: BlockStateId) -> shapes::BlockShapes {
        shapes::BlockShapes::new(
            self.get_static_collision_shape(state_id),
            self.get_static_support_shape(state_id),
            self.get_static_outline_shape(state_id),
            self.get_occlusion_shape(state_id),
            self.get_static_interaction_shape(state_id),
            self.get_static_visual_shape(state_id),
        )
    }

    #[must_use]
    pub fn get_light_properties(&self, state_id: BlockStateId) -> BlockLightProperties {
        let Some((block, offset)) = self.block_and_state_offset(state_id) else {
            return BlockLightProperties::OPAQUE_FULL_BLOCK;
        };
        block.get_light_properties(offset)
    }

    pub fn copy_matching_properties(&self, source: BlockStateId, target: BlockRef) -> BlockStateId {
        let props = self.get_properties(source);
        let matching: Vec<(&str, &str)> = props
            .iter()
            .filter(|(name, _)| target.properties.iter().any(|p| p.get_name() == *name))
            .copied()
            .collect();
        self.state_id_from_block_properties(target, &matching)
            .unwrap_or_else(|| self.get_default_state_id(target))
    }
}

/// Macro to generate offset calculation from property values in all positions.
///
/// Takes property objects and their values, automatically converts to indices.
/// All properties must be specified in order.
///
/// # Note
/// For boolean properties, use `.index_of(value)` to handle the inverted encoding
/// (true=0, false=1 for Java compatibility).
///
/// # Example
/// ```ignore
/// use steel_registry::{offset, properties::{BlockStateProperties as Props, RedstoneSide}};
///
/// const WIRE: Block = Block::new("wire", behavior, PROPS)
///     .with_default_state(offset!(
///         Props::EAST_REDSTONE => RedstoneSide::Up,
///         Props::NORTH_REDSTONE => RedstoneSide::None,
///         Props::POWER => 10,
///         Props::ATTACHED => Props::ATTACHED.index_of(false)  // Bools need .index_of()
///     ));
/// ```
#[macro_export]
macro_rules! offset {
    ($($prop:expr => $value:expr),* $(,)?) => {{
        const INDICES: &[usize] = &[$($value as usize),*];
        const COUNTS: &[usize] = &[$($prop.value_count()),*];
        $crate::blocks::Block::calculate_offset(INDICES, COUNTS)
    }};
}

/// Re-export for easier access
pub use offset;
use steel_utils::Identifier;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::properties::{BlockStateProperties, Direction};
    use crate::vanilla_blocks;

    fn create_test_registry() -> BlockRegistry {
        let mut registry = BlockRegistry::new();
        vanilla_blocks::register_blocks(&mut registry);
        registry.freeze();
        registry
    }

    #[test]
    fn test_redstone_wire_properties() {
        let registry = create_test_registry();
        let redstone_wire = registry
            .by_key(&Identifier::vanilla_static("redstone_wire"))
            .expect("redstone_wire should exist");

        // Redstone wire has 5 properties
        assert_eq!(redstone_wire.properties.len(), 5);

        // Check property names
        let prop_names: Vec<&str> = redstone_wire
            .properties
            .iter()
            .map(|p| p.get_name())
            .collect();
        assert!(prop_names.contains(&"east"));
        assert!(prop_names.contains(&"north"));
        assert!(prop_names.contains(&"south"));
        assert!(prop_names.contains(&"west"));
        assert!(prop_names.contains(&"power"));
    }

    #[test]
    fn test_redstone_wire_state_count() {
        let registry = create_test_registry();

        // Redstone wire: 3 sides × 3 sides × 3 sides × 3 sides × 16 power levels = 1296 states
        // Actually checking the state count
        let redstone_wire = registry
            .by_key(&Identifier::vanilla_static("redstone_wire"))
            .expect("redstone_wire should exist");

        let mut state_count = 1;
        for prop in redstone_wire.properties {
            state_count *= prop.get_possible_values().len();
        }
        assert_eq!(state_count, 3 * 3 * 3 * 3 * 16); // 1296
    }

    #[test]
    fn test_get_properties_default_state() {
        let registry = create_test_registry();
        let redstone_wire = registry
            .by_key(&Identifier::vanilla_static("redstone_wire"))
            .expect("redstone_wire should exist");

        let default_state = registry.get_default_state_id(redstone_wire);
        let properties = registry.get_properties(default_state);

        // Default state should have all sides "none" and power 0
        assert_eq!(properties.len(), 5);

        for (name, value) in &properties {
            match *name {
                "east" | "north" | "south" | "west" => {
                    assert_eq!(*value, "none", "Default side should be 'none'");
                }
                "power" => {
                    assert_eq!(*value, "0", "Default power should be '0'");
                }
                _ => panic!("Unexpected property: {}", name),
            }
        }
    }

    #[test]
    fn test_state_id_from_properties_roundtrip() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("redstone_wire");

        // Test with specific properties
        let properties = [
            ("east", "up"),
            ("north", "side"),
            ("south", "none"),
            ("west", "up"),
            ("power", "15"),
        ];

        let state_id = registry
            .state_id_from_properties(&key, &properties)
            .expect("Should find state");

        // Get properties back and verify
        let retrieved = registry.get_properties(state_id);
        assert_eq!(retrieved.len(), 5);

        for (name, value) in &properties {
            let found = retrieved
                .iter()
                .find(|(n, _)| n == name)
                .expect("Property should exist");
            assert_eq!(found.1, *value, "Property {} mismatch", name);
        }
    }

    #[test]
    fn test_state_id_from_properties_partial() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("redstone_wire");

        // Only specify some properties - others should default to index 0
        let partial_props = [("power", "10"), ("east", "side")];

        let state_id = registry
            .state_id_from_properties(&key, &partial_props)
            .expect("Should find state");

        let retrieved = registry.get_properties(state_id);

        // Verify specified properties
        let power = retrieved.iter().find(|(n, _)| *n == "power").unwrap();
        assert_eq!(power.1, "10");

        let east = retrieved.iter().find(|(n, _)| *n == "east").unwrap();
        assert_eq!(east.1, "side");

        // Unspecified properties should be at index 0 (first value in enum)
        let north = retrieved.iter().find(|(n, _)| *n == "north").unwrap();
        assert_eq!(north.1, "up"); // Index 0 is "up" for RedstoneSide
    }

    #[test]
    fn test_state_id_from_block_defaulted_properties_keeps_missing_defaults() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("redstone_wire");
        let block = registry.by_key(&key).expect("redstone_wire should exist");

        let state_id = registry
            .state_id_from_block_defaulted_properties(block, [("power", "10")])
            .expect("Should find state");

        let retrieved = registry.get_properties(state_id);

        let power = retrieved.iter().find(|(n, _)| *n == "power").unwrap();
        assert_eq!(power.1, "10");

        for direction in ["east", "north", "south", "west"] {
            let side = retrieved.iter().find(|(n, _)| *n == direction).unwrap();
            assert_eq!(side.1, "none");
        }
    }

    #[test]
    fn test_state_id_from_properties_empty() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("redstone_wire");

        // Empty properties - should get base state with all defaults at index 0
        let state_id = registry
            .state_id_from_properties(&key, &[])
            .expect("Should find state");

        let retrieved = registry.get_properties(state_id);

        // All should be at index 0
        for (name, value) in &retrieved {
            match *name {
                "east" | "north" | "south" | "west" => {
                    assert_eq!(*value, "up", "Empty props should use index 0 = 'up'");
                }
                "power" => {
                    assert_eq!(*value, "0", "Empty props should use index 0 = '0'");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_state_id_from_properties_invalid_block() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("nonexistent_block");

        let result = registry.state_id_from_properties(&key, &[]);
        assert!(result.is_none(), "Should return None for invalid block");
    }

    #[test]
    fn test_state_id_from_properties_invalid_property() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("redstone_wire");

        let invalid_props = [("invalid_property", "value")];
        let result = registry.state_id_from_properties(&key, &invalid_props);
        assert!(result.is_none(), "Should return None for invalid property");
    }

    #[test]
    fn test_state_id_from_properties_invalid_value() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("redstone_wire");

        let invalid_props = [("power", "999")]; // Power only goes 0-15
        let result = registry.state_id_from_properties(&key, &invalid_props);
        assert!(result.is_none(), "Should return None for invalid value");
    }

    #[test]
    fn same_named_direction_properties_translate_by_value_name() {
        let registry = create_test_registry();
        let wall_torch = registry.get_default_state_id(&vanilla_blocks::WALL_TORCH);

        let south_torch = registry.set_property(
            wall_torch,
            &BlockStateProperties::HORIZONTAL_FACING,
            Direction::South,
        );
        let facing_from_six_way_property =
            registry.try_get_property(south_torch, &BlockStateProperties::FACING);
        assert_eq!(facing_from_six_way_property, Some(Direction::South));

        let west_torch =
            registry.set_property(south_torch, &BlockStateProperties::FACING, Direction::West);
        let facing_from_horizontal_property =
            registry.try_get_property(west_torch, &BlockStateProperties::HORIZONTAL_FACING);
        assert_eq!(facing_from_horizontal_property, Some(Direction::West));

        let dispenser = registry.get_default_state_id(&vanilla_blocks::DISPENSER);
        let upward_dispenser =
            registry.set_property(dispenser, &BlockStateProperties::FACING, Direction::Up);
        let horizontal_facing =
            registry.try_get_property(upward_dispenser, &BlockStateProperties::HORIZONTAL_FACING);
        assert_eq!(horizontal_facing, None);
    }

    #[test]
    fn test_state_id_from_properties_rejects_properties_on_propertyless_block() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("stone");
        let stone = registry.by_key(&key).expect("stone should exist");

        let result = registry.state_id_from_properties(&key, &[("power", "1")]);
        assert!(
            result.is_none(),
            "Should return None for invalid property on propertyless block"
        );

        let result = registry.state_id_from_block_defaulted_properties(stone, [("power", "1")]);
        assert!(
            result.is_none(),
            "Should return None for invalid defaulted property on propertyless block"
        );
    }

    #[test]
    fn test_stone_no_properties() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("stone");

        // Stone has no properties
        let stone = registry.by_key(&key).expect("stone should exist");
        assert!(stone.properties.is_empty());

        // Should still work with empty properties
        let state_id = registry
            .state_id_from_properties(&key, &[])
            .expect("Should find state");

        let retrieved = registry.get_properties(state_id);
        assert!(retrieved.is_empty());
    }

    #[test]
    fn test_all_redstone_power_levels() {
        let registry = create_test_registry();
        let key = Identifier::vanilla_static("redstone_wire");

        // Test all 16 power levels
        for power in 0..=15 {
            let power_str = power.to_string();
            let props = [("power", power_str.as_str())];

            let state_id = registry
                .state_id_from_properties(&key, &props)
                .unwrap_or_else(|| panic!("Should find state for power {}", power));

            let retrieved = registry.get_properties(state_id);
            let found_power = retrieved.iter().find(|(n, _)| *n == "power").unwrap();
            assert_eq!(
                found_power.1,
                power_str.as_str(),
                "Power level {} mismatch",
                power
            );
        }
    }

    #[test]
    #[cfg(feature = "minecraft-src")]
    fn test_all_block_state_ids_match_minecraft() {
        use rustc_hash::FxHashMap as HashMap;
        use std::fs;

        #[derive(serde::Deserialize)]
        struct BlockState {
            id: u16,
            #[serde(default)]
            properties: HashMap<String, String>,
            #[serde(default)]
            default: bool,
        }

        #[derive(serde::Deserialize)]
        struct BlockData {
            states: Vec<BlockState>,
        }

        // Try multiple paths to find blocks.json
        let possible_paths = [
            "minecraft-src/minecraft/resources/datagen-reports/blocks.json",
            "../minecraft-src/minecraft/resources/datagen-reports/blocks.json",
        ];
        let json_content = possible_paths
            .iter()
            .find_map(|path| fs::read_to_string(path).ok())
            .expect("Failed to read blocks.json - make sure minecraft-src is available");
        let blocks: HashMap<String, BlockData> =
            serde_json::from_str(&json_content).expect("Failed to parse blocks.json");

        let registry = create_test_registry();
        let mut errors = Vec::new();

        for (block_name, block_data) in &blocks {
            // Strip "minecraft:" prefix
            let key = Identifier::vanilla_static(
                block_name
                    .strip_prefix("minecraft:")
                    .unwrap_or(block_name)
                    .to_string()
                    .leak(),
            );

            let Some(block) = registry.by_key(&key) else {
                errors.push(format!("Block {} not found in registry", block_name));
                continue;
            };

            // Verify default state
            for state in &block_data.states {
                if state.default {
                    let our_default = registry.get_default_state_id(block);
                    if our_default.0 != state.id {
                        errors.push(format!(
                            "{}: default state mismatch - expected {}, got {}",
                            block_name, state.id, our_default.0
                        ));
                    }
                }
            }

            // Verify all states
            for state in &block_data.states {
                let props: Vec<(&str, &str)> = state
                    .properties
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();

                let Some(our_state_id) = registry.state_id_from_properties(&key, &props) else {
                    errors.push(format!(
                        "{}: failed to get state for properties {:?}",
                        block_name, props
                    ));
                    continue;
                };

                if our_state_id.0 != state.id {
                    errors.push(format!(
                        "{}: state mismatch for {:?} - expected {}, got {}",
                        block_name, props, state.id, our_state_id.0
                    ));
                }
            }
        }

        if !errors.is_empty() {
            // Print first 20 errors for readability
            let display_errors: String = errors
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            panic!(
                "Found {} state ID mismatches:\n{}{}",
                errors.len(),
                display_errors,
                if errors.len() > 20 {
                    format!("\n... and {} more", errors.len() - 20)
                } else {
                    String::new()
                }
            );
        }
    }
}
