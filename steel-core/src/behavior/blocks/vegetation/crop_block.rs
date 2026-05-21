//! Crop block implementation (wheat, carrots, potatoes, beetroot).

use std::sync::Arc;

use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, vanilla_block_tags, vanilla_entities, vanilla_game_rules,
    vanilla_items,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::Vegetation;
use crate::behavior::blocks::vegetation::bonemealable::{Bonemealable, CropBonemealExt};
use crate::behavior::blocks::vegetation::vegetation_block::{
    vegetation_can_survive, vegetation_update_shape,
};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::world::{LevelReader, ScheduledTickAccess, World};

/// Behavior for crop blocks (wheat, carrots, potatoes).
///
/// Crops grow through random ticks when placed on farmland with sufficient light.
/// Growth speed is affected by nearby farmland moisture and crop arrangement.
#[block_behavior]
pub struct CropBlock {
    block: BlockRef,
}

pub trait CropLike {
    fn block(&self) -> BlockRef;
    fn age_property(&self) -> &IntProperty;
    fn max_age(&self) -> u8;
    fn clone_item_stack(&self) -> ItemStack;

    /// Additional checks before calling the standard `on_random_tick` code
    fn should_random_tick(&self) -> bool {
        true
    }

    fn get_age(&self, state: BlockStateId) -> u8 {
        state.get_value(self.age_property())
    }

    fn get_state_for_age(&self, age: u8) -> BlockStateId {
        self.block()
            .default_state()
            .set_value(self.age_property(), age)
    }

    fn is_max_age(&self, state: BlockStateId) -> bool {
        state.get_value(self.age_property()) >= self.max_age()
    }

    fn has_sufficient_light(&self, world: &dyn LevelReader, pos: BlockPos) -> bool {
        world.raw_brightness(pos, 0) >= 8
    }

    fn has_sufficient_growth_light(&self, world: &dyn LevelReader, pos: BlockPos) -> bool {
        world.raw_brightness(pos, 0) >= 9
    }

    /// Calculates the growth speed based on surrounding farmland.
    ///
    /// Factors affecting growth speed:
    /// - Farmland below: +1.0 (dry) or +3.0 (hydrated)
    /// - Adjacent farmland: +0.25 (dry) or +0.75 (hydrated)
    /// - Same crop in row: /2.0 speed penalty
    fn get_growth_speed(&self, world: &Arc<World>, pos: BlockPos) -> f32 {
        let mut speed = 1.0f32;
        let below = pos.below();

        // Check 3x3 area of farmland below
        for dx in -1..=1 {
            for dz in -1..=1 {
                let check_pos = below.offset(dx, 0, dz);
                let block_state = world.get_block_state(check_pos);
                let mut block_speed = 0.0f32;

                if steel_registry::REGISTRY.blocks.is_in_tag(
                    block_state.get_block(),
                    &vanilla_block_tags::GROWS_CROPS_TAG,
                ) {
                    block_speed = 1.0;
                    // Check moisture level (defaults to 0 for non-farmland blocks)
                    let moisture = block_state
                        .try_get_value(&BlockStateProperties::MOISTURE)
                        .unwrap_or(0);
                    if moisture > 0 {
                        block_speed = 3.0;
                    }
                }

                // Diagonal/adjacent farmland contributes less
                if dx != 0 || dz != 0 {
                    block_speed /= 4.0;
                }

                speed += block_speed;
            }
        }

        // Check for same crop in adjacent positions (reduces growth speed)
        let north = world.get_block_state(pos.north());
        let south = world.get_block_state(pos.south());
        let west = world.get_block_state(pos.west());
        let east = world.get_block_state(pos.east());

        let block = self.block();

        let horizontal_row = block == west.get_block() || block == east.get_block();
        let vertical_row = block == north.get_block() || block == south.get_block();

        if horizontal_row && vertical_row {
            // Crops in both directions - penalty
            speed /= 2.0;
        } else {
            // Check diagonals
            let nw = world.get_block_state(pos.north().west());
            let ne = world.get_block_state(pos.north().east());
            let sw = world.get_block_state(pos.south().west());
            let se = world.get_block_state(pos.south().east());

            let has_diagonal = block == nw.get_block()
                || block == ne.get_block()
                || block == sw.get_block()
                || block == se.get_block();

            if has_diagonal {
                speed /= 2.0;
            }
        }

        speed
    }

    fn on_random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.has_sufficient_growth_light(world.as_ref(), pos) {
            return;
        }

        let age = self.get_age(state);
        if age < self.max_age() {
            let growth_speed = self.get_growth_speed(world, pos);

            // Random chance to grow based on growth speed
            // Vanilla formula: random.nextInt((int)(25.0F / growthSpeed) + 1) == 0
            let growth_chance = (25.0 / growth_speed) as u32 + 1;

            if rand::random::<u32>().is_multiple_of(growth_chance) {
                let new_state = self.get_state_for_age(age + 1);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            }
        }
    }
}

impl CropBlock {
    /// Creates a new crop block behavior with a custom age property.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl CropLike for CropBlock {
    fn block(&self) -> BlockRef {
        self.block
    }

    fn age_property(&self) -> &IntProperty {
        &BlockStateProperties::AGE_7
    }

    fn max_age(&self) -> u8 {
        7
    }

    fn clone_item_stack(&self) -> ItemStack {
        ItemStack::new(&vanilla_items::ITEMS.wheat_seeds)
    }
}

impl Bonemealable for CropBlock {
    fn get_bonemeal_age_increase(&self, _world: &Arc<World>, rng: &mut dyn rand::Rng) -> u8 {
        rng.random_range(2..=5)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        self.default_perform_bonemeal(state, world, rng, pos);
    }

    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        !self.is_max_age(state)
    }
}

impl<T: CropLike + Bonemealable + Send + Sync> BlockBehavior for T {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(context.relative_pos.below()),
            context.world,
            context.relative_pos.below(),
        ) {
            Some(self.block().default_state())
        } else {
            None
        }
    }

    fn can_survive(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: steel_utils::BlockPos,
    ) -> bool {
        self.has_sufficient_light(world, pos) && vegetation_can_survive(self, state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: steel_utils::BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: steel_utils::BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        // Only tick if not fully grown
        !self.is_max_age(state)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if self.should_random_tick() {
            self.on_random_tick(state, world, pos);
        }
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
    ) {
        if entity.entity_type() == &vanilla_entities::RAVAGER
            && world
                .get_game_rule(&vanilla_game_rules::MOB_GRIEFING)
                .as_bool()
                == Some(true)
        {
            world.destroy_block(pos, true);
        }
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(self.clone_item_stack())
    }
}

impl<T: CropLike> Vegetation for T {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        REGISTRY
            .blocks
            .is_in_tag(state.get_block(), &vanilla_block_tags::SUPPORTS_CROPS_TAG)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    struct CropSurvivalLevel {
        support: BlockStateId,
        air: BlockStateId,
        raw_brightness: u8,
    }

    impl CropSurvivalLevel {
        fn new(support: BlockStateId, raw_brightness: u8) -> Self {
            Self {
                support,
                air: vanilla_blocks::AIR.default_state(),
                raw_brightness,
            }
        }
    }

    impl LevelReader for CropSurvivalLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == BlockPos::ZERO.below() {
                self.support
            } else {
                self.air
            }
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            self.raw_brightness
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    #[test]
    fn crop_survival_requires_vanilla_minimum_light() {
        init_test_registry();

        let crop = CropBlock::new(&vanilla_blocks::WHEAT);
        let state = vanilla_blocks::WHEAT.default_state();
        let farmland = vanilla_blocks::FARMLAND.default_state();

        assert!(!crop.can_survive(state, &CropSurvivalLevel::new(farmland, 7), BlockPos::ZERO));
        assert!(crop.can_survive(state, &CropSurvivalLevel::new(farmland, 8), BlockPos::ZERO));
    }
}
