use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _};
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_damage_types;
use steel_registry::{sound_events, vanilla_blocks, vanilla_fluids, vanilla_game_events};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::schedule_water_tick_if_waterlogged;
use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, block::schedule_placed_liquid_tick},
    entity::{Entity, InsideBlockEffectCollector, damage::DamageSource, projectile::Projectile},
    world::{
        ClipHitResult, LevelAccessor, ScheduledTickAccess, World, game_event::GameEventContext,
    },
};

/// Behavior for campfires and soul campfires.
///
/// TODO: Add campfire cooking, smoke particles, and dowse item ejection.
#[block_behavior]
pub struct CampfireBlock {
    block: BlockRef,
    #[json_arg(value, json = "spawn_particles")]
    _spawn_particles: bool,
    #[json_arg(value, json = "fire_damage")]
    fire_damage: i32,
}

const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;
const LIT: &BoolProperty = &BlockStateProperties::LIT;
const SIGNAL_FIRE: &BoolProperty = &BlockStateProperties::SIGNAL_FIRE;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl CampfireBlock {
    /// Creates a campfire block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, spawn_particles: bool, fire_damage: i32) -> Self {
        Self {
            block,
            _spawn_particles: spawn_particles,
            fire_damage,
        }
    }

    #[must_use]
    fn contact_damage_amount(&self, state: BlockStateId, is_living_entity: bool) -> Option<f32> {
        if state.get_value(LIT) && is_living_entity {
            Some(self.fire_damage as f32)
        } else {
            None
        }
    }

    fn is_smoke_source(state: BlockStateId) -> bool {
        state.get_block() == &vanilla_blocks::HAY_BLOCK
    }

    fn placement_state(
        &self,
        waterlogged: bool,
        below_state: BlockStateId,
        facing: Direction,
    ) -> BlockStateId {
        self.block
            .default_state()
            .set_value(WATERLOGGED, waterlogged)
            .set_value(SIGNAL_FIRE, Self::is_smoke_source(below_state))
            .set_value(LIT, !waterlogged)
            .set_value(HORIZONTAL_FACING, facing)
    }

    fn projectile_lit_state(
        state: BlockStateId,
        projectile_is_on_fire: bool,
        may_interact: bool,
    ) -> Option<BlockStateId> {
        (projectile_is_on_fire
            && may_interact
            && !state.get_value(LIT)
            && !state.get_value(WATERLOGGED))
        .then(|| state.set_value(LIT, true))
    }
}

impl BlockBehavior for CampfireBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let waterlogged = context.is_water_source();
        let below_state = context.world.get_block_state(context.place_pos().below());
        Some(self.placement_state(waterlogged, below_state, context.horizontal_direction()))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);

        if direction == Direction::Down {
            state.set_value(SIGNAL_FIRE, Self::is_smoke_source(neighbor_state))
        } else {
            state
        }
    }

    fn on_projectile_hit(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        let Some(lit_state) = Self::projectile_lit_state(
            state,
            projectile.is_on_fire(),
            projectile.projectile_may_interact(world, hit.block_pos),
        ) else {
            return;
        };
        world.set_block(hit.block_pos, lit_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
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
        if let Some(damage) = self.contact_damage_amount(state, entity.is_living_entity()) {
            entity.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::CAMPFIRE),
                damage,
            );
        }

        self.default_entity_inside(state, world, pos, entity, effect_collector, is_precise);
    }

    fn place_liquid(
        &self,
        level: &dyn LevelAccessor,
        pos: BlockPos,
        state: BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        if state.try_get_value(WATERLOGGED) != Some(false)
            || fluid_state.fluid_id != &vanilla_fluids::WATER
        {
            return false;
        }

        if state.get_value(LIT) {
            level.play_block_sound(
                &sound_events::ENTITY_GENERIC_EXTINGUISH_FIRE,
                pos,
                1.0,
                1.0,
                None,
            );
            level.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(None, Some(state.set_value(LIT, false))),
            );
        }

        level.set_block_state(
            pos,
            state.set_value(WATERLOGGED, true).set_value(LIT, false),
            UpdateFlags::UPDATE_ALL,
        );
        schedule_placed_liquid_tick(level, pos, fluid_state);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestLevel;
    use steel_registry::{
        blocks::block_state_ext::BlockStateExt, init_vanilla_registry, vanilla_blocks,
    };

    #[test]
    fn lit_campfire_damages_living_entities() {
        init_vanilla_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(LIT, true);

        assert_eq!(campfire.contact_damage_amount(state, true), Some(1.0));
    }

    #[test]
    fn unlit_campfire_does_not_damage_entities() {
        init_vanilla_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(LIT, false);

        assert_eq!(campfire.contact_damage_amount(state, true), None);
    }

    #[test]
    fn campfire_does_not_damage_non_living_entities() {
        init_vanilla_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::SOUL_CAMPFIRE, false, 2);
        let state = vanilla_blocks::SOUL_CAMPFIRE
            .default_state()
            .set_value(LIT, true);

        assert_eq!(campfire.contact_damage_amount(state, false), None);
    }

    #[test]
    fn burning_projectile_lights_only_dry_unlit_campfires() {
        init_vanilla_registry();

        let unlit = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(LIT, false)
            .set_value(WATERLOGGED, false);
        let lit = unlit.set_value(LIT, true);
        let waterlogged = unlit.set_value(WATERLOGGED, true);

        assert_eq!(
            CampfireBlock::projectile_lit_state(unlit, true, true),
            Some(lit)
        );
        assert_eq!(
            CampfireBlock::projectile_lit_state(unlit, false, true),
            None
        );
        assert_eq!(
            CampfireBlock::projectile_lit_state(unlit, true, false),
            None
        );
        assert_eq!(CampfireBlock::projectile_lit_state(lit, true, true), None);
        assert_eq!(
            CampfireBlock::projectile_lit_state(waterlogged, true, true),
            None
        );
    }

    #[test]
    fn placement_state_sets_facing_and_signal_fire() {
        init_vanilla_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);

        let state = campfire.placement_state(
            false,
            vanilla_blocks::HAY_BLOCK.default_state(),
            Direction::East,
        );

        assert_eq!(state.get_value(HORIZONTAL_FACING), Direction::East);
        assert!(state.get_value(SIGNAL_FIRE));
        assert!(state.get_value(LIT));
        assert!(!state.get_value(WATERLOGGED));
    }

    #[test]
    fn update_shape_recomputes_signal_fire_from_below() {
        init_vanilla_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let level = TestLevel::default();
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(SIGNAL_FIRE, false)
            .set_value(WATERLOGGED, false);

        let updated = campfire.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::HAY_BLOCK.default_state(),
        );

        assert!(updated.get_value(SIGNAL_FIRE));
    }

    #[test]
    fn water_placement_extinguishes_lit_campfire() {
        init_vanilla_registry();
        let level = TestLevel::default();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(LIT, true)
            .set_value(WATERLOGGED, false);
        let pos = BlockPos::new(1, 2, 3);

        assert!(campfire.place_liquid(
            &level,
            pos,
            state,
            FluidState::source(&vanilla_fluids::WATER),
        ));

        let placed = level
            .last_placed_state()
            .expect("campfire should be updated");
        assert!(!placed.get_value(LIT));
        assert!(placed.get_value(WATERLOGGED));
        assert_eq!(
            level
                .block_sounds
                .borrow()
                .iter()
                .map(|sound| sound.sound)
                .collect::<Vec<_>>(),
            vec![&sound_events::ENTITY_GENERIC_EXTINGUISH_FIRE]
        );
        assert_eq!(
            level
                .scheduled_fluid_ticks
                .borrow()
                .iter()
                .map(|tick| (tick.pos, tick.fluid, tick.delay))
                .collect::<Vec<_>>(),
            vec![(pos, &vanilla_fluids::WATER, 5)]
        );
        assert_eq!(
            level
                .game_events
                .borrow()
                .iter()
                .map(|event| event.event)
                .collect::<Vec<_>>(),
            vec![&vanilla_game_events::BLOCK_CHANGE]
        );
    }
}
