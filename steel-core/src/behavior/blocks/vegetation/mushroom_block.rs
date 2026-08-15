use std::ops::Sub;
use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::feature::ConfiguredFeatureRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::chunk_saver::PersistentProcessorList::Registry;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `MushroomBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct MushroomBlock {
    block: BlockRef,
    #[json_arg(vanilla_configured_features, json = "feature")]
    feature: ConfiguredFeatureRef,
}

impl MushroomBlock {
    /// Creates a new mushroom block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, feature: ConfiguredFeatureRef) -> Self {
        Self { block, feature: feature }
    }
    fn may_place_on(state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        state.is_solid_render()
    }
}

impl BlockBehavior for MushroomBlock {
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let mut random = rand::rng();
        if random.random_range(0..25) == 0 {
            let max = 5;

            for block_pos in BlockPos::between_closed(pos.offset(-4, -1, -4), pos.offset(4, 1, -4))
            {
                if world.get_block_state(block_pos).get_block() == self.block {
                    if max - 1 <= 0 {
                        return;
                    }
                }
            }

            let mut offset = pos.offset(
                random.random_range(0..3) - 1,
                random.random_range(0..2) - random.random_range(0..2),
                random.random_range(0..3) - 1,
            );
            let mut pos = pos;
            for _ in 0..4 {
                if world.get_block_state(offset).is_air() && self.can_survive(state, world, offset)
                {
                    pos = offset;
                }

                offset = pos.offset(
                    random.random_range(0..3) - 1,
                    random.random_range(0..2) - random.random_range(0..2),
                    random.random_range(0..3) - 1,
                );
            }

            if world.get_block_state(offset).is_air() && self.can_survive(state, world, offset) {
                world.set_block(offset, state, UpdateFlags::UPDATE_CLIENTS);
            }
        }
    }
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        if below
            .get_block()
            .has_tag(&BlockTag::OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT)
        {
            return true;
        }

        world.raw_brightness(pos, 0) < 13 && Self::may_place_on(below, world, below_pos)
    }

    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for MushroomBlock {
    fn is_valid_bonemeal_target(
    &self,
    state: BlockStateId,
    world: &dyn LevelReader,
    pos: BlockPos,
) -> bool
{
    let feature_holder = REGISTRY.configured_features.
}
    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        rng.random::<f32>() < 0.4
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, init_vanilla_registry, vanilla_blocks};

    use crate::test_support::TestLevel;

    use super::*;

    fn single_support_level(support: BlockStateId, raw_brightness: u8) -> TestLevel {
        TestLevel::default()
            .with_block(BlockPos::ZERO.below(), support)
            .with_raw_brightness(raw_brightness)
    }

    #[test]
    fn mushroom_survival_uses_solid_render_support() {
        init_vanilla_registry();

        let mushroom = MushroomBlock::new(&vanilla_blocks::BROWN_MUSHROOM);
        let state = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::BROWN_MUSHROOM);
        let pos = BlockPos::new(0, 0, 0);

        let grass_block = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::GRASS_BLOCK);
        assert!(mushroom.can_survive(state, &single_support_level(grass_block, 12), pos));
        assert!(!mushroom.can_survive(state, &single_support_level(grass_block, 13), pos));

        let oak_leaves = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::OAK_LEAVES);
        assert!(!mushroom.can_survive(state, &single_support_level(oak_leaves, 0), pos));

        let podzol = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::PODZOL);
        assert!(mushroom.can_survive(state, &single_support_level(podzol, 15), pos));
    }
}
