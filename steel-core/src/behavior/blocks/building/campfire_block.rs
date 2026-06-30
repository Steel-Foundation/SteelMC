use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _};
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_damage_types;
use steel_registry::{sound_events, vanilla_blocks, vanilla_fluids, vanilla_game_events};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, block::schedule_placed_liquid_tick},
    entity::{Entity, InsideBlockEffectCollector, damage::DamageSource},
    world::{LevelAccessor, ScheduledTickAccess, World, game_event_context::GameEventContext},
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
        if state.get_value(&BlockStateProperties::LIT) && is_living_entity {
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
            .set_value(&BlockStateProperties::WATERLOGGED, waterlogged)
            .set_value(
                &BlockStateProperties::SIGNAL_FIRE,
                Self::is_smoke_source(below_state),
            )
            .set_value(&BlockStateProperties::LIT, !waterlogged)
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
    }
}

impl BlockBehavior for CampfireBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let waterlogged = context.is_water_source();
        let below_state = context.world.get_block_state(context.relative_pos.below());
        Some(self.placement_state(waterlogged, below_state, context.horizontal_direction))
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
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        if direction == Direction::Down {
            state.set_value(
                &BlockStateProperties::SIGNAL_FIRE,
                Self::is_smoke_source(neighbor_state),
            )
        } else {
            state
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
        if let Some(damage) = self.contact_damage_amount(state, entity.is_living_entity()) {
            entity.hurt(
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
        if state.try_get_value(&BlockStateProperties::WATERLOGGED) != Some(false)
            || fluid_state.fluid_id != &vanilla_fluids::WATER
        {
            return false;
        }

        if state.get_value(&BlockStateProperties::LIT) {
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
                &GameEventContext::new(
                    None,
                    Some(state.set_value(&BlockStateProperties::LIT, false)),
                ),
            );
        }

        level.set_block_state(
            pos,
            state
                .set_value(&BlockStateProperties::WATERLOGGED, true)
                .set_value(&BlockStateProperties::LIT, false),
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
        blocks::block_state_ext::BlockStateExt, test_support::init_test_registry, vanilla_blocks,
    };

    #[test]
    fn lit_campfire_damages_living_entities() {
        init_test_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, true);

        assert_eq!(campfire.contact_damage_amount(state, true), Some(1.0));
    }

    #[test]
    fn unlit_campfire_does_not_damage_entities() {
        init_test_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, false);

        assert_eq!(campfire.contact_damage_amount(state, true), None);
    }

    #[test]
    fn campfire_does_not_damage_non_living_entities() {
        init_test_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::SOUL_CAMPFIRE, false, 2);
        let state = vanilla_blocks::SOUL_CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, true);

        assert_eq!(campfire.contact_damage_amount(state, false), None);
    }

    #[test]
    fn placement_state_sets_facing_and_signal_fire() {
        init_test_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);

        let state = campfire.placement_state(
            false,
            vanilla_blocks::HAY_BLOCK.default_state(),
            Direction::East,
        );

        assert_eq!(
            state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
            Direction::East
        );
        assert!(state.get_value(&BlockStateProperties::SIGNAL_FIRE));
        assert!(state.get_value(&BlockStateProperties::LIT));
        assert!(!state.get_value(&BlockStateProperties::WATERLOGGED));
    }

    #[test]
    fn update_shape_recomputes_signal_fire_from_below() {
        init_test_registry();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let level = TestLevel::default();
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::SIGNAL_FIRE, false)
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        let updated = campfire.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::HAY_BLOCK.default_state(),
        );

        assert!(updated.get_value(&BlockStateProperties::SIGNAL_FIRE));
    }

    #[test]
    fn water_placement_extinguishes_lit_campfire() {
        init_test_registry();
        let level = TestLevel::default();
        let campfire = CampfireBlock::new(&vanilla_blocks::CAMPFIRE, true, 1);
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, true)
            .set_value(&BlockStateProperties::WATERLOGGED, false);
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
        assert!(!placed.get_value(&BlockStateProperties::LIT));
        assert!(placed.get_value(&BlockStateProperties::WATERLOGGED));
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
