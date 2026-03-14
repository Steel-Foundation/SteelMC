use std::{ops::Not, sync::Arc};

use steel_registry::{
    REGISTRY,
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BambooLeaves, BlockStateProperties, EnumProperty, IntProperty},
    },
    vanilla_block_tags, vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{BlockBehaviour, BlockPlaceContext, blocks::vegetation::bonemealable::Bonemealable},
    world::World,
};

/// Behavior for the Bamboo Stalk Block
/// TODO:
/// - [ ] brightness
/// - [ ] dont replace fluids
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
    pub fn can_survive(world: &World, pos: BlockPos) -> bool {
        REGISTRY.blocks.is_in_tag(
            world.get_block_state(&pos.below()).get_block(),
            &vanilla_block_tags::BAMBOO_PLANTABLE_ON_TAG,
        )
    }

    fn stalk_segments_below(world: &World, pos: BlockPos) -> i32 {
        let mut height = 0;
        while height < 16
            && world.get_block_state(&pos.below_n(height + 1)).get_block() == vanilla_blocks::BAMBOO
        {
            height += 1;
        }

        height
    }

    fn stalk_segments_above(world: &World, pos: BlockPos) -> i32 {
        let mut height = 0;
        while height < 16
            && world.get_block_state(&pos.above_n(height + 1)).get_block() == vanilla_blocks::BAMBOO
        {
            height += 1;
        }

        height
    }

    fn grow(world: &World, pos: BlockPos, state: BlockStateId, height: i32) {
        let state_below = world.get_block_state(&pos.below());
        let block_below = state_below.get_block();
        let state_two_below = world.get_block_state(&pos.below_n(2));
        let leaves = if height == 0 {
            BambooLeaves::None
        } else {
            let leaves_below = state_below.get_value(&BAMBOO_LEAVES_PROPERTY);

            if block_below != vanilla_blocks::BAMBOO || leaves_below == BambooLeaves::None {
                BambooLeaves::Small
            } else {
                if state_two_below.get_block() == vanilla_blocks::BAMBOO {
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
                BambooLeaves::Large
            }
        };

        let new_age = u8::from(
            state.get_value(&AGE_PROPERTY) == 1
                || state_two_below.get_block() == vanilla_blocks::BAMBOO,
        );

        let new_stage = u8::from(height == 15 || (height >= 11 && rand::random::<f32>() < 0.25));

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
}

impl Bonemealable for BambooStalkBlock {
    fn get_age_increase(&self, _world: &World) -> u8 {
        1 + rand::random_range(0..2)
    }

    fn is_bonemealable(&self, _state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        let above = Self::stalk_segments_above(world, pos);
        let below = Self::stalk_segments_below(world, pos);
        (above + below + 1 < 16)
            && world
                .get_block_state(&pos.above_n(above))
                .get_value(&BlockStateProperties::STAGE)
                != 1
    }

    fn apply_bonemeal(&self, _state: BlockStateId, world: &World, pos: BlockPos) {
        let above = Self::stalk_segments_above(world, pos);
        let below = Self::stalk_segments_below(world, pos);
        let total_height = above + below + 1;

        for i in 0..i32::from(self.get_age_increase(world)) {
            let pos_above = pos.above_n(above + i);
            let state_above = world.get_block_state(&pos_above);
            let state_two_above = world.get_block_state(&pos_above.above());
            if total_height + i >= 16
                || state_above.get_value(&BlockStateProperties::STAGE) == 1
                || !state_two_above.is_air()
            {
                return;
            }

            Self::grow(world, pos_above, state_above, total_height + i);
        }
    }
}

impl BlockBehaviour for BambooStalkBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // FIXME: dont replace fluid

        let state_below = context.world.get_block_state(&context.clicked_pos);
        let block_below = state_below.get_block();

        if !REGISTRY
            .blocks
            .is_in_tag(block_below, &vanilla_block_tags::BAMBOO_PLANTABLE_ON_TAG)
        {
            return None;
        }

        if block_below == vanilla_blocks::BAMBOO_SAPLING {
            Some(
                vanilla_blocks::BAMBOO
                    .default_state()
                    .set_value(&AGE_PROPERTY, 0),
            )
        } else if block_below == vanilla_blocks::BAMBOO {
            Some(vanilla_blocks::BAMBOO.default_state().set_value(
                &AGE_PROPERTY,
                state_below.get_value(&BlockStateProperties::AGE_1),
            ))
        } else {
            let state_above = context.world.get_block_state(&context.clicked_pos.above());
            if state_above.get_block() == vanilla_blocks::BAMBOO {
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

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        state.get_value(&BlockStateProperties::STAGE) == 0
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::STAGE) != 0 {
            return;
        }
        if rand::random_range(0..3) == 0 && world.get_block_state(&pos.above()).is_air() {
            // TODO: brightness

            let height = Self::stalk_segments_below(world, pos);
            if height < 16 {
                Self::grow(world, pos, state, height);
            }
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
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
            && neighbor_state.get_block() == vanilla_blocks::BAMBOO
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
