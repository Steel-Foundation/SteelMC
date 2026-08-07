use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _};
use steel_registry::fluid::FluidState;
use steel_registry::items::item::BlockHitResult;
use steel_registry::vanilla_damage_types;
use steel_registry::{
    REGISTRY, sound_events, vanilla_block_entity_types, vanilla_blocks, vanilla_fluids,
    vanilla_game_events,
};
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, types::UpdateFlags};

use crate::behavior::{InteractionResult, InventoryAccess};
use crate::player::Player;
use crate::{
    behavior::{
        BlockBehavior, BlockEntityCreation, BlockPlaceContext, block::schedule_placed_liquid_tick,
    },
    block_entity::{BlockEntityTicker, entities::CampfireBlockEntity},
    entity::{Entity, InsideBlockEffectCollector, damage::DamageSource, projectile::Projectile},
    world::{
        ClipHitResult, LevelAccessor, ScheduledTickAccess, World, game_event::GameEventContext,
    },
};

/// Behavior for campfires and soul campfires.
#[block_behavior]
pub struct CampfireBlock {
    block: BlockRef,
    #[json_arg(value, json = "spawn_particles")]
    _spawn_particles: bool,
    #[json_arg(value, json = "fire_damage")]
    fire_damage: i32,
}

/// Outcome of attempting to place a held item onto a campfire.
enum CampfirePlaceOutcome {
    /// The item was placed onto a free slot and consumed from the hand.
    Placed,
    /// The item is cookable but all slots are occupied.
    NoSlot,
    /// The held item is not a campfire cooking ingredient.
    NotCookable,
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

    fn projectile_lit_state(
        state: BlockStateId,
        projectile_is_on_fire: bool,
        may_interact: bool,
    ) -> Option<BlockStateId> {
        (projectile_is_on_fire
            && may_interact
            && !state.get_value(&BlockStateProperties::LIT)
            && !state.get_value(&BlockStateProperties::WATERLOGGED))
        .then(|| state.set_value(&BlockStateProperties::LIT, true))
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

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::TryEmptyHandInteraction;
        };
        let Some(campfire) = block_entity.downcast_ref::<CampfireBlockEntity>() else {
            return InteractionResult::TryEmptyHandInteraction;
        };

        // Look up the campfire recipe for the held item and, if cookable, place
        // a single item onto the campfire and consume it from the hand.
        let outcome = inv.with_item(|stack| {
            let Some(recipe) = REGISTRY.recipes.find_campfire_recipe(stack) else {
                return CampfirePlaceOutcome::NotCookable;
            };
            if campfire.place_food(stack.clone(), recipe.cooking_time) {
                stack.shrink(1);
                CampfirePlaceOutcome::Placed
            } else {
                CampfirePlaceOutcome::NoSlot
            }
        });

        match outcome {
            CampfirePlaceOutcome::Placed => {
                world.send_block_updated(pos);
                let context = GameEventContext::new(Some(player), Some(state));
                world.game_event(&vanilla_game_events::BLOCK_CHANGE, pos, &context);
                // The INTERACT_WITH_CAMPFIRE stat awaits Steel's statistics foundation.
                InteractionResult::SuccessServer
            }
            CampfirePlaceOutcome::NoSlot => InteractionResult::Consume,
            CampfirePlaceOutcome::NotCookable => InteractionResult::TryEmptyHandInteraction,
        }
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::Created(Arc::new(CampfireBlockEntity::new(level, pos, state)))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::CAMPFIRE,
        )
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
    fn burning_projectile_lights_only_dry_unlit_campfires() {
        init_test_registry();

        let unlit = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, false)
            .set_value(&BlockStateProperties::WATERLOGGED, false);
        let lit = unlit.set_value(&BlockStateProperties::LIT, true);
        let waterlogged = unlit.set_value(&BlockStateProperties::WATERLOGGED, true);

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
