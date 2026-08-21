use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::blocks::shapes::{self, SupportType, is_block_local_face_sturdy};
use steel_registry::fluid::FluidStateExt;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess};

use super::BlockRef;

/// Behavior for vanilla multiface blocks.
#[block_behavior]
pub struct MultifaceBlock {
    block: BlockRef,
}

pub(super) struct MultifaceSpreader {
    block: BlockRef,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MultifaceSpreadPos {
    pub(crate) pos: BlockPos,
    pub(crate) face: Direction,
}

#[derive(Clone, Copy)]
pub(crate) enum MultifaceSpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

pub(crate) fn multiface_spread_pos(
    pos: BlockPos,
    spread_direction: Direction,
    starting_face: Direction,
    spread_type: MultifaceSpreadType,
) -> MultifaceSpreadPos {
    match spread_type {
        MultifaceSpreadType::SamePosition => MultifaceSpreadPos {
            pos,
            face: spread_direction,
        },
        MultifaceSpreadType::SamePlane => MultifaceSpreadPos {
            pos: pos.relative(spread_direction),
            face: starting_face,
        },
        MultifaceSpreadType::WrapAround => MultifaceSpreadPos {
            pos: pos.relative(spread_direction).relative(starting_face),
            face: spread_direction.opposite(),
        },
    }
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
        world: &dyn LevelReader,
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
        world: &dyn LevelReader,
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

    pub(super) fn has_face(state: BlockStateId, direction: Direction) -> bool {
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

impl MultifaceSpreader {
    const SPREAD_TYPES: [MultifaceSpreadType; 3] = [
        MultifaceSpreadType::SamePosition,
        MultifaceSpreadType::SamePlane,
        MultifaceSpreadType::WrapAround,
    ];

    pub(super) const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn can_spread_in_any_direction(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        starting_face: Direction,
    ) -> bool {
        Direction::ALL.iter().any(|spread_direction| {
            self.get_spread_from_face_toward_direction(
                state,
                world,
                pos,
                starting_face,
                *spread_direction,
            )
            .is_some()
        })
    }

    pub(super) fn spread_from_random_face_toward_random_direction<W: LevelAccessor>(
        &self,
        state: BlockStateId,
        world: &W,
        pos: BlockPos,
        rng: &mut dyn Rng,
    ) -> Option<MultifaceSpreadPos> {
        let mut faces = Direction::ALL;
        Self::shuffle_directions(&mut faces, rng);

        for starting_face in faces {
            if !MultifaceBlock::has_face(state, starting_face) {
                continue;
            }

            let mut spread_directions = Direction::ALL;
            Self::shuffle_directions(&mut spread_directions, rng);
            for spread_direction in spread_directions {
                if let Some(spread_pos) = self.spread_from_face_toward_direction(
                    state,
                    world,
                    pos,
                    starting_face,
                    spread_direction,
                ) {
                    return Some(spread_pos);
                }
            }
        }

        None
    }

    pub(super) fn spread_from_face_toward_direction<W: LevelAccessor>(
        &self,
        state: BlockStateId,
        world: &W,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
    ) -> Option<MultifaceSpreadPos> {
        let spread_pos = self.get_spread_from_face_toward_direction(
            state,
            world,
            pos,
            starting_face,
            spread_direction,
        )?;
        let old_state = world.get_block_state(spread_pos.pos);
        let spread_state = MultifaceBlock::get_state_for_placement_with_dir(
            self.block,
            old_state,
            world,
            spread_pos.pos,
            spread_pos.face,
        )?;

        world
            .set_block_state(spread_pos.pos, spread_state, UpdateFlags::UPDATE_CLIENTS)
            .then_some(spread_pos)
    }

    fn get_spread_from_face_toward_direction(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
    ) -> Option<MultifaceSpreadPos> {
        if spread_direction.axis() == starting_face.axis()
            || !MultifaceBlock::has_face(state, starting_face)
            || MultifaceBlock::has_face(state, spread_direction)
        {
            return None;
        }

        Self::SPREAD_TYPES.iter().find_map(|spread_type| {
            let spread_pos =
                multiface_spread_pos(pos, spread_direction, starting_face, *spread_type);
            self.can_spread_into(world, spread_pos)
                .then_some(spread_pos)
        })
    }

    fn can_spread_into(&self, world: &dyn LevelReader, spread_pos: MultifaceSpreadPos) -> bool {
        let old_state = world.get_block_state(spread_pos.pos);
        let fluid_state = get_fluid_state_from_block(old_state);
        let replaceable = old_state.is_air()
            || old_state.get_block() == self.block
            || (fluid_state.is_water() && fluid_state.is_source());

        replaceable
            && MultifaceBlock::is_valid_state_for_placement(
                self.block,
                world,
                old_state,
                spread_pos.pos,
                spread_pos.face,
            )
    }

    fn shuffle_directions(directions: &mut [Direction; 6], rng: &mut dyn Rng) {
        for i in (1..directions.len()).rev() {
            let j = rng.random_range(0..=i);
            directions.swap(i, j);
        }
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
        let old_state = level.get_block_state(context.place_pos());

        context
            .get_nearest_looking_directions()
            .iter()
            .find_map(|direction| {
                MultifaceBlock::get_state_for_placement_with_dir(
                    self.block,
                    old_state,
                    level,
                    context.place_pos(),
                    *direction,
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
