use crate::behavior::{
    BlockBehavior, EntityLandingContext, blocks::vegetation::bonemealable::Bonemealable,
    context::BlockPlaceContext,
};
use crate::entity::ai::path::PathComputationType;
use crate::world::{LevelReader, ScheduledTickAccess, World};
use glam::DVec3;
use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
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

const MUSHROOM_SHELF_BOUNCE_SCALE: f64 = 0.75f64;

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

    fn age(state: BlockStateId) -> u8 {
        state.get_value(AGE)
    }

    fn facing(state: BlockStateId) -> Direction {
        state.get_value(FACING_PROPERTY)
    }

    #[must_use]
    fn velocity_after_fall(context: EntityLandingContext) -> DVec3 {
        if context.velocity.y >= 0.0 {
            return context.velocity;
        }

        let bounce_factor = if context.is_living_entity { 1.0 } else { 0.8 };
        DVec3::new(
            context.velocity.x,
            -context.velocity.y * bounce_factor,
            context.velocity.z,
        )
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

    fn update_entity_movement_after_fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityLandingContext,
    ) -> DVec3 {
        if context.suppresses_bounce {
            return self.default_update_entity_movement_after_fall_on(state, world, pos, context);
        }

        Self::velocity_after_fall(context)
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
}

impl Bonemealable for ShelfMushroomBlock {
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
        world.set_block(pos, state.set_value(AGE, 1), UpdateFlags::UPDATE_CLIENTS);
    }
}
