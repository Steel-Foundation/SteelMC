//! Crop block implementation (wheat, carrots, potatoes, beetroot).

use std::sync::Arc;

use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{vanilla_entities, vanilla_game_rules, vanilla_items};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::Vegetation;
use crate::behavior::blocks::vegetation::bonemealable::{Bonemealable, CropBonemealExt};
use crate::behavior::blocks::vegetation::vegetation_block::{
    survival_update_shape, vegetation_can_survive,
};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::world::{LevelReader, ScheduledTickAccess, World};

/// Behavior for crop blocks (wheat, carrots, potatoes).
///
/// Crops grow through random ticks when placed on farmland with sufficient light.
/// Growth speed is affected by nearby farmland moisture and crop arrangement.
#[block_behavior]
pub struct CropBlock {
    block: BlockRef,
}

/// Calculates Vanilla crop growth speed for the supplied crop block.
///
/// Stems use the same farmland and neighboring-crop calculation as crops, but
/// have different survival and mature-growth behavior.
pub(super) fn crop_growth_speed(block: BlockRef, world: &dyn LevelReader, pos: BlockPos) -> f32 {
    let mut speed = 1.0_f32;
    let below = pos.below();

    for dx in -1..=1 {
        for dz in -1..=1 {
            let state = world.get_block_state(below.offset(dx, 0, dz));
            if !state.get_block().has_tag(&BlockTag::GROWS_CROPS) {
                continue;
            }

            let block_speed = if state
                .try_get_value(&BlockStateProperties::MOISTURE)
                .unwrap_or(0)
                > 0
            {
                3.0
            } else {
                1.0
            };

            speed += if dx == 0 && dz == 0 {
                block_speed
            } else {
                block_speed / 4.0
            };
        }
    }

    let north = pos.north();
    let south = pos.south();
    let west = pos.west();
    let east = pos.east();
    let same_block_at = |neighbor: BlockPos| block == world.get_block_state(neighbor).get_block();

    let east_west = [west, east]
        .into_iter()
        .any(|neighbor| same_block_at(neighbor));
    let north_south = [north, south]
        .into_iter()
        .any(|neighbor| same_block_at(neighbor));
    let crowded = (east_west && north_south)
        || [west.north(), east.north(), east.south(), west.south()]
            .into_iter()
            .any(same_block_at);

    if crowded { speed / 2.0 } else { speed }
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
        crop_growth_speed(self.block(), world.as_ref(), pos)
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

pub(super) fn ravager_breaks_crop(entity_type: EntityTypeRef, mob_griefing: bool) -> bool {
    entity_type == &vanilla_entities::RAVAGER && mob_griefing
}

pub(super) fn destroy_crop_on_ravager_contact(
    world: &Arc<World>,
    pos: BlockPos,
    entity: &dyn Entity,
) {
    if ravager_breaks_crop(
        entity.entity_type(),
        world.get_game_rule(&vanilla_game_rules::MOB_GRIEFING),
    ) {
        world.destroy_block_by_entity(pos, true, entity);
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
        ItemStack::new(&vanilla_items::WHEAT_SEEDS)
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
            context.world.get_block_state(context.place_pos().below()),
            context.world,
            context.place_pos().below(),
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
        survival_update_shape(self, state, world, pos)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if self.should_random_tick() {
            self.on_random_tick(state, world, pos);
        }
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        is_precise: bool,
    ) {
        destroy_crop_on_ravager_contact(world, pos, entity);
        self.default_entity_inside(state, world, pos, entity, effect_collector, is_precise);
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
        state.get_block().has_tag(&BlockTag::SUPPORTS_CROPS)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks};

    use crate::test_support::TestLevel;

    use super::*;

    fn crop_survival_level(support: BlockStateId, raw_brightness: u8) -> TestLevel {
        TestLevel::default()
            .with_block(BlockPos::ZERO.below(), support)
            .with_raw_brightness(raw_brightness)
    }

    #[test]
    fn crop_survival_requires_vanilla_minimum_light() {
        init_vanilla_registry();

        let crop = CropBlock::new(&vanilla_blocks::WHEAT);
        let state = vanilla_blocks::WHEAT.default_state();
        let farmland = vanilla_blocks::FARMLAND.default_state();

        assert!(!crop.can_survive(state, &crop_survival_level(farmland, 7), BlockPos::ZERO));
        assert!(crop.can_survive(state, &crop_survival_level(farmland, 8), BlockPos::ZERO));
    }

    #[test]
    fn ravager_crop_breaking_requires_mob_griefing() {
        assert!(ravager_breaks_crop(&vanilla_entities::RAVAGER, true));
        assert!(!ravager_breaks_crop(&vanilla_entities::RAVAGER, false));
        assert!(!ravager_breaks_crop(&vanilla_entities::ZOMBIE, true));
    }
}
