use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, IntProperty,
};
use steel_registry::feature::ConfiguredFeatureKind;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{REGISTRY, vanilla_blocks, vanilla_configured_features};
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};
use steel_worldgen::biomes::obfuscate_biome_seed;

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess, World};
use crate::worldgen::feature::FeatureDecorationRunner;

use super::BlockRef;

/// Vanilla `MangrovePropaguleBlock` behavior.
#[block_behavior]
pub struct MangrovePropaguleBlock {
    block: BlockRef,
}

const AGE: &IntProperty = &BlockStateProperties::AGE_4;
const HANGING: &BoolProperty = &BlockStateProperties::HANGING;
const STAGE: &IntProperty = &BlockStateProperties::STAGE;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
const MAX_AGE: u8 = 4;
const RANDOM_GROWTH_BOUND: u32 = 7;
const BONEMEAL_SUCCESS_CHANCE: f32 = 0.45;
const TALL_MANGROVE_CHANCE: f32 = 0.85;
const MANGROVE_MINIMUM_HEIGHT: i32 = 2;

impl MangrovePropaguleBlock {
    /// Creates a new mangrove propagule block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
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

    fn grow_tree(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn Rng,
    ) -> bool {
        let feature = if rng.random::<f32>() < TALL_MANGROVE_CHANCE {
            &*vanilla_configured_features::TALL_MANGROVE
        } else {
            &*vanilla_configured_features::MANGROVE
        };
        let ConfiguredFeatureKind::Tree(config) = &feature.kind else {
            return false;
        };
        let replacement = if state.get_value(WATERLOGGED) {
            vanilla_blocks::WATER.default_state()
        } else {
            vanilla_blocks::AIR.default_state()
        };
        world.set_block(pos, replacement, UpdateFlags::UPDATE_NONE);

        let mut worldgen_random = WorldgenRandom::from_seed(rng.random());
        let mut level = Arc::clone(world);
        let placed = FeatureDecorationRunner::place_tree_feature(
            &mut level,
            &REGISTRY,
            &mut worldgen_random,
            config,
            pos,
            obfuscate_biome_seed(world.seed()),
        );

        if placed {
            if world.get_block_state(pos) == replacement {
                world.send_block_updated(pos);
            }
        } else {
            world.set_block(pos, state, UpdateFlags::UPDATE_NONE);
        }
        placed
    }

    fn advance_tree(world: &Arc<World>, pos: BlockPos, state: BlockStateId, rng: &mut dyn Rng) {
        if state.get_value(STAGE) == 0 {
            world.set_block(pos, state.set_value(STAGE, 1), UpdateFlags::UPDATE_NONE);
        } else {
            Self::grow_tree(world, pos, state, rng);
        }
    }

    fn random_tick_with_rng(
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        rng: &mut dyn Rng,
    ) {
        if state.get_value(HANGING) {
            Self::advance_hanging(state, world, pos);
        } else if rng.random_range(0..RANDOM_GROWTH_BOUND) == 0 {
            Self::advance_tree(world, pos, state, rng);
        }
    }
}

impl BlockBehavior for MangrovePropaguleBlock {
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
        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
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
        Self::random_tick_with_rng(state, world, pos, &mut rand::rng());
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

        !world.is_outside_build_height(pos.above_n(MANGROVE_MINIMUM_HEIGHT).y())
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
            Self::advance_tree(world, pos, state, rng);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rand::{SeedableRng, TryRng, rngs::StdRng};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_items};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};

    #[derive(Default)]
    struct ZeroRng;

    impl TryRng for ZeroRng {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            Ok(0)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(0)
        }

        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            dst.fill(0);
            Ok(())
        }
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
        let behavior = MangrovePropaguleBlock::new(&vanilla_blocks::MANGROVE_PROPAGULE);

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
        let behavior = MangrovePropaguleBlock::new(&vanilla_blocks::MANGROVE_PROPAGULE);
        let hanging = MangrovePropaguleBlock::create_new_hanging_propagule();
        let level = TestLevel::default().with_min_y(0).with_height(10);

        assert!(behavior.is_valid_bonemeal_target(hanging, &level, BlockPos::ZERO));
        assert!(!behavior.is_valid_bonemeal_target(
            hanging.set_value(AGE, MAX_AGE),
            &level,
            BlockPos::ZERO,
        ));
        assert!(behavior.is_valid_bonemeal_target(
            vanilla_blocks::MANGROVE_PROPAGULE.default_state(),
            &level,
            BlockPos::new(0, 7, 0),
        ));
        assert!(!behavior.is_valid_bonemeal_target(
            vanilla_blocks::MANGROVE_PROPAGULE.default_state(),
            &level,
            BlockPos::new(0, 8, 0),
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
        let behavior = MangrovePropaguleBlock::new(&vanilla_blocks::MANGROVE_PROPAGULE);

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
            MangrovePropaguleBlock::grow_tree(&world, pos, state, &mut rng)
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
        let behavior = MangrovePropaguleBlock::new(&vanilla_blocks::MANGROVE_PROPAGULE);
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
    }
}
