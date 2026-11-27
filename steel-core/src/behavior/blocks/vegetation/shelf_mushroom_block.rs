use crate::behavior::{
    BlockBehavior, EntityFallOnContext, blocks::vegetation::bonemealable::Bonemealable,
    context::BlockPlaceContext,
};
use crate::entity::ai::path::PathComputationType;
use crate::world::{LevelReader, ScheduledTickAccess, World};
use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::sound_events::BLOCK_SHELF_MUSHROOM_BOUNCE;
use steel_registry::vanilla_entities::{ITEM, TNT};
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, Direction, EnumProperty, IntProperty},
    },
    vanilla_blocks,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

const MAX_AGE: u8 = 1;
const AGE: &IntProperty = &BlockStateProperties::AGE_1;
const FACING_PROPERTY: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

/// Shelf Mushroom Block behavior
#[block_behavior(class = "ShelfMushroomBlock")]
pub struct ShelfMushroomBlock {
    block: BlockRef,
}

impl ShelfMushroomBlock {
    /// Creates a new shelf mushroom block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[inline]
    fn age(state: BlockStateId) -> u8 {
        state.get_value(AGE)
    }

    #[inline]
    fn facing(state: BlockStateId) -> Direction {
        state.get_value(FACING_PROPERTY)
    }
}

impl BlockBehavior for ShelfMushroomBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let facing = Self::facing(state);
        let support_pos = pos.relative(facing.opposite());
        let support_state = world.get_block_state(support_pos);
        world.is_face_sturdy(support_state, support_pos, facing)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        for direction in context.get_nearest_looking_directions() {
            if !direction.is_horizontal() {
                continue;
            }

            let state = self
                .block
                .default_state()
                .set_value(FACING_PROPERTY, direction.opposite());
            if self.can_survive(state, context.world, context.place_pos()) {
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
        let facing = Self::facing(state);
        if direction == facing.opposite() && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }

    fn bounce_on(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) {
        if context.entity.entity_type != &ITEM && context.entity.entity_type != &TNT {
            world.play_sound(
                &BLOCK_SHELF_MUSHROOM_BOUNCE,
                SoundSource::Blocks,
                pos,
                1.0f32,
                1.0f32,
                None,
            );
        }
    }
}

impl Bonemealable for ShelfMushroomBlock {
    #[inline]
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        Self::age(state) < MAX_AGE
    }

    #[inline]
    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        world.set_block(
            pos,
            state.set_value(AGE, MAX_AGE),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}
