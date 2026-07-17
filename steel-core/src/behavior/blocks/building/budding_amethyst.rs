use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt, blocks::AmethystBlock},
    entity::projectile::Projectile,
    fluid::FluidStateExt as _,
    world::{ClipHitResult, World},
};
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY, RegistryEntry,
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty, EnumProperty},
    },
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

/// Behavior for vanilla budding amethyst blocks.
#[block_behavior]
pub struct BuddingAmethystBlock {
    block: BlockRef,
}

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl BuddingAmethystBlock {
    /// Creates a new budding amethyst block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn can_cluster_grow_at_state(state: BlockStateId, block_id: usize) -> bool {
        state.is_air()
            || (block_id == vanilla_blocks::WATER.id() && state.get_fluid_state().is_full())
    }

    fn check_cluster(
        state: BlockStateId,
        block_id: usize,
        direction: Direction,
        block: BlockRef,
    ) -> bool {
        block_id == block.id() && state.get_value(FACING) == direction
    }

    fn growth_state(
        block: BlockRef,
        replaced_state: BlockStateId,
        direction: Direction,
    ) -> BlockStateId {
        block
            .default_state()
            .set_value(FACING, direction)
            .set_value(WATERLOGGED, replaced_state.get_fluid_state().is_water())
    }
}

impl BlockBehavior for BuddingAmethystBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if rand::random_range(0..5) == 0 {
            let direction = Direction::random();
            let grow_pos = pos.relative(direction);
            let state = world.get_block_state(grow_pos);
            let Some(&block_id) = REGISTRY.blocks.state_to_block_id.get(state.0 as usize) else {
                panic!(
                    "budding amethyst received invalid block state id {}",
                    state.0
                );
            };

            let mut growth_stage: Option<BlockRef> = None;
            if Self::can_cluster_grow_at_state(state, block_id) {
                growth_stage = Some(&vanilla_blocks::SMALL_AMETHYST_BUD);
            } else if Self::check_cluster(
                state,
                block_id,
                direction,
                &vanilla_blocks::SMALL_AMETHYST_BUD,
            ) {
                growth_stage = Some(&vanilla_blocks::MEDIUM_AMETHYST_BUD);
            } else if Self::check_cluster(
                state,
                block_id,
                direction,
                &vanilla_blocks::MEDIUM_AMETHYST_BUD,
            ) {
                growth_stage = Some(&vanilla_blocks::LARGE_AMETHYST_BUD);
            } else if Self::check_cluster(
                state,
                block_id,
                direction,
                &vanilla_blocks::LARGE_AMETHYST_BUD,
            ) {
                growth_stage = Some(&vanilla_blocks::AMETHYST_CLUSTER);
            }

            if let Some(growth_stage) = growth_stage {
                let block_state = Self::growth_state(growth_stage, state, direction);
                world.set_block(grow_pos, block_state, UpdateFlags::UPDATE_ALL);
            }
        }
    }
    fn on_projectile_hit(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        _projectile: &dyn Projectile,
    ) {
        AmethystBlock::play_projectile_hit_sound(world, hit.block_pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use steel_registry::{
        blocks::properties::BlockStateProperties, test_support::init_test_registry,
    };

    #[test]
    fn growth_state_waterlogs_when_replacing_water_block() {
        init_test_registry();
        init_behaviors();

        let state = BuddingAmethystBlock::growth_state(
            &vanilla_blocks::SMALL_AMETHYST_BUD,
            vanilla_blocks::WATER.default_state(),
            Direction::Up,
        );

        assert!(state.get_value(WATERLOGGED));
    }

    #[test]
    fn cluster_can_grow_in_falling_full_water() {
        init_test_registry();
        init_behaviors();

        let falling_full_water = vanilla_blocks::WATER
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, 8);

        assert!(BuddingAmethystBlock::can_cluster_grow_at_state(
            falling_full_water,
            vanilla_blocks::WATER.id()
        ));
    }

    #[test]
    fn cluster_cannot_grow_in_partial_flowing_water() {
        init_test_registry();
        init_behaviors();

        let partial_flowing_water = vanilla_blocks::WATER
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, 1);

        assert!(!BuddingAmethystBlock::can_cluster_grow_at_state(
            partial_flowing_water,
            vanilla_blocks::WATER.id()
        ));
    }
}
