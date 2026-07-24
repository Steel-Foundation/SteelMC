//! Vanilla note-block behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, NoteBlockInstrument};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{sound_events, vanilla_game_events};
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{
    BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, SignalGetter as _, World};

const NOTE_VOLUME: f32 = 3.0;

/// Vanilla `NoteBlock`, including tuning, redstone playback, and instrument selection.
#[block_behavior]
pub struct NoteBlock {
    block: BlockRef,
}

impl NoteBlock {
    /// Creates note-block behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn block_instrument(state: BlockStateId) -> NoteBlockInstrument {
        state.get_block().config.instrument
    }

    fn set_instrument(level: &dyn LevelReader, pos: BlockPos, state: BlockStateId) -> BlockStateId {
        let instrument_above = Self::block_instrument(level.get_block_state(pos.above()));
        if instrument_above.works_above_note_block() {
            return state.set_value(
                &BlockStateProperties::NOTEBLOCK_INSTRUMENT,
                instrument_above,
            );
        }

        let instrument_below = Self::block_instrument(level.get_block_state(pos.below()));
        let instrument = if instrument_below.works_above_note_block() {
            NoteBlockInstrument::Harp
        } else {
            instrument_below
        };
        state.set_value(&BlockStateProperties::NOTEBLOCK_INSTRUMENT, instrument)
    }

    fn cycle_note(state: BlockStateId) -> BlockStateId {
        let note = state.get_value(&BlockStateProperties::NOTE);
        let next = if note == BlockStateProperties::NOTE.max {
            BlockStateProperties::NOTE.min
        } else {
            note + 1
        };
        state.set_value(&BlockStateProperties::NOTE, next)
    }

    fn play_note(
        &self,
        source: Option<&dyn Entity>,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) {
        let instrument = state.get_value(&BlockStateProperties::NOTEBLOCK_INSTRUMENT);
        if !instrument.works_above_note_block() && !world.get_block_state(pos.above()).is_air() {
            return;
        }

        world.block_event(pos, self.block, 0, 0);
        world.game_event(
            &vanilla_game_events::NOTE_BLOCK_PLAY,
            pos,
            &GameEventContext::new(source, None),
        );
    }

    /// Vanilla `NoteBlock.getPitchFromNote`.
    #[must_use]
    pub fn pitch_from_note(note: u8) -> f32 {
        2.0_f64.powf((f64::from(note) - 12.0) / 12.0) as f32
    }

    fn sound_event(instrument: NoteBlockInstrument) -> Option<SoundEventRef> {
        Some(match instrument {
            NoteBlockInstrument::Harp => &sound_events::BLOCK_NOTE_BLOCK_HARP,
            NoteBlockInstrument::Basedrum => &sound_events::BLOCK_NOTE_BLOCK_BASEDRUM,
            NoteBlockInstrument::Snare => &sound_events::BLOCK_NOTE_BLOCK_SNARE,
            NoteBlockInstrument::Hat => &sound_events::BLOCK_NOTE_BLOCK_HAT,
            NoteBlockInstrument::Bass => &sound_events::BLOCK_NOTE_BLOCK_BASS,
            NoteBlockInstrument::Flute => &sound_events::BLOCK_NOTE_BLOCK_FLUTE,
            NoteBlockInstrument::Bell => &sound_events::BLOCK_NOTE_BLOCK_BELL,
            NoteBlockInstrument::Guitar => &sound_events::BLOCK_NOTE_BLOCK_GUITAR,
            NoteBlockInstrument::Chime => &sound_events::BLOCK_NOTE_BLOCK_CHIME,
            NoteBlockInstrument::Xylophone => &sound_events::BLOCK_NOTE_BLOCK_XYLOPHONE,
            NoteBlockInstrument::IronXylophone => &sound_events::BLOCK_NOTE_BLOCK_IRON_XYLOPHONE,
            NoteBlockInstrument::CowBell => &sound_events::BLOCK_NOTE_BLOCK_COW_BELL,
            NoteBlockInstrument::Didgeridoo => &sound_events::BLOCK_NOTE_BLOCK_DIDGERIDOO,
            NoteBlockInstrument::Bit => &sound_events::BLOCK_NOTE_BLOCK_BIT,
            NoteBlockInstrument::Banjo => &sound_events::BLOCK_NOTE_BLOCK_BANJO,
            NoteBlockInstrument::Pling => &sound_events::BLOCK_NOTE_BLOCK_PLING,
            NoteBlockInstrument::Trumpet => &sound_events::BLOCK_NOTE_BLOCK_TRUMPET,
            NoteBlockInstrument::TrumpetExposed => &sound_events::BLOCK_NOTE_BLOCK_TRUMPET_EXPOSED,
            NoteBlockInstrument::TrumpetOxidized => {
                &sound_events::BLOCK_NOTE_BLOCK_TRUMPET_OXIDIZED
            }
            NoteBlockInstrument::TrumpetWeathered => {
                &sound_events::BLOCK_NOTE_BLOCK_TRUMPET_WEATHERED
            }
            NoteBlockInstrument::Zombie => &sound_events::BLOCK_NOTE_BLOCK_IMITATE_ZOMBIE,
            NoteBlockInstrument::Skeleton => &sound_events::BLOCK_NOTE_BLOCK_IMITATE_SKELETON,
            NoteBlockInstrument::Creeper => &sound_events::BLOCK_NOTE_BLOCK_IMITATE_CREEPER,
            NoteBlockInstrument::Dragon => &sound_events::BLOCK_NOTE_BLOCK_IMITATE_ENDER_DRAGON,
            NoteBlockInstrument::WitherSkeleton => {
                &sound_events::BLOCK_NOTE_BLOCK_IMITATE_WITHER_SKELETON
            }
            NoteBlockInstrument::Piglin => &sound_events::BLOCK_NOTE_BLOCK_IMITATE_PIGLIN,
            NoteBlockInstrument::CustomHead => return None,
        })
    }
}

impl BlockBehavior for NoteBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(Self::set_instrument(
            context.world,
            context.place_pos(),
            self.block.default_state(),
        ))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction.axis().is_vertical() {
            Self::set_instrument(world, pos, state)
        } else {
            state
        }
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        let signal = world.has_neighbor_signal(pos);
        if signal == state.get_value(&BlockStateProperties::POWERED) {
            return;
        }

        if signal {
            self.play_note(None, state, world, pos);
        }
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::POWERED, signal),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn use_item_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _player: &Player,
        _hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if hit_result.direction == Direction::Up
            && inv.with_item(|item| item.item().has_tag(&ItemTag::NOTEBLOCK_TOP_INSTRUMENTS))
        {
            InteractionResult::Pass
        } else {
            InteractionResult::TryEmptyHandInteraction
        }
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let tuned_state = Self::cycle_note(state);
        world.set_block(pos, tuned_state, UpdateFlags::UPDATE_ALL);
        self.play_note(Some(player), tuned_state, world, pos);
        // The tune-noteblock stat awaits Steel's shared statistics foundation.
        InteractionResult::Success
    }

    fn attack(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, player: &Player) {
        self.play_note(Some(player), state, world, pos);
        // The play-noteblock stat awaits Steel's shared statistics foundation.
    }

    fn trigger_event(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _param_a: i32,
        _param_b: i32,
    ) -> bool {
        let instrument = state.get_value(&BlockStateProperties::NOTEBLOCK_INSTRUMENT);
        let Some(sound) = Self::sound_event(instrument) else {
            // Custom player-head sounds require the skull block entity's note-block sound.
            return false;
        };
        let pitch = if instrument.is_tunable() {
            Self::pitch_from_note(state.get_value(&BlockStateProperties::NOTE))
        } else {
            1.0
        };

        world.play_sound(sound, SoundSource::Records, pos, NOTE_VOLUME, pitch, None);
        // The block-event packet makes the client create Vanilla's local note particle.
        true
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn vertical_blocks_select_instruments_with_vanilla_priority() {
        init_test_registry();
        let pos = BlockPos::new(2, 64, 3);
        let note_state = vanilla_blocks::NOTE_BLOCK.default_state();
        let level = TestLevel::default()
            .with_block(pos.above(), vanilla_blocks::ZOMBIE_HEAD.default_state())
            .with_block(pos.below(), vanilla_blocks::CLAY.default_state());

        let selected = NoteBlock::set_instrument(&level, pos, note_state);
        assert_eq!(
            selected.get_value(&BlockStateProperties::NOTEBLOCK_INSTRUMENT),
            NoteBlockInstrument::Zombie
        );

        let below_head = TestLevel::default()
            .with_block(pos.below(), vanilla_blocks::ZOMBIE_HEAD.default_state());
        let selected = NoteBlock::set_instrument(&below_head, pos, note_state);
        assert_eq!(
            selected.get_value(&BlockStateProperties::NOTEBLOCK_INSTRUMENT),
            NoteBlockInstrument::Harp
        );
    }

    #[test]
    fn tuning_wraps_after_the_top_note() {
        init_test_registry();
        let highest = vanilla_blocks::NOTE_BLOCK
            .default_state()
            .set_value(&BlockStateProperties::NOTE, BlockStateProperties::NOTE.max);

        assert_eq!(
            NoteBlock::cycle_note(highest).get_value(&BlockStateProperties::NOTE),
            BlockStateProperties::NOTE.min
        );
    }

    #[test]
    fn redstone_updates_powered_state_on_both_edges() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("note_block_redstone_edges");
        let pos = BlockPos::new(8, 64, 8);
        let power_pos = pos.west();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::NOTE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::POWERED)
        );
        world.run_block_events();

        assert!(world.remove_block(power_pos, false));
        assert!(
            !world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::POWERED)
        );
    }
}
