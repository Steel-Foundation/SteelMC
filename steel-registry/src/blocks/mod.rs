pub mod behaviour;
pub mod block_state_ext;
pub mod properties;
pub mod shapes;

use rustc_hash::FxHashMap;

use crate::RegistryExt;
use crate::blocks::behaviour::BlockConfig;
use crate::blocks::properties::{DynProperty, Property};

/// Function type for shape lookups. Takes a state offset and returns the shape.
pub type ShapeFn = fn(u16) -> &'static [shapes::AABB];

pub struct Block {
    pub key: Identifier,
    pub config: BlockConfig,
    pub properties: &'static [&'static dyn DynProperty],
    pub default_state_offset: u16,
    /// Function to get collision shape for a state offset
    pub collision_shape: ShapeFn,
    /// Function to get outline shape for a state offset
    pub outline_shape: ShapeFn,
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
const fn full_block_shape(_offset: u16) -> &'static [shapes::AABB] {
    &[shapes::AABB::FULL_BLOCK]
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
            collision_shape: full_block_shape,
            outline_shape: full_block_shape,
        }
    }

    /// Sets the shape functions for this block.
    pub const fn with_shapes(mut self, collision: ShapeFn, outline: ShapeFn) -> Self {
        self.collision_shape = collision;
        self.outline_shape = outline;
        self
    }

    /// Gets the collision shape for a given state offset.
    #[inline]
    pub fn get_collision_shape(&self, offset: u16) -> &'static [shapes::AABB] {
        (self.collision_shape)(offset)
    }

    /// Gets the outline shape for a given state offset.
    #[inline]
    pub fn get_outline_shape(&self, offset: u16) -> &'static [shapes::AABB] {
        (self.outline_shape)(offset)
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

        self.blocks_by_key.insert(block.key.clone(), id);
        self.blocks_by_id.push(block);
        self.block_to_base_state.push(base_state_id);

        let mut state_count = 1;
        for property in block.properties {
            state_count *= property.get_possible_values().len();
        }

        for _ in 0..state_count {
            self.state_to_block_lookup.push(block);
            self.state_to_block_id.push(id);
        }

        self.next_state_id += state_count as u16;

        id
    }

    /// Replaces a block at a given index.
    /// Returns true if the block was replaced and false if the block wasn't replaced
    #[must_use]
    pub fn replace(&mut self, block: BlockRef, id: usize) -> bool {
        if id >= self.blocks_by_id.len() {
            return false;
        }
        self.blocks_by_id[id] = block;
        true
    }

    #[must_use]
    pub fn get_base_state_id(&self, block: BlockRef) -> BlockStateId {
        BlockStateId(self.block_to_base_state[*self.get_id(block)])
    }

    /// Gets the default state ID for a block (base state + default offset)
    #[must_use]
    pub fn get_default_state_id(&self, block: BlockRef) -> BlockStateId {
        let base = self.block_to_base_state[*self.get_id(block)];
        BlockStateId(base + block.default_state_offset)
    }

    // Retrieves a block by its ID.
    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<BlockRef> {
        self.blocks_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, block: BlockRef) -> &usize {
        self.blocks_by_key.get(&block.key).expect("Block not found")
    }

    #[must_use]
    pub fn by_state_id(&self, state_id: BlockStateId) -> Option<BlockRef> {
        self.state_to_block_lookup.get(state_id.0 as usize).copied()
    }

    // Retrieves a block by its name.
    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<BlockRef> {
        self.blocks_by_key.get(key).and_then(|id| self.by_id(*id))
    }

    #[must_use]
    pub fn get_properties(&self, id: BlockStateId) -> Vec<(&str, &str)> {
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

        // Decode the property indices from the relative state index.
        // Properties are decoded in reverse order (last property = inner loop).
        let mut index = relative_index;
        let mut property_values = vec![("", ""); block.properties.len()];

        for (i, prop) in block.properties.iter().enumerate().rev() {
            let count = prop.get_possible_values().len() as u16;
            let current_index = (index % count) as usize;

            let possible_values = prop.get_possible_values();
            property_values[i] = (prop.get_name(), possible_values[current_index]);

            index /= count;
        }

        property_values
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
        let block_id = *self.blocks_by_key.get(key)?;
        let base_state_id = self.block_to_base_state[block_id];

        // If no properties, just return base state
        if block.properties.is_empty() {
            return Some(BlockStateId(base_state_id));
        }

        // Build property indices (start with defaults = 0)
        let mut property_indices = vec![0usize; block.properties.len()];

        // Apply provided properties
        for (prop_name, prop_value) in properties {
            // Find this property in the block's property list
            let prop_idx = block
                .properties
                .iter()
                .position(|p| p.get_name() == *prop_name)?;

            // Find the value index
            let prop = block.properties[prop_idx];
            let value_idx = prop
                .get_possible_values()
                .iter()
                .position(|v| *v == *prop_value)?;

            property_indices[prop_idx] = value_idx;
        }

        // Encode property indices to state offset.
        // Properties are processed in reverse order (last property = inner loop).
        let mut offset = 0u16;
        let mut multiplier = 1u16;
        for (idx, prop) in property_indices.iter().zip(block.properties.iter()).rev() {
            offset += *idx as u16 * multiplier;
            multiplier *= prop.get_possible_values().len() as u16;
        }

        Some(BlockStateId(base_state_id + offset))
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

        // Decode the property indices from the relative state index.
        // Properties are decoded in reverse order (last property = inner loop).
        let mut index = relative_index;
        let mut property_value_index = 0;

        for (i, prop) in block.properties.iter().enumerate().rev() {
            let count = prop.get_possible_values().len() as u16;
            let current_index = (index % count) as usize;

            if i == property_index {
                property_value_index = current_index;
            }

            index /= count;
        }

        // Convert the index back to the actual value
        Some(property.value_from_index(property_value_index))
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

        // Update the specific property's index
        let new_value_index = property.get_internal_index(&value);
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

    #[must_use]
    pub fn len(&self) -> usize {
        self.blocks_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks_by_id.is_empty()
    }

    // Tag-related methods

    /// Registers a tag with a list of block keys.
    /// Block keys that don't exist in the registry are silently skipped.
    pub fn register_tag(&mut self, tag: Identifier, block_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifier: Vec<Identifier> = block_keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                // Only include if the block actually exists
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifier);
    }

    /// Gives the access to all blocks to delete and add new entries
    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let new_items = f(existing)
            .into_iter()
            .filter(|block| {
                let exists = self.blocks_by_key.contains_key(block);
                if !exists {
                    tracing::error!("block {block} not found in registry, skipping from tag {tag}");
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), new_items);
    }

    /// Checks if a block is in a given tag.
    #[must_use]
    pub fn is_in_tag(&self, block: BlockRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|ident| ident.contains(&block.key))
    }

    /// Gets all blocks in a tag.
    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<BlockRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    /// Iterates over all blocks in a tag.
    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = BlockRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    /// Gets all tag keys.
    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> + '_ {
        self.tags.keys()
    }
}

impl RegistryExt for BlockRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

// Shape lookup methods
impl BlockRegistry {
    /// Gets the collision shape for a block state.
    ///
    /// Returns a slice of AABBs that make up the collision shape.
    /// For simple blocks this is typically a single full-block AABB.
    /// For complex blocks like fences, this may be multiple AABBs.
    #[must_use]
    pub fn get_collision_shape(&self, state_id: BlockStateId) -> &'static [shapes::AABB] {
        let block = self.state_to_block_lookup.get(state_id.0 as usize).copied();
        let Some(block) = block else {
            return &[shapes::AABB::FULL_BLOCK];
        };
        let block_id = self
            .state_to_block_id
            .get(state_id.0 as usize)
            .copied()
            .unwrap_or(0);
        let base_state = self.block_to_base_state.get(block_id).copied().unwrap_or(0);
        let offset = state_id.0.saturating_sub(base_state);
        block.get_collision_shape(offset)
    }

    /// Gets the outline shape for a block state.
    ///
    /// This is the shape shown when the player targets the block.
    /// Often the same as collision shape, but can differ (e.g., fences).
    #[must_use]
    pub fn get_outline_shape(&self, state_id: BlockStateId) -> &'static [shapes::AABB] {
        let block = self.state_to_block_lookup.get(state_id.0 as usize).copied();
        let Some(block) = block else {
            return &[shapes::AABB::FULL_BLOCK];
        };
        let block_id = self
            .state_to_block_id
            .get(state_id.0 as usize)
            .copied()
            .unwrap_or(0);
        let base_state = self.block_to_base_state.get(block_id).copied().unwrap_or(0);
        let offset = state_id.0.saturating_sub(base_state);
        block.get_outline_shape(offset)
    }

    /// Gets both collision and outline shapes for a block state.
    #[must_use]
    pub fn get_shapes(&self, state_id: BlockStateId) -> shapes::BlockShapes {
        shapes::BlockShapes::new(
            self.get_collision_shape(state_id),
            self.get_outline_shape(state_id),
        )
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
/// const WIRE: Block = Block::new("wire", behaviour, PROPS)
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
use steel_utils::registry::registry_vanilla_or_custom_tag;
use steel_utils::{BlockStateId, Identifier};

#[cfg(test)]
mod tests {
    use super::*;
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
