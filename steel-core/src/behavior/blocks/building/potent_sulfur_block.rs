//! `PotentSulfurBlock` behavior

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, PotentSulfurState};
use steel_registry::sound_events;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_events;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::{BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt as _};
use crate::block_entity::entities::PotentSulfurBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::fluid::FluidStateExt as _;
use crate::world::{LevelReader, ScheduledTickAccess, World, game_event_context::GameEventContext};

/// Vanilla `PotentSulfurBlock` behavior
#[block_behavior]
pub struct PotentSulfurBlock {
    block: BlockRef,
}

impl PotentSulfurBlock {
    /// New potent sulfur block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn valid_state(state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> BlockStateId {
        let above_fluid = world.get_block_state(pos.above()).get_fluid_state();
        if !above_fluid.is_source() || !above_fluid.is_water() {
            return state.set_value(
                &BlockStateProperties::POTENT_SULFUR_STATE,
                PotentSulfurState::Dry,
            );
        }

        let below = world.get_block_state(pos.below());
        let below_fluid = below.get_fluid_state();
        let fluid_ok = below_fluid.is_empty() || below_fluid.is_source();

        if below
            .get_block()
            .has_tag(&BlockTag::CAUSES_CONTINUOUS_GEYSER_ERUPTIONS)
            && fluid_ok
        {
            return state.set_value(
                &BlockStateProperties::POTENT_SULFUR_STATE,
                PotentSulfurState::Continuous,
            );
        }

        if below
            .get_block()
            .has_tag(&BlockTag::CAUSES_PERIODIC_GEYSER_ERUPTIONS)
            && fluid_ok
        {
            let is_geyser = matches!(
                state.get_value(&BlockStateProperties::POTENT_SULFUR_STATE),
                PotentSulfurState::Dormant | PotentSulfurState::Erupting
            );
            if !is_geyser
                && let Some(block_entity) = world.get_block_entity(pos)
                && let Some(potent_sulfur) = block_entity
                    .lock()
                    .as_any_mut()
                    .downcast_mut::<PotentSulfurBlockEntity>()
            {
                potent_sulfur.reset_countdown();
            }

            if state.get_value(&BlockStateProperties::POTENT_SULFUR_STATE)
                == PotentSulfurState::Erupting
            {
                return state;
            }
            return state.set_value(
                &BlockStateProperties::POTENT_SULFUR_STATE,
                PotentSulfurState::Dormant,
            );
        }

        state.set_value(
            &BlockStateProperties::POTENT_SULFUR_STATE,
            PotentSulfurState::Wet,
        )
    }
}

impl BlockBehavior for PotentSulfurBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(Self::valid_state(
            self.block.default_state(),
            context.world,
            context.place_pos,
        ))
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
        Self::valid_state(state, world, pos)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        let current = state.get_value(&BlockStateProperties::POTENT_SULFUR_STATE);
        if !matches!(
            current,
            PotentSulfurState::Erupting | PotentSulfurState::Continuous
        ) {
            return;
        }

        world.block_event(pos, self.block, 0, 0);
        let sound = if current == PotentSulfurState::Continuous {
            &sound_events::BLOCK_POTENT_SULFUR_GEYSER_CONTINUOUS_ERUPTION
        } else {
            &sound_events::BLOCK_POTENT_SULFUR_GEYSER_ERUPTION
        };
        world.play_block_sound(sound, pos, 1.0, 1.0, None);
        world.game_event(
            &vanilla_game_events::BLOCK_ACTIVATE,
            pos,
            &GameEventContext::new(None, Some(state)),
        );
    }

    // TODO: Implement vanilla animateTick once Steel has client-side ambient tick/particle support:
    // sulfur bubbles above non-dry states and occasional noxious gas ambient sound.

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::POTENT_SULFUR,
            level,
            pos,
            state,
        )
    }
}
