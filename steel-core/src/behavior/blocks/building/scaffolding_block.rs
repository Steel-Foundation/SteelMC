use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, BoolProperty, Direction, IntProperty},
    shapes::VoxelShape,
};
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId};

use crate::behavior::{
    BlockBehavior, BlockCollisionContext, BlockPlaceContext,
    block::schedule_water_tick_if_waterlogged,
};
use crate::entity::entities::FallingBlockEntity;
use crate::world::{LevelReader, ScheduledTickAccess, World};

const SHAPE_STABLE_BOXES: &[BlockLocalAabb] = &[
    BlockLocalAabb::new(0.0, 0.875, 0.0, 1.0, 1.0, 1.0),
    BlockLocalAabb::new(0.0, 0.0, 0.0, 0.125, 1.0, 0.125),
    BlockLocalAabb::new(0.875, 0.0, 0.0, 1.0, 1.0, 0.125),
    BlockLocalAabb::new(0.0, 0.0, 0.875, 0.125, 1.0, 1.0),
    BlockLocalAabb::new(0.875, 0.0, 0.875, 1.0, 1.0, 1.0),
];
const SHAPE_UNSTABLE_BOTTOM_BOXES: &[BlockLocalAabb] =
    &[BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.125, 1.0)];
const SHAPE_BELOW_BLOCK_BOXES: &[BlockLocalAabb] =
    &[BlockLocalAabb::new(0.0, -1.0, 0.0, 1.0, 0.0, 1.0)];

const SHAPE_STABLE: VoxelShape = VoxelShape::from_boxes(SHAPE_STABLE_BOXES);
const SHAPE_UNSTABLE_BOTTOM: VoxelShape = VoxelShape::from_boxes(SHAPE_UNSTABLE_BOTTOM_BOXES);
const SHAPE_BELOW_BLOCK: VoxelShape = VoxelShape::from_boxes(SHAPE_BELOW_BLOCK_BOXES);

const TICK_DELAY: i32 = 1;

/// Vanilla scaffolding placement, stability, falling, and collision behavior.
#[block_behavior]
pub struct ScaffoldingBlock {
    block: BlockRef,
}

const BOTTOM: &BoolProperty = &BlockStateProperties::BOTTOM;
const STABILITY_DISTANCE: &IntProperty = &BlockStateProperties::STABILITY_DISTANCE;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl ScaffoldingBlock {
    pub(crate) const STABILITY_MAX_DISTANCE: u8 = 7;

    /// Creates a scaffolding block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn is_bottom(&self, world: &dyn LevelReader, pos: BlockPos, distance: u8) -> bool {
        distance > 0 && world.get_block_state(pos.below()).get_block() != self.block
    }

    /// Vanilla `ScaffoldingBlock.getDistance`.
    pub(crate) fn get_distance(world: &dyn LevelReader, pos: BlockPos) -> u8 {
        let below_pos = pos.below();
        let below_state = world.get_block_state(below_pos);
        let mut distance = Self::STABILITY_MAX_DISTANCE;
        if below_state.get_block() == &vanilla_blocks::SCAFFOLDING {
            distance = below_state.get_value(STABILITY_DISTANCE);
        } else if world.is_face_sturdy(below_state, below_pos, Direction::Up) {
            return 0;
        }

        for direction in Direction::HORIZONTAL {
            let neighbor_state = world.get_block_state(pos.relative(direction));
            if neighbor_state.get_block() == &vanilla_blocks::SCAFFOLDING {
                distance = distance.min(neighbor_state.get_value(STABILITY_DISTANCE) + 1);
                if distance == 1 {
                    break;
                }
            }
        }
        distance
    }
}

impl BlockBehavior for ScaffoldingBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        world.schedule_block_tick_default(pos, self.block, TICK_DELAY);
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.place_pos();
        let distance = Self::get_distance(context.world.as_ref(), pos);
        Some(
            self.block
                .default_state()
                .set_value(WATERLOGGED, context.is_water_source())
                .set_value(STABILITY_DISTANCE, distance)
                .set_value(
                    BOTTOM,
                    self.is_bottom(context.world.as_ref(), pos, distance),
                ),
        )
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::get_distance(world, pos) < Self::STABILITY_MAX_DISTANCE
    }

    fn can_be_replaced(&self, _state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        context.with_item(|item| item.item() == REGISTRY.items.by_block(self.block))
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        let _ = world.schedule_block_tick_default(pos, self.block, TICK_DELAY);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let distance = Self::get_distance(world.as_ref(), pos);
        let new_state = state
            .set_value(STABILITY_DISTANCE, distance)
            .set_value(BOTTOM, self.is_bottom(world.as_ref(), pos, distance));

        if distance == Self::STABILITY_MAX_DISTANCE {
            if state.get_value(STABILITY_DISTANCE) == Self::STABILITY_MAX_DISTANCE {
                let _ = FallingBlockEntity::fall(world, pos, new_state);
            } else {
                let _ = world.destroy_block(pos, true);
            }
        } else if state != new_state {
            let _ = world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
        }
    }

    fn get_collision_shape(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        pos: BlockPos,
        context: BlockCollisionContext,
    ) -> VoxelShape {
        if context.is_placement() {
            return VoxelShape::EMPTY;
        }

        if context.is_above(VoxelShape::FULL_BLOCK, pos, true) && !context.is_descending() {
            return SHAPE_STABLE;
        }

        let distance = state.get_value(STABILITY_DISTANCE);
        let bottom = state.get_value(BOTTOM);
        if distance != 0 && bottom && context.is_above(SHAPE_BELOW_BLOCK, pos, true) {
            SHAPE_UNSTABLE_BOTTOM
        } else {
            VoxelShape::EMPTY
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities, vanilla_fluids};
    use steel_utils::{ChunkPos, WorldAabb};

    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};

    fn scaffolding_state(distance: u8, bottom: bool) -> BlockStateId {
        vanilla_blocks::SCAFFOLDING
            .default_state()
            .set_value(STABILITY_DISTANCE, distance)
            .set_value(BOTTOM, bottom)
    }

    fn collision_shape(state: BlockStateId, context: BlockCollisionContext) -> VoxelShape {
        let behavior = ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING);
        let level = TestLevel::default().with_min_y(0);
        behavior.get_collision_shape(state, &level, BlockPos::new(0, 64, 0), context)
    }

    #[test]
    fn placement_context_has_no_scaffolding_collision() {
        init_vanilla_registry();

        let shape = collision_shape(
            scaffolding_state(0, false),
            BlockCollisionContext::with_position(65.0, false),
        );

        assert_eq!(shape, VoxelShape::EMPTY);
    }

    #[test]
    fn entity_above_scaffolding_collides_with_stable_shape() {
        init_vanilla_registry();

        let shape = collision_shape(
            scaffolding_state(0, false),
            BlockCollisionContext::entity(65.0, false),
        );

        assert_eq!(shape, SHAPE_STABLE);
    }

    #[test]
    fn descending_entity_only_collides_with_unstable_bottom_shape() {
        init_vanilla_registry();

        let shape = collision_shape(
            scaffolding_state(1, true),
            BlockCollisionContext::entity(64.5, true),
        );

        assert_eq!(shape, SHAPE_UNSTABLE_BOTTOM);
    }

    #[test]
    fn non_bottom_descending_scaffolding_has_empty_collision() {
        init_vanilla_registry();

        let shape = collision_shape(
            scaffolding_state(1, false),
            BlockCollisionContext::entity(64.5, true),
        );

        assert_eq!(shape, VoxelShape::EMPTY);
    }

    #[test]
    fn shape_update_schedules_stability_and_water_ticks() {
        init_vanilla_registry();
        let behavior = ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING);
        let state = vanilla_blocks::SCAFFOLDING
            .default_state()
            .set_value(WATERLOGGED, true);
        let pos = BlockPos::new(0, 64, 0);
        let level = TestLevel::default();

        assert_eq!(
            behavior.update_shape(
                state,
                &level,
                pos,
                Direction::North,
                pos.north(),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );
        assert_eq!(
            level
                .scheduled_block_ticks
                .borrow()
                .iter()
                .map(|tick| (tick.pos, tick.block, tick.delay))
                .collect::<Vec<_>>(),
            vec![(pos, &vanilla_blocks::SCAFFOLDING, 1)]
        );
        assert_eq!(
            level
                .scheduled_fluid_ticks
                .borrow()
                .iter()
                .map(|tick| (tick.pos, tick.fluid, tick.delay))
                .collect::<Vec<_>>(),
            vec![(pos, &vanilla_fluids::WATER, 5)]
        );
    }

    #[test]
    fn distance_uses_sturdy_vertical_and_nearest_horizontal_support() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let level = TestLevel::default().with_min_y(0);

        assert_eq!(
            ScaffoldingBlock::get_distance(&level, pos),
            ScaffoldingBlock::STABILITY_MAX_DISTANCE
        );

        level.set_test_block(pos.below(), vanilla_blocks::STONE.default_state());
        assert_eq!(ScaffoldingBlock::get_distance(&level, pos), 0);

        level.set_test_block(pos.below(), scaffolding_state(4, false));
        assert_eq!(ScaffoldingBlock::get_distance(&level, pos), 4);

        level.set_test_block(pos.below(), vanilla_blocks::AIR.default_state());
        level.set_test_block(pos.east(), scaffolding_state(4, true));
        level.set_test_block(pos.west(), scaffolding_state(2, true));
        assert_eq!(ScaffoldingBlock::get_distance(&level, pos), 3);
    }

    #[test]
    fn newly_unsupported_scaffolding_breaks_and_drops_an_item() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("scaffolding_destroy_tick");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = scaffolding_state(ScaffoldingBlock::STABILITY_MAX_DISTANCE - 1, true);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING).tick(state, &world, pos);

        assert!(world.get_block_state(pos).is_air());
        let entities =
            world.get_entities_in_aabb(&WorldAabb::new(7.0, 63.0, 7.0, 10.0, 67.0, 10.0));
        assert!(
            entities
                .iter()
                .any(|entity| entity.entity_type() == &vanilla_entities::ITEM)
        );
    }

    #[test]
    fn max_distance_waterlogged_scaffolding_falls_and_leaves_water() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("scaffolding_fall_tick");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = scaffolding_state(ScaffoldingBlock::STABILITY_MAX_DISTANCE, true)
            .set_value(WATERLOGGED, true);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING).tick(state, &world, pos);

        assert_eq!(
            world.get_block_state(pos),
            vanilla_blocks::WATER.default_state()
        );
        let entities =
            world.get_entities_in_aabb(&WorldAabb::new(7.0, 63.0, 7.0, 10.0, 67.0, 10.0));
        assert!(
            entities
                .iter()
                .any(|entity| entity.entity_type() == &vanilla_entities::FALLING_BLOCK)
        );
    }
}
