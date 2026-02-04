//! Block behavior trait and registry.

use std::sync::Weak;

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::SharedBlockEntity;
use crate::player::Player;
use crate::world::World;

/// Trait defining the behavior of a block.
///
/// This trait handles all dynamic/functional aspects of blocks:
/// - Placement logic
/// - Neighbor updates
/// - Player interactions
/// - State changes
pub trait BlockBehaviour: Send + Sync {
    /// Called when a neighboring block changes shape.
    /// Returns the new state for this block after considering the neighbor change.
    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        state
    }

    /// Returns the block state to use when placing this block.
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId>;

    /// Called when this block is placed in the world.
    ///
    /// # Arguments
    /// * `state` - The new block state that was placed
    /// * `world` - The world the block was placed in
    /// * `pos` - The position where the block was placed
    /// * `old_state` - The previous block state at this position
    /// * `moved_by_piston` - Whether the block was moved by a piston
    #[allow(unused_variables)]
    fn on_place(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        old_state: BlockStateId,
        moved_by_piston: bool,
    ) {
        // Default: no-op
    }

    /// Called after this block is removed from the world, to affect neighbors.
    ///
    /// This is used for things like rails notifying neighbors when removed.
    ///
    /// # Arguments
    /// * `state` - The block state that was removed
    /// * `world` - The world the block was removed from
    /// * `pos` - The position where the block was removed
    /// * `moved_by_piston` - Whether the block was moved by a piston
    #[allow(unused_variables)]
    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        // Default: no-op
    }

    /// Called when a player uses an item on this block.
    ///
    /// Returns `TryEmptyHandInteraction` by default to fall through to item use.
    /// Override this to handle block-specific interactions (e.g., opening chests,
    /// using buttons, etc.).
    #[allow(unused_variables, clippy::too_many_arguments)]
    fn use_item_on(
        &self,
        item_stack: &ItemStack,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        player: &Player,
        hand: InteractionHand,
        hit_result: &BlockHitResult,
    ) -> InteractionResult {
        InteractionResult::TryEmptyHandInteraction
    }

    /// Called when a player uses this block without an item (or as a fallback
    /// when `use_item_on` returns `TryEmptyHandInteraction`).
    ///
    /// Returns `Pass` by default. Override this for blocks that have interactions
    /// without needing an item (e.g., buttons, levers, repeaters).
    #[allow(unused_variables)]
    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        player: &Player,
        hit_result: &BlockHitResult,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// Called when a neighboring block changes (not shape-related).
    ///
    /// This is the Rust equivalent of vanilla's `BlockState.handleNeighborChanged()`.
    /// Used by redstone components, doors, and other blocks that react to neighbor changes.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world
    /// * `pos` - Position of this block
    /// * `source_block` - The block type that changed
    /// * `moved_by_piston` - Whether the change was caused by a piston
    #[allow(unused_variables)]
    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        // Default: no-op
        // Override for redstone components, doors, etc.
    }

    /// Returns the item stack to give when a player picks this block (middle click).
    ///
    /// The default implementation looks up an item with the same key as the block.
    /// Override this for blocks where the pick item differs from the block key
    /// (e.g., crops → seeds, redstone wire → redstone dust, wall torch → torch).
    ///
    /// # Arguments
    /// * `block` - The block being picked
    /// * `_state` - The block state (some blocks vary pick item based on state)
    /// * `_include_data` - Whether to include block entity data (creative + Ctrl)
    #[allow(unused_variables)]
    fn get_clone_item_stack(
        &self,
        block: BlockRef,
        state: BlockStateId,
        include_data: bool,
    ) -> Option<ItemStack> {
        // Default: look up item by block's key
        REGISTRY.items.by_key(&block.key).map(ItemStack::new)
    }

    /// Returns whether this block should receive random ticks.
    ///
    /// Override to return true for blocks like crops, grass, ice, fire, etc.
    /// This is used to optimize chunk ticking by skipping sections with no
    /// randomly-ticking blocks.
    #[allow(unused_variables)]
    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        false
    }

    /// Called on random tick for blocks that support random ticking.
    ///
    /// This is only called if `is_randomly_ticking()` returns true.
    /// Used for crop growth, grass spread, ice melting, fire behavior, etc.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world the block is in
    /// * `pos` - The position of the block
    #[allow(unused_variables)]
    fn random_tick(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        // Default: no-op
    }

    // === Block Entity Methods ===

    /// Returns whether this block has an associated block entity.
    ///
    /// Override to return `true` for blocks like chests, furnaces, signs, etc.
    fn has_block_entity(&self) -> bool {
        false
    }

    /// Creates a new block entity for this block.
    ///
    /// Only called if `has_block_entity()` returns `true`.
    ///
    /// # Arguments
    /// * `level` - Weak reference to the world
    /// * `pos` - The position where the block entity will be placed
    /// * `state` - The block state for this block entity
    #[allow(unused_variables)]
    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        None
    }

    /// Returns whether the block entity should be kept when the block state changes.
    ///
    /// This is used when a block changes to a different block type that shares
    /// the same block entity type (e.g., different chest variants).
    ///
    /// # Arguments
    /// * `old_state` - The previous block state
    /// * `new_state` - The new block state
    #[allow(unused_variables)]
    fn should_keep_block_entity(&self, old_state: BlockStateId, new_state: BlockStateId) -> bool {
        false
    }

    // === Redstone / Comparator Methods ===

    /// Returns whether this block can provide an analog output signal to comparators.
    ///
    /// Override to return `true` for containers (chests, barrels, hoppers, etc.)
    /// and other blocks that comparators can read (composters, beehives, etc.).
    #[allow(unused_variables)]
    fn has_analog_output_signal(&self, state: BlockStateId) -> bool {
        false
    }

    /// Returns the analog output signal strength (0-15) for comparators.
    ///
    /// Only called if `has_analog_output_signal()` returns `true`.
    /// For containers, this is typically based on how full they are.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world
    /// * `pos` - The position of the block
    #[allow(unused_variables)]
    fn get_analog_output_signal(&self, state: BlockStateId, world: &World, pos: BlockPos) -> i32 {
        0
    }
}

/// Default block behavior that returns the block's default state for placement.
pub struct DefaultBlockBehaviour {
    block: BlockRef,
}

impl DefaultBlockBehaviour {
    /// Creates a new default block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for DefaultBlockBehaviour {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}

/// Registry for block behaviors.
///
/// Created after the main registry is frozen. All blocks are initialized with
/// default behaviors, then custom behaviors are registered for specific blocks.
pub struct BlockBehaviorRegistry {
    behaviors: Vec<Box<dyn BlockBehaviour>>,
}

impl BlockBehaviorRegistry {
    /// Creates a new behavior registry with default behaviors for all blocks.
    #[must_use]
    pub fn new() -> Self {
        let block_count = REGISTRY.blocks.len();
        let mut behaviors: Vec<Box<dyn BlockBehaviour>> = Vec::with_capacity(block_count);

        // Initialize all blocks with default behavior
        for (_, block) in REGISTRY.blocks.iter() {
            behaviors.push(Box::new(DefaultBlockBehaviour::new(block)));
        }

        Self { behaviors }
    }

    /// Sets a custom behavior for a block.
    pub fn set_behavior(&mut self, block: BlockRef, behavior: Box<dyn BlockBehaviour>) {
        let id = *REGISTRY.blocks.get_id(block);
        self.behaviors[id] = behavior;
    }

    /// Gets the behavior for a block.
    #[must_use]
    pub fn get_behavior(&self, block: BlockRef) -> &dyn BlockBehaviour {
        let id = *REGISTRY.blocks.get_id(block);
        self.behaviors[id].as_ref()
    }

    /// Gets the behavior for a block by its ID.
    #[must_use]
    pub fn get_behavior_by_id(&self, id: usize) -> Option<&dyn BlockBehaviour> {
        self.behaviors.get(id).map(AsRef::as_ref)
    }

    /// Gets the behavior for a block state.
    #[must_use]
    pub fn get_behavior_for_state(&self, state: BlockStateId) -> Option<&dyn BlockBehaviour> {
        let block = REGISTRY.blocks.by_state_id(state)?;
        Some(self.get_behavior(block))
    }
}

impl Default for BlockBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
