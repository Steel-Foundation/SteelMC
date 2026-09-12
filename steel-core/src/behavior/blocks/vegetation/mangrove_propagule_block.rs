use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, IntProperty,
};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, SaplingBlock, TreeGrower};

/// Vanilla `MangrovePropaguleBlock` behavior.
///
/// Vanilla extends `SaplingBlock`; the planted growth path is shared through
/// [`SaplingBlock::advance_tree`].
#[block_behavior]
pub struct MangrovePropaguleBlock {
    block: BlockRef,
    #[json_arg(r#enum = "TreeGrower", json = "tree_grower_name")]
    tree_grower: TreeGrower,
}

const AGE: &IntProperty = &BlockStateProperties::AGE_4;
const HANGING: &BoolProperty = &BlockStateProperties::HANGING;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
const MAX_AGE: u8 = 4;
const RANDOM_GROWTH_BOUND: u32 = 7;
const BONEMEAL_SUCCESS_CHANCE: f32 = 0.45;

impl MangrovePropaguleBlock {
    /// Creates a new mangrove propagule block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, tree_grower: TreeGrower) -> Self {
        Self { block, tree_grower }
    }

    /// Creates vanilla's initial hanging propagule state.
    pub(crate) fn create_new_hanging_propagule() -> BlockStateId {
        vanilla_blocks::MANGROVE_PROPAGULE
            .default_state()
            .set_value(HANGING, true)
            .set_value(AGE, 0)
    }

    fn advance_hanging(state: BlockStateId, world: &dyn LevelAccessor, pos: BlockPos) -> bool {
        let age = state.get_value(AGE);
        if age >= MAX_AGE {
            return false;
        }

        world.set_block_state(
            pos,
            state.set_value(AGE, age + 1),
            UpdateFlags::UPDATE_CLIENTS,
        )
    }

    fn advance_tree(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn Rng,
    ) {
        SaplingBlock::advance_tree(self.tree_grower, world, pos, state, rng);
    }

    fn random_tick_with_rng(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        rng: &mut dyn Rng,
    ) {
        if state.get_value(HANGING) {
            Self::advance_hanging(state, world, pos);
        } else if rng.random_range(0..RANDOM_GROWTH_BOUND) == 0 {
            self.advance_tree(world, pos, state, rng);
        }
    }
}

impl BlockBehavior for MangrovePropaguleBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        if direction == Direction::Up && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(HANGING) {
            let above = world.get_block_state(pos.above());
            return above
                .get_block()
                .has_tag(&BlockTag::SUPPORTS_HANGING_MANGROVE_PROPAGULE);
        }

        let below = world.get_block_state(pos.below());
        below
            .get_block()
            .has_tag(&BlockTag::SUPPORTS_MANGROVE_PROPAGULE)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self
            .block
            .default_state()
            .set_value(AGE, MAX_AGE)
            .set_value(WATERLOGGED, context.is_water_source());
        self.can_survive(state, context.world, context.place_pos())
            .then_some(state)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.random_tick_with_rng(state, world, pos, &mut rand::rng());
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for MangrovePropaguleBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        if state.get_value(HANGING) {
            return state.get_value(AGE) < MAX_AGE;
        }

        SaplingBlock::is_valid_bonemeal_target_for(self.tree_grower, world, pos)
    }

    fn is_bonemeal_success(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        if state.get_value(HANGING) {
            return state.get_value(AGE) < MAX_AGE;
        }

        rng.random::<f32>() < BONEMEAL_SUCCESS_CHANCE
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        if state.get_value(HANGING) {
            Self::advance_hanging(state, world, pos);
        } else {
            self.advance_tree(world, pos, state, rng);
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_items};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, ZeroRng, fresh_test_world, insert_ready_full_chunk};

    const STAGE: &IntProperty = &BlockStateProperties::STAGE;

    fn behavior() -> MangrovePropaguleBlock {
        MangrovePropaguleBlock::new(&vanilla_blocks::MANGROVE_PROPAGULE, TreeGrower::Mangrove)
    }

    #[test]
    fn new_hanging_propagule_starts_at_age_zero() {
        init_vanilla_registry();

        let state = MangrovePropaguleBlock::create_new_hanging_propagule();

        assert_eq!(state.get_block(), &vanilla_blocks::MANGROVE_PROPAGULE);
        assert!(state.get_value(HANGING));
        assert_eq!(state.get_value(AGE), 0);
    }

    #[test]
    fn placed_propagules_are_mature_and_preserve_source_water() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("mangrove_propagule_placement");
        let wet_pos = BlockPos::new(8, 64, 8);
        let dry_pos = wet_pos.east();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(wet_pos));
        assert!(world.set_block(
            wet_pos.below(),
            vanilla_blocks::DIRT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            dry_pos.below(),
            vanilla_blocks::DIRT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            wet_pos,
            vanilla_blocks::WATER.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let behavior = behavior();

        let wet_state = {
            let mut stack = ItemStack::new(&vanilla_items::MANGROVE_PROPAGULE);
            let context = BlockPlaceContext::directional(
                &world,
                wet_pos,
                Direction::Down,
                &mut stack,
                Direction::Up,
            );
            behavior
                .get_state_for_placement(&context)
                .expect("wet propagule should have a placement state")
        };
        let dry_state = {
            let mut stack = ItemStack::new(&vanilla_items::MANGROVE_PROPAGULE);
            let context = BlockPlaceContext::directional(
                &world,
                dry_pos,
                Direction::Down,
                &mut stack,
                Direction::Up,
            );
            behavior
                .get_state_for_placement(&context)
                .expect("dry propagule should have a placement state")
        };

        assert_eq!(wet_state.get_value(AGE), MAX_AGE);
        assert!(wet_state.get_value(WATERLOGGED));
        assert_eq!(dry_state.get_value(AGE), MAX_AGE);
        assert!(!dry_state.get_value(WATERLOGGED));
    }

    #[test]
    fn hanging_growth_stops_at_age_four() {
        init_vanilla_registry();
        let level = TestLevel::default();
        let pos = BlockPos::ZERO;
        let age_three = MangrovePropaguleBlock::create_new_hanging_propagule().set_value(AGE, 3);

        assert!(MangrovePropaguleBlock::advance_hanging(
            age_three, &level, pos
        ));
        assert_eq!(
            level
                .last_placed_state()
                .expect("hanging growth should place an updated state")
                .get_value(AGE),
            MAX_AGE
        );
        assert!(!MangrovePropaguleBlock::advance_hanging(
            age_three.set_value(AGE, MAX_AGE),
            &level,
            pos,
        ));
    }

    #[test]
    fn bonemeal_targets_match_hanging_age_and_tree_height() {
        init_vanilla_registry();
        let behavior = behavior();
        let hanging = MangrovePropaguleBlock::create_new_hanging_propagule();
        let level = TestLevel::default().with_min_y(0).with_height(10);

        assert!(behavior.is_valid_bonemeal_target(hanging, &level, BlockPos::ZERO));
        assert!(!behavior.is_valid_bonemeal_target(
            hanging.set_value(AGE, MAX_AGE),
            &level,
            BlockPos::ZERO,
        ));
        let min_height = TreeGrower::Mangrove
            .minimum_height()
            .expect("mangrove has a primary tree");
        assert!(behavior.is_valid_bonemeal_target(
            vanilla_blocks::MANGROVE_PROPAGULE.default_state(),
            &level,
            BlockPos::new(0, 9 - min_height, 0),
        ));
        assert!(!behavior.is_valid_bonemeal_target(
            vanilla_blocks::MANGROVE_PROPAGULE.default_state(),
            &level,
            BlockPos::new(0, 10 - min_height, 0),
        ));
    }

    #[test]
    fn planted_bonemeal_advances_stage_before_growing_tree() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("mangrove_propagule_stage");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::MANGROVE_PROPAGULE.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let behavior = behavior();

        behavior.perform_bonemeal(state, &world, &mut ZeroRng, pos);

        assert_eq!(world.get_block_state(pos).get_value(STAGE), 1);
    }

    #[test]
    fn mature_planted_propagule_generates_mangrove_tree() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("mangrove_propagule_tree");
        let pos = BlockPos::new(8, 64, 8);
        let center = ChunkPos::from_block_pos(pos);
        for chunk_x in center.0.x - 1..=center.0.x + 1 {
            for chunk_z in center.0.y - 1..=center.0.y + 1 {
                insert_ready_full_chunk(&world, ChunkPos::new(chunk_x, chunk_z));
            }
        }
        for x in pos.x() - 12..=pos.x() + 12 {
            for z in pos.z() - 12..=pos.z() + 12 {
                assert!(world.set_block(
                    BlockPos::new(x, pos.y() - 1, z),
                    vanilla_blocks::DIRT.default_state(),
                    UpdateFlags::UPDATE_NONE,
                ));
            }
        }
        let state = vanilla_blocks::MANGROVE_PROPAGULE
            .default_state()
            .set_value(STAGE, 1)
            .set_value(AGE, MAX_AGE);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let placed = (0..64).any(|seed| {
            let mut rng = StdRng::seed_from_u64(seed);
            TreeGrower::Mangrove.grow_tree(&world, pos, state, &mut rng)
        });

        assert!(placed);
        assert!((pos.y()..pos.y() + 24).any(|y| {
            world
                .get_block_state(BlockPos::new(pos.x(), y, pos.z()))
                .get_block()
                == &vanilla_blocks::MANGROVE_LOG
        }));
    }

    #[test]
    fn unsupported_waterlogged_propagule_schedules_water_before_breaking() {
        init_vanilla_registry();
        let behavior = behavior();
        let state = vanilla_blocks::MANGROVE_PROPAGULE
            .default_state()
            .set_value(WATERLOGGED, true);
        let level = TestLevel::default();

        assert!(
            behavior
                .update_shape(
                    state,
                    &level,
                    BlockPos::ZERO,
                    Direction::Up,
                    BlockPos::ZERO.above(),
                    vanilla_blocks::AIR.default_state(),
                )
                .is_air()
        );
        assert!(level.scheduled_water_tick());

        let side_update = behavior.update_shape(
            state,
            &TestLevel::default(),
            BlockPos::ZERO,
            Direction::North,
            BlockPos::ZERO.north(),
            vanilla_blocks::AIR.default_state(),
        );
        assert_eq!(side_update, state);
    }
}
