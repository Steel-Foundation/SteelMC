use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, BoolProperty, Direction, IntProperty},
    shapes::VoxelShape,
};
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
const STABILITY_MAX_DISTANCE: u8 = 7;

/// Vanilla scaffolding stability-distance, bottom-support, and falling behavior.
///
/// Scaffolding is not a vanilla `FallingBlock`; it spawns a falling entity from its own
/// scheduled tick once the stability distance reaches `STABILITY_MAX_DISTANCE`.
#[block_behavior]
pub struct ScaffoldingBlock {
    block: BlockRef,
}

const BOTTOM: &BoolProperty = &BlockStateProperties::BOTTOM;
const STABILITY_DISTANCE: &IntProperty = &BlockStateProperties::STABILITY_DISTANCE;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl ScaffoldingBlock {
    /// Creates a scaffolding block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `ScaffoldingBlock.isBottom`.
    fn is_bottom(&self, world: &dyn LevelReader, pos: BlockPos, distance: u8) -> bool {
        distance > 0 && world.get_block_state(pos.below()).get_block() != self.block
    }

    /// Vanilla `ScaffoldingBlock.getDistance`.
    fn get_distance(&self, world: &dyn LevelReader, pos: BlockPos) -> u8 {
        let below_pos = pos.below();
        let below_state = world.get_block_state(below_pos);
        let mut distance = STABILITY_MAX_DISTANCE;
        if below_state.get_block() == self.block {
            distance = below_state.get_value(STABILITY_DISTANCE);
        } else if world.is_face_sturdy(below_state, below_pos, Direction::Up) {
            return 0;
        }

        for direction in [
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            let relative_state = world.get_block_state(pos.relative(direction));
            if relative_state.get_block() == self.block {
                distance = distance.min(relative_state.get_value(STABILITY_DISTANCE) + 1);
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
        world.schedule_block_tick_default(pos, self.block, 1);
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.place_pos();
        let distance = self.get_distance(context.world, pos);
        Some(
            self.block
                .default_state()
                .set_value(WATERLOGGED, context.is_water_source())
                .set_value(STABILITY_DISTANCE, distance)
                .set_value(BOTTOM, self.is_bottom(context.world, pos, distance)),
        )
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.get_distance(world, pos) < STABILITY_MAX_DISTANCE
    }

    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        // Vanilla: `context.getItemInHand().is(this.asItem())`.
        context.with_item(|item| item.item() == REGISTRY.items.by_block(state.get_block()))
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
        let distance = self.get_distance(world.as_ref(), pos);
        let new_state = state
            .set_value(STABILITY_DISTANCE, distance)
            .set_value(BOTTOM, self.is_bottom(world.as_ref(), pos, distance));
        if new_state.get_value(STABILITY_DISTANCE) == STABILITY_MAX_DISTANCE {
            if state.get_value(STABILITY_DISTANCE) == STABILITY_MAX_DISTANCE {
                let _ = FallingBlockEntity::fall(world, pos, new_state);
            } else {
                world.destroy_block(pos, true);
            }
        } else if state != new_state {
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
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
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_fluids};
    use steel_utils::ChunkPos;

    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
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
            BlockCollisionContext::pre_move(65.0, false),
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
    fn distance_is_zero_on_sturdy_ground() {
        init_vanilla_registry();

        let level = TestLevel::default()
            .with_min_y(0)
            .with_default_block_state(vanilla_blocks::STONE.default_state());

        assert_eq!(
            ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING)
                .get_distance(&level, BlockPos::new(0, 64, 0)),
            0
        );
    }

    #[test]
    fn distance_inherits_from_scaffolding_below() {
        init_vanilla_registry();

        let level = TestLevel::default()
            .with_min_y(0)
            .with_block(BlockPos::new(0, 63, 0), scaffolding_state(3, false));

        assert_eq!(
            ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING)
                .get_distance(&level, BlockPos::new(0, 64, 0)),
            3
        );
    }

    #[test]
    fn distance_takes_minimum_horizontal_neighbor_plus_one() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let level = TestLevel::default()
            .with_min_y(0)
            .with_block(pos.east(), scaffolding_state(2, true))
            .with_block(pos.west(), scaffolding_state(4, true));

        assert_eq!(
            ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING).get_distance(&level, pos),
            3
        );
    }

    #[test]
    fn unsupported_scaffolding_reaches_max_distance_and_cannot_survive() {
        init_vanilla_registry();
        let behavior = ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING);
        let level = TestLevel::default().with_min_y(0);
        let pos = BlockPos::new(0, 64, 0);

        assert_eq!(behavior.get_distance(&level, pos), STABILITY_MAX_DISTANCE);
        assert!(!behavior.can_survive(scaffolding_state(0, true), &level, pos));
    }

    #[test]
    fn supported_scaffolding_can_survive() {
        init_vanilla_registry();
        let behavior = ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING);
        let level = TestLevel::default()
            .with_min_y(0)
            .with_default_block_state(vanilla_blocks::STONE.default_state());

        assert!(behavior.can_survive(scaffolding_state(0, false), &level, BlockPos::new(0, 64, 0)));
    }

    #[test]
    fn first_max_distance_tick_destroys_unsupported_scaffolding_with_drops() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("scaffolding_destroy_tick");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        // Stored distance 1 as if it were recently supported; the air surroundings
        // recompute distance 7 for the first time, so vanilla destroys with drops.
        let stale = scaffolding_state(1, true);
        assert!(world.set_block(pos, stale, UpdateFlags::UPDATE_NONE));

        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::SCAFFOLDING)
            .tick(stale, &world, pos);

        assert!(world.get_block_state(pos).is_air());
    }

    #[test]
    fn repeated_max_distance_tick_converts_scaffolding_to_falling_entity() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("scaffolding_fall_tick");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        // Already at max distance: a second stability tick converts the block to a
        // falling entity, which clears the position (fall replaces waterlogged=false
        // scaffolding with its empty fluid, i.e. air).
        let maxed = scaffolding_state(STABILITY_MAX_DISTANCE, true);
        assert!(world.set_block(pos, maxed, UpdateFlags::UPDATE_NONE));

        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::SCAFFOLDING)
            .tick(maxed, &world, pos);

        assert!(world.get_block_state(pos).is_air());
    }

    #[test]
    fn stability_tick_rewrites_distance_for_supported_scaffolding() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("scaffolding_update_tick");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let stale = scaffolding_state(4, true);
        assert!(world.set_block(pos, stale, UpdateFlags::UPDATE_NONE));

        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::SCAFFOLDING)
            .tick(stale, &world, pos);

        let after = world.get_block_state(pos);
        assert_eq!(after.get_block(), &vanilla_blocks::SCAFFOLDING);
        assert_eq!(after.get_value(STABILITY_DISTANCE), 0);
        assert!(!after.get_value(BOTTOM));
    }
}
