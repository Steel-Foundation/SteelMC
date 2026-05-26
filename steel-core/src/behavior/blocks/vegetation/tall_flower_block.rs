use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> refs/remotes/origin/master
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::{BlockPlaceContext, InventoryAccess};
<<<<<<< HEAD
=======
=======
use steel_registry::{REGISTRY, RegistryExt};
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
>>>>>>> refs/remotes/origin/master
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, DoublePlantBlock};

<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> refs/remotes/origin/master
/// Behavior for two-block-tall flowers.
#[block_behavior]
pub struct TallFlowerBlock {
    block: BlockRef,
<<<<<<< HEAD
=======
/// Vanilla `TallFlowerBlock` survival.
// TODO: Implement bonemeal and the rest of vanilla behavior.
#[block_behavior]
pub struct TallFlowerBlock {
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
/// Behavior for two-block-tall flowers.
#[block_behavior]
pub struct TallFlowerBlock {
    block: BlockRef,
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
=======
>>>>>>> refs/remotes/origin/master
    base: DoublePlantBlock,
}

impl TallFlowerBlock {
    /// Creates a new tall flower block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
            block,
=======
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
            block,
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
=======
            block,
>>>>>>> refs/remotes/origin/master
            base: DoublePlantBlock::new(block),
        }
    }
}

impl BlockBehavior for TallFlowerBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.base
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.base.can_survive(state, world, pos)
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: Option<&Player>,
<<<<<<< HEAD
<<<<<<< HEAD
        inv: &InventoryAccess,
    ) {
        self.base.set_placed_by(state, world, pos, player, inv);
=======
        item_stack: &ItemStack,
    ) {
        self.base
            .set_placed_by(state, world, pos, player, item_stack);
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
        inv: &InventoryAccess,
    ) {
        self.base.set_placed_by(state, world, pos, player, inv);
>>>>>>> refs/remotes/origin/master
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.base.get_state_for_placement(context)
    }
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
=======
>>>>>>> refs/remotes/origin/master

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for TallFlowerBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        if let Some(item) = REGISTRY.items.by_key(&self.block.key) {
            world.pop_resource(pos, ItemStack::new(item));
        }
    }
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
=======
>>>>>>> refs/remotes/origin/master
}
