//! Turtle egg block behavior impl

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_damage_types, vanilla_entities, vanilla_game_events,
    vanilla_game_rules,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;
use crate::world::game_event_context::GameEventContext;

/// Behavior for turtle eggs
/// TODO: everything else in turtle eggs
#[block_behavior]
pub struct TurtleEggBlock {
    block: BlockRef,
}

impl TurtleEggBlock {
    /// Creates a new turtle egg block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// TODO: vanilla also exempts turtles and bats here, those entity types are
    /// not implemented yet
    fn can_destroy_egg(world: &Arc<World>, entity: &dyn Entity) -> bool {
        if !entity.is_living() {
            return false;
        }

        let is_player = entity.entity_type() == &vanilla_entities::PLAYER;
        let mob_griefing =
            world.get_game_rule(&vanilla_game_rules::MOB_GRIEFING) == GameRuleValue::Bool(true);
        is_player || mob_griefing
    }

    fn destroy_egg(
        world: &Arc<World>,
        state: BlockStateId,
        pos: BlockPos,
        entity: &dyn Entity,
        randomness: u32,
    ) {
        if state.get_block() != &vanilla_blocks::TURTLE_EGG {
            return;
        }
        if !Self::can_destroy_egg(world, entity) {
            return;
        }

        // TODO: vanilla uses the world RNG (Level.getRandom mirror once a
        // world-scoped RNG is available (same gap as FarmlandBlock)
        if rand::random_range(0..randomness) == 0 {
            Self::decrease_eggs(world, pos, state);
        }
    }

    fn decrease_eggs(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        world.play_block_sound(
            sound_events::ENTITY_TURTLE_EGG_BREAK,
            pos,
            0.7,
            0.9 + rand::random::<f32>() * 0.2,
            None,
        );

        let eggs: u8 = state.get_value(&BlockStateProperties::EGGS);
        if eggs <= 1 {
            world.destroy_block(pos, false);
        } else {
            let new_state = state.set_value(&BlockStateProperties::EGGS, eggs - 1);

            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            world.game_event(
                &vanilla_game_events::BLOCK_DESTROY,
                pos,
                // TODO: GameEventContext::new(None, Some(u32::from(new_state.0)) Idk if i need this or if ::default is fine
                &GameEventContext::default(),
            );
            world.destroy_block_effect(pos, u32::from(state.0), None);
        }
    }
}

impl BlockBehavior for TurtleEggBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        fall_distance: f32,
    ) {
        // TODO: vanilla skips egg destruction for zombies (and subclasses);
        // re-add the guard once zombie entity types exist.
        Self::destroy_egg(world, state, pos, entity, 3);

        entity.cause_fall_damage(
            fall_distance,
            1.0,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        );
    }
}
