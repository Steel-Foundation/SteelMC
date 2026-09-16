use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::level_events;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::projectile::Projectile;
use crate::world::{ClipHitResult, LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, ChorusPlantBlock, default_surviving_state};

const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// Vanilla `ChorusFlowerBlock` survival behavior.
#[block_behavior]
pub struct ChorusFlowerBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks)]
    plant: BlockRef,
}

const AGE: &IntProperty = &BlockStateProperties::AGE_5;
const DEAD_AGE: u8 = 5;
const PILLAR_SCAN_DEPTH: i32 = 4;
const MIN_PILLAR_HEIGHT: i32 = 2;
const GROWTH_RANDOM_RANGE: i32 = 4;
const GROWTH_RANDOM_RANGE_ON_SUPPORT: i32 = 5;
const BRANCH_ATTEMPT_RANDOM_RANGE: i32 = 4;

impl ChorusFlowerBlock {
    /// Creates a new chorus flower block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, plant: BlockRef) -> Self {
        Self { block, plant }
    }

    fn projectile_can_break(projectile: &dyn Projectile, world: &World, pos: BlockPos) -> bool {
        projectile.projectile_may_interact(world, pos) && projectile.may_break(world)
    }

    fn all_neighbors_empty(
        world: &dyn LevelReader,
        pos: BlockPos,
        ignore: Option<Direction>,
    ) -> bool {
        HORIZONTAL_DIRECTIONS.into_iter().all(|direction| {
            Some(direction) == ignore || world.get_block_state(pos.relative(direction)).is_air()
        })
    }

    fn place_grown_flower(&self, world: &Arc<World>, pos: BlockPos, age: u8) {
        world.set_block(
            pos,
            self.block.default_state().set_value(AGE, age),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.level_event(level_events::SOUND_CHORUS_GROW, pos, 0, None);
    }

    fn place_dead_flower(&self, world: &Arc<World>, pos: BlockPos) {
        world.set_block(
            pos,
            self.block.default_state().set_value(AGE, DEAD_AGE),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.level_event(level_events::SOUND_CHORUS_DEATH, pos, 0, None);
    }

    fn random_tick_with_rng(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        rng: &mut dyn Rng,
    ) {
        let above = pos.above();
        if !world.get_block_state(above).is_air() || world.is_outside_build_height(above.y()) {
            return;
        }

        let current_age = state.get_value(AGE);
        if current_age >= DEAD_AGE {
            return;
        }

        let mut grow_upwards = false;
        let mut pillar_on_support_block = false;
        let below = world.get_block_state(pos.below());
        if below.get_block().has_tag(&BlockTag::SUPPORTS_CHORUS_FLOWER) {
            grow_upwards = true;
        } else if below.get_block() == self.plant {
            let mut height = 1;
            for _ in 0..PILLAR_SCAN_DEPTH {
                let test_state = world.get_block_state(pos.below_n(height + 1));
                if test_state.get_block() != self.plant {
                    pillar_on_support_block = test_state
                        .get_block()
                        .has_tag(&BlockTag::SUPPORTS_CHORUS_FLOWER);
                    break;
                }
                height += 1;
            }

            if height < MIN_PILLAR_HEIGHT
                || height
                    <= rng.random_range(
                        0..if pillar_on_support_block {
                            GROWTH_RANDOM_RANGE_ON_SUPPORT
                        } else {
                            GROWTH_RANDOM_RANGE
                        },
                    )
            {
                grow_upwards = true;
            }
        } else if below.is_air() {
            grow_upwards = true;
        }

        if grow_upwards
            && Self::all_neighbors_empty(world, above, None)
            && world.get_block_state(pos.above_n(2)).is_air()
        {
            world.set_block(
                pos,
                ChorusPlantBlock::state_with_connections(world, pos, self.plant.default_state()),
                UpdateFlags::UPDATE_CLIENTS,
            );
            self.place_grown_flower(world, above, current_age);
        } else if current_age < DEAD_AGE - 1 {
            let mut branch_attempts = rng.random_range(0..BRANCH_ATTEMPT_RANDOM_RANGE);
            if pillar_on_support_block {
                branch_attempts += 1;
            }

            let mut created_branch = false;
            for _ in 0..branch_attempts {
                let direction =
                    HORIZONTAL_DIRECTIONS[rng.random_range(0..HORIZONTAL_DIRECTIONS.len())];
                let target = pos.relative(direction);
                if world.get_block_state(target).is_air()
                    && world.get_block_state(target.below()).is_air()
                    && Self::all_neighbors_empty(world, target, Some(direction.opposite()))
                {
                    self.place_grown_flower(world, target, current_age + 1);
                    created_branch = true;
                }
            }

            if created_branch {
                world.set_block(
                    pos,
                    ChorusPlantBlock::state_with_connections(
                        world,
                        pos,
                        self.plant.default_state(),
                    ),
                    UpdateFlags::UPDATE_CLIENTS,
                );
            } else {
                self.place_dead_flower(world, pos);
            }
        } else {
            self.place_dead_flower(world, pos);
        }
    }
}

impl BlockBehavior for ChorusFlowerBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_state = world.get_block_state(pos.below());
        if below_state.get_block() == self.plant
            || below_state
                .get_block()
                .has_tag(&BlockTag::SUPPORTS_CHORUS_FLOWER)
        {
            return true;
        }

        if !below_state.is_air() {
            return false;
        }

        let mut has_single_plant_neighbor = false;
        for direction in HORIZONTAL_DIRECTIONS {
            let neighbor_state = world.get_block_state(pos.relative(direction));
            if neighbor_state.get_block() == self.plant {
                if has_single_plant_neighbor {
                    return false;
                }
                has_single_plant_neighbor = true;
            } else if !neighbor_state.is_air() {
                return false;
            }
        }

        has_single_plant_neighbor
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.random_tick_with_rng(state, world, pos, &mut rand::rng());
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
        if direction != Direction::Up && !self.can_survive(state, world, pos) {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        state
    }

    fn on_projectile_hit(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        if Self::projectile_can_break(projectile, world, hit.block_pos) {
            world.destroy_block_by_entity(hit.block_pos, true, projectile.as_entity_event_source());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rand::TryRng;
    use rand::{SeedableRng, rngs::StdRng};
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::ChunkPos;

    use crate::{
        behavior::init_behaviors,
        test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk},
    };

    use super::*;

    fn behavior() -> ChorusFlowerBlock {
        ChorusFlowerBlock::new(
            &vanilla_blocks::CHORUS_FLOWER,
            &vanilla_blocks::CHORUS_PLANT,
        )
    }

    #[derive(Default)]
    struct MaxRng;

    impl TryRng for MaxRng {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            Ok(u32::MAX)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(u64::MAX)
        }

        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            dst.fill(u8::MAX);
            Ok(())
        }
    }

    #[test]
    fn grows_upward_from_chorus_support() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("chorus_flower_growth");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::END_STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let state = vanilla_blocks::CHORUS_FLOWER.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        behavior().random_tick_with_rng(state, &world, pos, &mut StdRng::seed_from_u64(0));

        let stem = world.get_block_state(pos);
        assert_eq!(stem.get_block(), &vanilla_blocks::CHORUS_PLANT);
        assert!(stem.get_value(&BlockStateProperties::UP));
        let grown = world.get_block_state(pos.above());
        assert_eq!(grown.get_block(), &vanilla_blocks::CHORUS_FLOWER);
        assert_eq!(grown.get_value(AGE), 0);
    }

    #[test]
    fn mature_flower_dies_when_it_cannot_grow() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("chorus_flower_death");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let state = vanilla_blocks::CHORUS_FLOWER
            .default_state()
            .set_value(AGE, DEAD_AGE - 1);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        behavior().random_tick_with_rng(state, &world, pos, &mut StdRng::seed_from_u64(0));

        assert_eq!(world.get_block_state(pos).get_value(AGE), DEAD_AGE);
    }

    #[test]
    fn creates_branches_when_upward_growth_is_obstructed() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("chorus_flower_branching");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::END_STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            pos.above().north(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let state = vanilla_blocks::CHORUS_FLOWER.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        behavior().random_tick_with_rng(state, &world, pos, &mut MaxRng);

        assert_eq!(
            world.get_block_state(pos).get_block(),
            &vanilla_blocks::CHORUS_PLANT
        );
        assert!(HORIZONTAL_DIRECTIONS.into_iter().any(|direction| {
            let branch = world.get_block_state(pos.relative(direction));
            branch.get_block() == &vanilla_blocks::CHORUS_FLOWER && branch.get_value(AGE) == 1
        }));
    }

    #[test]
    fn unsupported_flower_breaks_on_scheduled_tick() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("chorus_flower_survival");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let state = vanilla_blocks::CHORUS_FLOWER.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        behavior().tick(state, &world, pos);

        assert!(world.get_block_state(pos).is_air());
    }

    #[test]
    fn schedules_survival_tick_when_non_up_neighbor_changes() {
        init_vanilla_registry();
        let level = TestLevel::default();
        let pos = BlockPos::ZERO;
        let state = vanilla_blocks::CHORUS_FLOWER.default_state();

        assert_eq!(
            behavior().update_shape(
                state,
                &level,
                pos,
                Direction::Down,
                pos.below(),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );

        assert_eq!(level.scheduled_block_ticks.borrow().len(), 1);
        let tick = level.scheduled_block_ticks.borrow()[0];
        assert_eq!(tick.pos, pos);
        assert_eq!(tick.block, &vanilla_blocks::CHORUS_FLOWER);
        assert_eq!(tick.delay, 1);
    }
}
