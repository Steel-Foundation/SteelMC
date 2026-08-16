use std::sync::{Arc, LazyLock};

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::feature::{ConfiguredFeature, ConfiguredFeatureKind};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

use steel_utils::random::worldgen_random::WorldgenRandom;

use crate::worldgen::feature::FeatureDecorationRunner;

/// Vanilla `MushroomBlock` survival and growth.
#[block_behavior]
pub struct MushroomBlock {
    block: BlockRef,
    #[json_arg(vanilla_configured_features, json = "feature")]
    feature: &'static LazyLock<ConfiguredFeature>,
}

impl MushroomBlock {
    /// Creates a new mushroom block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, feature: &'static LazyLock<ConfiguredFeature>) -> Self {
        Self { block, feature }
    }
    fn may_place_on(state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        state.is_solid_render()
    }
    fn grow_mushroom(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        random: &mut dyn Rng,
    ) -> bool {
        let configured_feature = &**self.feature;
        world.remove_block(pos, false);

        let mut worldgen_random = WorldgenRandom::from_seed(random.random());
        let placed = match &configured_feature.kind {
            ConfiguredFeatureKind::HugeBrownMushroom(config) => {
                FeatureDecorationRunner::place_huge_brown_mushroom_feature(
                    world,
                    &REGISTRY,
                    &mut worldgen_random,
                    config,
                    pos,
                )
            }
            ConfiguredFeatureKind::HugeRedMushroom(config) => {
                FeatureDecorationRunner::place_huge_red_mushroom_feature(
                    world,
                    &REGISTRY,
                    &mut worldgen_random,
                    config,
                    pos,
                )
            }
            _ => false,
        };

        if placed {
            true
        } else {
            world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
            false
        }
    }
}

impl BlockBehavior for MushroomBlock {
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let mut random = rand::rng();
        if random.random_range(0..25) == 0 {
            let max = 5;

            for block_pos in BlockPos::between_closed(pos.offset(-4, -1, -4), pos.offset(4, 1, -4))
            {
                if world.get_block_state(block_pos).get_block() == self.block && max - 1 <= 0 {
                    return;
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
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let configured_feature = &**self.feature;
        let (ConfiguredFeatureKind::HugeBrownMushroom(config)
        | ConfiguredFeatureKind::HugeRedMushroom(config)) = &configured_feature.kind
        else {
            return false;
        };
        let min_height = 4 + config.foliage_radius;

        !world.is_outside_build_height(pos.above_n(min_height).y())
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
    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.grow_mushroom(world, pos, state, rng);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        REGISTRY, init_vanilla_registry, vanilla_blocks, vanilla_configured_features,
    };

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

        let mushroom = MushroomBlock::new(
            &vanilla_blocks::BROWN_MUSHROOM,
            &vanilla_configured_features::HUGE_BROWN_MUSHROOM,
        );
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

    #[test]
    fn mushroom_bonemeal_validation_checks_build_height() {
        init_vanilla_registry();

        let mushroom = MushroomBlock::new(
            &vanilla_blocks::BROWN_MUSHROOM,
            &vanilla_configured_features::HUGE_BROWN_MUSHROOM,
        );
        let state = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::BROWN_MUSHROOM);
        let pos = BlockPos::new(0, 0, 0);

        let grass_block = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::GRASS_BLOCK);
        let level = single_support_level(grass_block, 0);
        assert!(mushroom.is_valid_bonemeal_target(state, &level, pos));
    }
}
