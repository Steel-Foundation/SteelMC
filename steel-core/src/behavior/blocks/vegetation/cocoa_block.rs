use std::sync::Arc;

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, Direction, EnumProperty, IntProperty},
    },
    item_stack::ItemStack,
    vanilla_block_tags::BlockTag,
    vanilla_blocks, vanilla_items,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehavior, blocks::vegetation::bonemealable::Bonemealable, context::BlockPlaceContext,
    },
    entity::ai::path::PathComputationType,
    world::{LevelReader, ScheduledTickAccess, World},
};

const MAX_AGE: u8 = 2;
const AGE_PROPERTY: IntProperty = BlockStateProperties::AGE_2;
const FACING_PROPERTY: EnumProperty<Direction> = BlockStateProperties::HORIZONTAL_FACING;

/// Cocoa Block behavior
#[block_behavior]
pub struct CocoaBlock {
    block: BlockRef,
}

impl CocoaBlock {
    /// Creates a new cocoa block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn age(state: BlockStateId) -> u8 {
        state.get_value(&AGE_PROPERTY)
    }

    fn with_age(state: BlockStateId, age: u8) -> BlockStateId {
        state.set_value(&AGE_PROPERTY, age)
    }
}

impl BlockBehavior for CocoaBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let facing: Direction = state.get_value(&FACING_PROPERTY);
        let support = world.get_block_state(pos.relative(facing));
        support.get_block().has_tag(&BlockTag::SUPPORTS_COCOA)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        for direction in context.get_nearest_looking_directions() {
            if !direction.is_horizontal() {
                continue;
            }

            let state = self
                .block
                .default_state()
                .set_value(&FACING_PROPERTY, direction);
            if self.can_survive(state, context.world, context.relative_pos) {
                return Some(state);
            }
        }

        None
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let facing: Direction = state.get_value(&FACING_PROPERTY);
        if direction == facing && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        Self::age(state) < MAX_AGE
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if rand::random_range(0..5) != 0 {
            return;
        }

        let age = Self::age(state);
        if age >= MAX_AGE {
            return;
        }

        world.set_block(
            pos,
            Self::with_age(state, age + 1),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::ITEMS.cocoa_beans))
    }
}

impl Bonemealable for CocoaBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        Self::age(state) < MAX_AGE
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        world.set_block(
            pos,
            Self::with_age(state, Self::age(state) + 1),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}
