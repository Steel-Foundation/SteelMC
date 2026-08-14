use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::blocks::shapes::{self, SupportType, is_block_local_face_sturdy};
use steel_registry::fluid::FluidStateExt;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Behavior for vanilla multiface blocks.
#[block_behavior]
pub struct MultifaceBlock {
    block: BlockRef,
}

const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
const DOWN: &BoolProperty = &BlockStateProperties::DOWN;
const EAST: &BoolProperty = &BlockStateProperties::EAST;
const NORTH: &BoolProperty = &BlockStateProperties::NORTH;
const SOUTH: &BoolProperty = &BlockStateProperties::SOUTH;
const UP: &BoolProperty = &BlockStateProperties::UP;
const WEST: &BoolProperty = &BlockStateProperties::WEST;

impl MultifaceBlock {
    /// Creates a new multiface block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `MultifaceBlock.canAttachTo(level, directionTowardsNeighbor, neighborPos, neighborState)`.
    ///
    /// Returns whether the block at `neighbor_pos` has a full face on the side
    /// facing back toward us. Checks the support shape first, then the collision
    /// shape, matching vanilla's `Block.isFaceFull` OR pattern.
    pub fn can_attach_to(
        world: &dyn LevelReader,
        pos: BlockPos,
        direction_to_neighbor: Direction,
    ) -> bool {
        let neighbor_pos = pos.relative(direction_to_neighbor);
        let block_state = world.get_block_state(neighbor_pos);
        Self::can_attach_to_state(world, direction_to_neighbor, neighbor_pos, block_state)
    }

    pub(super) fn can_attach_to_state(
        world: &dyn LevelReader,
        direction_to_neighbor: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> bool {
        let support_direction = direction_to_neighbor.opposite();
        if neighbor_state.get_block().config.dynamic_shape {
            let behavior = BLOCK_BEHAVIORS.get_behavior(neighbor_state.get_block());
            return is_block_local_face_sturdy(
                &behavior.get_block_support_boxes(neighbor_state, world, neighbor_pos),
                support_direction,
                SupportType::Full,
            ) || is_block_local_face_sturdy(
                &behavior.get_collision_boxes(
                    neighbor_state,
                    world,
                    neighbor_pos,
                    BlockCollisionContext::empty(),
                ),
                support_direction,
                SupportType::Full,
            );
        }

        shapes::is_offset_face_full(
            neighbor_state.get_support_shape_at(neighbor_pos),
            support_direction,
        ) || shapes::is_offset_face_full(
            neighbor_state.get_collision_shape_at(neighbor_pos),
            support_direction,
        )
    }

    /// Vanilla `MultifaceBlock.getFaceProperty(faceDirection)`.
    pub(super) const fn face_property(direction: Direction) -> &'static BoolProperty {
        match direction {
            Direction::Up => UP,
            Direction::Down => DOWN,
            Direction::North => NORTH,
            Direction::South => SOUTH,
            Direction::East => EAST,
            Direction::West => WEST,
        }
    }

    const fn is_face_supported(_face_direction: Direction) -> bool {
        true
    }

    /// Vanilla `MultifaceBlock.get_state_for_placement()`
    fn get_state_for_placement_with_dir(
        block: BlockRef,
        old_state: BlockStateId,
        world: &Arc<World>,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> Option<BlockStateId> {
        if !Self::is_valid_state_for_placement(
            block,
            world,
            old_state,
            placement_pos,
            placement_direction,
        ) {
            return None;
        }

        let mut new_state = if old_state.get_block() == block {
            old_state
        } else {
            let fluid_state = get_fluid_state_from_block(old_state);
            if fluid_state.is_water() && fluid_state.is_source() {
                block.default_state().set_value(WATERLOGGED, true)
            } else {
                block.default_state()
            }
        };
        new_state = new_state.set_value(Self::face_property(placement_direction), true);
        Some(new_state)
    }

    fn is_valid_state_for_placement(
        block: BlockRef,
        world: &Arc<World>,
        old_state: BlockStateId,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> bool {
        if Self::is_face_supported(placement_direction)
            && (old_state.get_block() != block || !Self::has_face(old_state, placement_direction))
        {
            let neighbor_pos = placement_pos.relative(placement_direction);
            return Self::can_attach_to_state(
                world,
                placement_direction,
                neighbor_pos,
                world.get_block_state(neighbor_pos),
            );
        }
        false
    }

    fn has_face(state: BlockStateId, direction: Direction) -> bool {
        state
            .try_get_value(Self::face_property(direction))
            .unwrap_or(false)
    }

    fn has_any_face(state: BlockStateId) -> bool {
        Direction::ALL
            .iter()
            .any(|direction| Self::has_face(state, *direction))
    }

    fn has_any_vacant_face(state: BlockStateId) -> bool {
        Direction::ALL
            .iter()
            .any(|direction| !Self::has_face(state, *direction))
    }

    fn remove_face(state: BlockStateId, property: &BoolProperty) -> BlockStateId {
        let new_state = state.set_value(property, false);
        if Self::has_any_face(new_state) {
            return new_state;
        }
        vanilla_blocks::AIR.default_state()
    }
}

impl BlockBehavior for MultifaceBlock {
    /// Vanilla `MultifaceBlock.canSurvive`.
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let mut has_at_least_one_face = false;
        for direction in Direction::ALL {
            if Self::has_face(state, direction) {
                if !MultifaceBlock::can_attach_to(world, pos, direction) {
                    return false;
                }
                has_at_least_one_face = true;
            }
        }
        has_at_least_one_face
    }

    /// Vanilla `MultifaceBlock.getStateForPlacement`
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let level = context.world;
        let place_pos = if context.replaces_clicked_block() {
            context.hit_pos()
        } else {
            context.place_pos()
        };
        let old_state = level.get_block_state(place_pos);

        context
            .get_nearest_looking_directions()
            .iter()
            .find_map(|direction| {
                MultifaceBlock::get_state_for_placement_with_dir(
                    self.block, old_state, level, place_pos, *direction,
                )
            })
    }

    /// Vanilla `MultifaceBlock.updateShape`
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);

        if !MultifaceBlock::has_any_face(state) {
            return vanilla_blocks::AIR.default_state();
        }

        if Self::has_face(state, direction)
            && !Self::can_attach_to_state(world, direction, neighbor_pos, neighbor_state)
        {
            return Self::remove_face(state, Self::face_property(direction));
        }
        state
    }

    /// Vanilla `MultifaceBlock.canBeReplaced`
    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        !context.with_item(|item| item.item() == REGISTRY.items.by_block(state.get_block()))
            || Self::has_any_vacant_face(state)
    }
}
