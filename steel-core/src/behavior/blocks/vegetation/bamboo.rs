use std::{ops::Not, sync::Arc};

use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BambooLeaves, BlockStateProperties, EnumProperty, IntProperty},
    },
    vanilla_block_tags, vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt,
        blocks::vegetation::bonemealable::Bonemealable,
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for the Bamboo Stalk Block
#[block_behavior]
pub struct BambooStalkBlock;

const BAMBOO_LEAVES_PROPERTY: EnumProperty<BambooLeaves> = BlockStateProperties::BAMBOO_LEAVES;
const AGE_PROPERTY: IntProperty = BlockStateProperties::AGE_1;

impl BambooStalkBlock {
    /// Creates a new Bamboo Stalk Behavior
    #[must_use]
    pub const fn new(_block: BlockRef) -> Self {
        Self
    }

    /// Checks if the Block below is in the tag `BAMBOO_PLANTABLE_ON`
    pub fn can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
        REGISTRY.blocks.is_in_tag(
            world.get_block_state(pos.below()).get_block(),
            &vanilla_block_tags::SUPPORTS_BAMBOO_TAG,
        )
    }

    fn stalk_segments_below(world: &dyn LevelReader, pos: BlockPos) -> i32 {
        let mut height = 0;
        while height < 16
            && world.get_block_state(pos.below_n(height + 1)).get_block() == &vanilla_blocks::BAMBOO
        {
            height += 1;
        }

        height
    }

    fn stalk_segments_above(world: &dyn LevelReader, pos: BlockPos) -> i32 {
        let mut height = 0;
        while height < 16
            && world.get_block_state(pos.above_n(height + 1)).get_block() == &vanilla_blocks::BAMBOO
        {
            height += 1;
        }

        height
    }

    fn grow(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn rand::Rng,
        height: i32,
    ) {
        let state_below = world.get_block_state(pos.below());
        let state_two_below = world.get_block_state(pos.below_n(2));
        let leaves = if height == 0 {
            BambooLeaves::None
        } else {
            let leaves = Self::leaves_for_new_segment(state_below);
            if leaves == BambooLeaves::Large
                && state_two_below.get_block() == &vanilla_blocks::BAMBOO
            {
                world.set_block(
                    pos.below(),
                    state_below.set_value(&BAMBOO_LEAVES_PROPERTY, BambooLeaves::Small),
                    UpdateFlags::UPDATE_ALL,
                );
                world.set_block(
                    pos.below_n(2),
                    state_two_below.set_value(&BAMBOO_LEAVES_PROPERTY, BambooLeaves::None),
                    UpdateFlags::UPDATE_ALL,
                );
            }
            leaves
        };

        let new_age = u8::from(
            state.get_value(&AGE_PROPERTY) == 1
                || state_two_below.get_block() == &vanilla_blocks::BAMBOO,
        );

        let new_stage = u8::from(height == 15 || (height >= 11 && rng.random::<f32>() < 0.25));

        world.set_block(
            pos.above(),
            vanilla_blocks::BAMBOO
                .default_state()
                .set_value(&AGE_PROPERTY, new_age)
                .set_value(&BlockStateProperties::STAGE, new_stage)
                .set_value(&BlockStateProperties::BAMBOO_LEAVES, leaves),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn leaves_for_new_segment(state_below: BlockStateId) -> BambooLeaves {
        if state_below.get_block() != &vanilla_blocks::BAMBOO
            || state_below.get_value(&BAMBOO_LEAVES_PROPERTY) == BambooLeaves::None
        {
            BambooLeaves::Small
        } else {
            BambooLeaves::Large
        }
    }
}

impl Bonemealable for BambooStalkBlock {
    fn get_bonemeal_age_increase(&self, _world: &Arc<World>, rng: &mut dyn rand::Rng) -> u8 {
        1 + rng.random_range(0..2)
    }

    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let above = Self::stalk_segments_above(world, pos);
        let below = Self::stalk_segments_below(world, pos);
        let growth_pos = pos.above_n(above + 1);
        (above + below + 1 < 16)
            && world
                .get_block_state(pos.above_n(above))
                .get_value(&BlockStateProperties::STAGE)
                != 1
            && !world.is_outside_build_height(growth_pos.y())
            && world.get_block_state(growth_pos).is_air()
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        let above = Self::stalk_segments_above(world, pos);
        let below = Self::stalk_segments_below(world, pos);
        let total_height = above + below + 1;

        for i in 0..i32::from(self.get_bonemeal_age_increase(world, rng)) {
            let pos_above = pos.above_n(above + i);
            let state_above = world.get_block_state(pos_above);
            let growth_pos = pos_above.above();
            if total_height + i >= 16
                || state_above.get_value(&BlockStateProperties::STAGE) == 1
                || !world.is_in_valid_bounds(growth_pos)
                || !world.get_block_state(growth_pos).is_air()
            {
                return;
            }

            Self::grow(world, pos_above, state_above, rng, total_height + i);
        }
    }
}

impl BlockBehavior for BambooStalkBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if !context
            .world
            .get_block_state(context.relative_pos)
            .get_fluid_state()
            .is_empty()
        {
            return None;
        }

        let state_below = context.world.get_block_state(context.relative_pos.below());
        let block_below = state_below.get_block();

        if !REGISTRY
            .blocks
            .is_in_tag(block_below, &vanilla_block_tags::SUPPORTS_BAMBOO_TAG)
        {
            return None;
        }

        if block_below == &vanilla_blocks::BAMBOO_SAPLING {
            Some(
                vanilla_blocks::BAMBOO
                    .default_state()
                    .set_value(&AGE_PROPERTY, 0),
            )
        } else if block_below == &vanilla_blocks::BAMBOO {
            Some(vanilla_blocks::BAMBOO.default_state().set_value(
                &AGE_PROPERTY,
                state_below.get_value(&BlockStateProperties::AGE_1),
            ))
        } else {
            let state_above = context.world.get_block_state(context.relative_pos.above());
            if state_above.get_block() == &vanilla_blocks::BAMBOO {
                Some(
                    vanilla_blocks::BAMBOO
                        .default_state()
                        .set_value(&AGE_PROPERTY, state_above.get_value(&AGE_PROPERTY)),
                )
            } else {
                Some(vanilla_blocks::BAMBOO_SAPLING.default_state())
            }
        }
    }

    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !Self::can_survive(world, pos) {
            world.destroy_block(pos, true);
        }
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::can_survive(world, pos)
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        state.get_value(&BlockStateProperties::STAGE) == 0
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::STAGE) != 0 {
            return;
        }
        let mut rng = rand::rng();
        if rng.random_range(0..3) == 0
            && world.get_block_state(pos.above()).is_air()
            && world.raw_brightness(pos.above(), 0) >= 9
        {
            let height = Self::stalk_segments_below(world, pos) + 1;
            if height < 16 {
                Self::grow(world, pos, state, &mut rng, height);
            }
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !Self::can_survive(world, pos) {
            world.schedule_block_tick_default(pos, state.get_block(), 1);
        }

        let age = state.get_value(&AGE_PROPERTY);

        if direction == Direction::Up
            && neighbor_state.get_block() == &vanilla_blocks::BAMBOO
            && neighbor_state.get_value(&AGE_PROPERTY) > age
        {
            return state.set_value(&AGE_PROPERTY, age.not() & 1); // 0 => 1; 1 => 0
        }
        state
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, Registry, vanilla_blocks};

    use super::*;

    fn init_registry() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    }

    #[test]
    fn bamboo_growth_does_not_read_leaves_from_non_bamboo_support() {
        init_registry();
        let dirt = vanilla_blocks::DIRT.default_state();

        assert_eq!(
            BambooStalkBlock::leaves_for_new_segment(dirt),
            BambooLeaves::Small
        );
    }
}
