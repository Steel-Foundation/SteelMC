//! Vanilla jukebox block-entity storage and song playback.

use std::io::Cursor;
use std::mem;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::{
    BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView,
    NbtTag as BorrowedNbtTag, read_compound as read_borrowed_compound,
};
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::vanilla_components::JUKEBOX_PLAYABLE;
use steel_registry::item_stack::ItemStack;
use steel_registry::jukebox_song::JukeboxSongValue;
use steel_registry::particle_type::ParticleData;
use steel_registry::{
    RegistryEntry, level_events, vanilla_block_entity_types, vanilla_game_events,
    vanilla_particle_types,
};
use steel_utils::nbt::{merge_nbt_compounds, nbt_compounds_equal};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;
use crate::world::game_event::GameEventContext;

const RECORD_ITEM_TAG: &str = "RecordItem";
const TICKS_SINCE_SONG_STARTED_TAG: &str = "ticks_since_song_started";
const TICKS_PER_SECOND: f32 = 20.0;
const PLAY_EVENT_INTERVAL_TICKS: i64 = 20;
const SONG_END_PADDING_TICKS: i32 = 20;
const UNKNOWN_SONG_REGISTRY_ID: i32 = -1;
const BLOCK_CENTER_OFFSET: f64 = 0.5;
const ITEM_EJECTION_Y_OFFSET: f64 = 1.01;
const ITEM_EJECTION_RANDOM_CENTER: f32 = 0.5;
const ITEM_EJECTION_HORIZONTAL_SPREAD: f32 = 0.7;
const NOTE_PARTICLE_Y_OFFSET: f32 = 1.2;
const NOTE_PARTICLE_COLOR_VARIANTS: u8 = 4;
const NOTE_PARTICLE_COLOR_DIVISOR: f32 = 24.0;

struct JukeboxPlayback {
    ticks_since_song_started: i64,
}

struct JukeboxState {
    item: ItemStack,
    playback: Option<JukeboxPlayback>,
}

/// Vanilla `JukeboxBlockEntity`.
pub struct JukeboxBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<JukeboxState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `JukeboxBlockEntity`.
unsafe impl DowncastType for JukeboxBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/jukebox");
}

impl JukeboxBlockEntity {
    /// Creates an empty jukebox block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::JUKEBOX, level, pos, state),
            state: SyncMutex::new(JukeboxState {
                item: ItemStack::empty(),
                playback: None,
            }),
        }
    }

    fn song_value(item: &ItemStack) -> Option<&JukeboxSongValue> {
        Some(item.get(JUKEBOX_PLAYABLE)?.song().value())
    }

    fn song_event_data(item: &ItemStack) -> i32 {
        let Some(playable) = item.get(JUKEBOX_PLAYABLE) else {
            return UNKNOWN_SONG_REGISTRY_ID;
        };
        let song = playable.song();
        song.as_reference()
            .and_then(RegistryEntry::try_id)
            .and_then(|id| i32::try_from(id).ok())
            .unwrap_or(UNKNOWN_SONG_REGISTRY_ID)
    }

    fn song_has_finished(song: &JukeboxSongValue, ticks_elapsed: i64) -> bool {
        // Vanilla multiplies as `float`, applies `Mth.ceil(float)`, and performs
        // the padding addition as a wrapping Java `int` before widening.
        let length_in_ticks = (song.length_in_seconds * TICKS_PER_SECOND).ceil() as i32;
        ticks_elapsed >= i64::from(length_in_ticks.wrapping_add(SONG_END_PADDING_TICKS))
    }

    fn value_input_long(tag: BorrowedNbtTag<'_, '_>) -> Option<i64> {
        tag.byte()
            .map(i64::from)
            .or_else(|| tag.short().map(i64::from))
            .or_else(|| tag.int().map(i64::from))
            .or_else(|| tag.long())
            .or_else(|| tag.float().map(|value| value as i64))
            // Vanilla's DoubleTag.longValue floors before narrowing.
            .or_else(|| tag.double().map(|value| value.floor() as i64))
    }

    fn on_song_changed(&self, world: &Arc<World>) {
        world.update_neighbors_at(self.get_block_pos(), self.get_block_state().get_block());
        self.set_changed();
    }

    fn emit_stop_pair(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        world.game_event(
            &vanilla_game_events::JUKEBOX_STOP_PLAY,
            pos,
            &GameEventContext::new(None, Some(self.get_block_state())),
        );
        world.level_event(level_events::SOUND_STOP_JUKEBOX_SONG, pos, 0, None);
    }

    fn stop_playback(&self, world: Option<&Arc<World>>) {
        let stopped = self.state.lock().playback.take().is_some();
        if !stopped {
            return;
        }
        let Some(world) = world else {
            return;
        };
        self.emit_stop_pair(world);
        self.on_song_changed(world);
    }

    fn notify_item_changed(&self, world: &Arc<World>, has_record: bool) {
        let pos = self.get_block_pos();
        let cached_state = self.get_block_state();
        if world.get_block_state(pos) != cached_state {
            return;
        }

        world.set_block(
            pos,
            cached_state.set_value(&BlockStateProperties::HAS_RECORD, has_record),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(None, Some(self.get_block_state())),
        );
    }

    /// Replaces the stored item and starts or stops its jukebox song.
    pub fn set_the_item(&self, item: ItemStack) {
        let has_record = !item.is_empty();
        let has_song = Self::song_value(&item).is_some();
        let song_event_data = Self::song_event_data(&item);
        let world = self.get_level();
        self.state.lock().item = item;
        let Some(world) = world else {
            return;
        };

        self.notify_item_changed(&world, has_record);
        if has_record && has_song {
            self.state.lock().playback = Some(JukeboxPlayback {
                ticks_since_song_started: 0,
            });
            world.level_event(
                level_events::SOUND_PLAY_JUKEBOX_SONG,
                self.get_block_pos(),
                song_event_data,
                None,
            );
            self.on_song_changed(&world);
        } else {
            self.stop_playback(Some(&world));
        }
    }

    /// Ejects the stored item with Vanilla's position, velocity, and pickup delay.
    pub fn pop_out_the_item(&self) {
        let Some(world) = self.get_level() else {
            return;
        };
        let item = {
            let mut state = self.state.lock();
            if state.item.is_empty() {
                return;
            }
            mem::replace(&mut state.item, ItemStack::empty())
        };

        self.notify_item_changed(&world, false);
        self.stop_playback(Some(&world));

        let pos = self.get_block_pos();
        let random_x =
            (rand::random::<f32>() - ITEM_EJECTION_RANDOM_CENTER) * ITEM_EJECTION_HORIZONTAL_SPREAD;
        let random_z =
            (rand::random::<f32>() - ITEM_EJECTION_RANDOM_CENTER) * ITEM_EJECTION_HORIZONTAL_SPREAD;
        let item_pos = DVec3::new(
            f64::from(pos.x()) + BLOCK_CENTER_OFFSET + f64::from(random_x),
            f64::from(pos.y()) + ITEM_EJECTION_Y_OFFSET,
            f64::from(pos.z()) + BLOCK_CENTER_OFFSET + f64::from(random_z),
        );
        if let Some(entity) = world.spawn_item(item_pos, item) {
            entity.set_default_pickup_delay();
        }
        // Vanilla notifies a second time after attempting to add the entity.
        self.on_song_changed(&world);
    }

    /// Returns whether a song is currently active.
    #[must_use]
    pub fn is_record_playing(&self) -> bool {
        self.state.lock().playback.is_some()
    }

    /// Returns the stored song's extracted comparator output.
    #[must_use]
    pub fn analog_output_signal(&self) -> i32 {
        let state = self.state.lock();
        Self::song_value(&state.item).map_or(0, |song| song.comparator_output)
    }

    /// Merges an owned `BLOCK_ENTITY_DATA` payload into this jukebox.
    ///
    /// The placing block validates the payload's declared block-entity type
    /// before calling this method and releases the source inventory lock first.
    pub fn apply_item_block_entity_data(&self, payload: NbtCompound) -> bool {
        let before = self.save_custom_only();
        let mut merged = self.save_custom_only();
        merge_nbt_compounds(&mut merged, &payload);
        if nbt_compounds_equal(&before, &merged) {
            return false;
        }

        let mut bytes = Vec::new();
        merged.write(&mut bytes);
        let Ok(borrowed) = read_borrowed_compound(&mut Cursor::new(bytes.as_slice())) else {
            log::warn!(
                "failed to reborrow item block-entity data for jukebox at {:?}",
                self.get_block_pos()
            );
            return false;
        };
        self.load_additional(&borrowed);
        self.set_changed();
        true
    }
}

enum TickAction {
    Stop,
    EmitPlayingEvent,
    None,
}

impl BlockEntity for JukeboxBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let new_item = nbt
            .compound(RECORD_ITEM_TAG)
            .and_then(|item| ItemStack::from_borrowed_compound(&item))
            .unwrap_or_else(ItemStack::empty);
        let saved_ticks = nbt
            .get(TICKS_SINCE_SONG_STARTED_TAG)
            .and_then(Self::value_input_long);

        let should_stop = {
            let state = self.state.lock();
            !state.item.is_empty()
                && !ItemStack::is_same_item_same_components(&new_item, &state.item)
        };
        if should_stop {
            let world = self.get_level();
            self.stop_playback(world.as_ref());
        }

        let should_resume = saved_ticks.is_some_and(|ticks| {
            Self::song_value(&new_item).is_some_and(|song| !Self::song_has_finished(song, ticks))
        });
        let mut state = self.state.lock();
        state.item = new_item;
        if should_resume {
            state.playback = saved_ticks.map(|ticks| JukeboxPlayback {
                ticks_since_song_started: ticks,
            });
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        if !state.item.is_empty() {
            nbt.insert(RECORD_ITEM_TAG, state.item.to_nbt_tag_ref());
        }
        if let Some(playback) = &state.playback {
            nbt.insert(
                TICKS_SINCE_SONG_STARTED_TAG,
                playback.ticks_since_song_started,
            );
        }
    }

    fn tick(&self, world: &Arc<World>) {
        let action = {
            let mut state = self.state.lock();
            let Some(ticks) = state
                .playback
                .as_ref()
                .map(|playback| playback.ticks_since_song_started)
            else {
                return;
            };

            if Self::song_value(&state.item).is_none_or(|song| Self::song_has_finished(song, ticks))
            {
                state.playback = None;
                TickAction::Stop
            } else if ticks % PLAY_EVENT_INTERVAL_TICKS == 0 {
                TickAction::EmitPlayingEvent
            } else {
                TickAction::None
            }
        };

        match action {
            TickAction::Stop => {
                self.emit_stop_pair(world);
                self.on_song_changed(world);
                return;
            }
            TickAction::EmitPlayingEvent => {
                let pos = self.get_block_pos();
                world.game_event(
                    &vanilla_game_events::JUKEBOX_PLAY,
                    pos,
                    &GameEventContext::new(None, Some(self.get_block_state())),
                );
                let random_color = f64::from(
                    f32::from(rand::random_range(0..NOTE_PARTICLE_COLOR_VARIANTS))
                        / NOTE_PARTICLE_COLOR_DIVISOR,
                );
                world.send_particles(
                    ParticleData::simple(&vanilla_particle_types::NOTE),
                    DVec3::new(
                        f64::from(pos.x()) + BLOCK_CENTER_OFFSET,
                        f64::from(pos.y()) + f64::from(NOTE_PARTICLE_Y_OFFSET),
                        f64::from(pos.z()) + BLOCK_CENTER_OFFSET,
                    ),
                    0,
                    DVec3::new(random_color, 0.0, 0.0),
                    1.0,
                );
            }
            TickAction::None => {}
        }

        if let Some(playback) = &mut self.state.lock().playback {
            playback.ticks_since_song_started = playback.ticks_since_song_started.wrapping_add(1);
        }
    }

    fn pre_remove_side_effects(&self, _pos: BlockPos, _state: BlockStateId) {
        self.pop_out_the_item();
    }

    fn on_set_removed(&self) {
        if let Some(world) = self.get_level() {
            // Vanilla emits this pair on every removal callback, even when
            // normal ejection already stopped and cleared the song.
            self.emit_stop_pair(&world);
        }
    }
}

#[cfg(test)]
mod tests {
    use simdnbt::borrow::read_compound;
    use steel_registry::data_components::components::JukeboxPlayable;
    use steel_registry::jukebox_song::JukeboxSongValue;
    use steel_registry::sound_event::SoundEventHolder;
    use steel_registry::{
        init_vanilla_registry, vanilla_blocks, vanilla_items, vanilla_jukebox_songs,
    };
    use steel_utils::Identifier;
    use text_components::TextComponent;

    use super::*;

    const SAVED_PLAYBACK_TICKS: i64 = 37;

    fn jukebox() -> JukeboxBlockEntity {
        init_vanilla_registry();
        JukeboxBlockEntity::new(
            Weak::new(),
            BlockPos::new(3, 70, -4),
            vanilla_blocks::JUKEBOX.default_state(),
        )
    }

    fn record_payload(item: &ItemStack, ticks: Option<i64>) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert(RECORD_ITEM_TAG, item.to_nbt_tag_ref());
        if let Some(ticks) = ticks {
            nbt.insert(TICKS_SINCE_SONG_STARTED_TAG, ticks);
        }
        nbt
    }

    fn load_owned(jukebox: &JukeboxBlockEntity, nbt: &NbtCompound) {
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test jukebox NBT should reborrow");
        jukebox.load_additional(&borrowed);
    }

    #[test]
    fn saved_song_resumes_before_but_not_at_its_vanilla_finish_tick() {
        let record = ItemStack::new(&vanilla_items::MUSIC_DISC_CAT);
        let song = JukeboxBlockEntity::song_value(&record)
            .expect("vanilla music disc should carry a jukebox song");
        let length_in_ticks = (song.length_in_seconds * TICKS_PER_SECOND).ceil() as i32;
        let finish_tick = i64::from(length_in_ticks.wrapping_add(SONG_END_PADDING_TICKS));

        let before_finish = jukebox();
        load_owned(
            &before_finish,
            &record_payload(&record, Some(finish_tick - 1)),
        );
        assert!(before_finish.is_record_playing());
        let mut saved = NbtCompound::new();
        before_finish.save_additional(&mut saved);
        assert_eq!(
            saved.long(TICKS_SINCE_SONG_STARTED_TAG),
            Some(finish_tick - 1)
        );

        let at_finish = jukebox();
        load_owned(&at_finish, &record_payload(&record, Some(finish_tick)));
        assert!(!at_finish.is_record_playing());
        assert_eq!(at_finish.analog_output_signal(), song.comparator_output);
    }

    #[test]
    fn item_block_entity_data_merges_with_existing_record_before_loading() {
        let record = ItemStack::new(&vanilla_items::MUSIC_DISC_PIGSTEP);
        let jukebox = jukebox();
        load_owned(&jukebox, &record_payload(&record, None));
        assert!(!jukebox.is_record_playing());

        let mut ticks_only = NbtCompound::new();
        ticks_only.insert(TICKS_SINCE_SONG_STARTED_TAG, SAVED_PLAYBACK_TICKS);
        assert!(jukebox.apply_item_block_entity_data(ticks_only));
        assert!(jukebox.is_record_playing());
        assert_eq!(
            jukebox.analog_output_signal(),
            JukeboxBlockEntity::song_value(&record)
                .expect("vanilla music disc should carry a jukebox song")
                .comparator_output
        );

        let mut saved = NbtCompound::new();
        jukebox.save_additional(&mut saved);
        assert!(saved.compound(RECORD_ITEM_TAG).is_some());
        assert_eq!(
            saved.long(TICKS_SINCE_SONG_STARTED_TAG),
            Some(SAVED_PLAYBACK_TICKS)
        );

        let mut unchanged = NbtCompound::new();
        unchanged.insert(TICKS_SINCE_SONG_STARTED_TAG, SAVED_PLAYBACK_TICKS);
        assert!(!jukebox.apply_item_block_entity_data(unchanged));
    }

    #[test]
    fn direct_song_uses_vanilla_unknown_registry_level_event_data() {
        init_vanilla_registry();
        let mut direct = ItemStack::new(&vanilla_items::STONE);
        direct.set(
            JUKEBOX_PLAYABLE,
            JukeboxPlayable::direct(JukeboxSongValue {
                sound_event: SoundEventHolder::Direct {
                    sound_id: Identifier::vanilla_static("jukebox_test_direct"),
                    fixed_range: None,
                },
                description: TextComponent::plain("Direct test song"),
                length_in_seconds: 1.0,
                comparator_output: 1,
            }),
        );

        assert!(JukeboxBlockEntity::song_value(&direct).is_some());
        assert_eq!(
            JukeboxBlockEntity::song_event_data(&direct),
            UNKNOWN_SONG_REGISTRY_ID
        );

        let reference = ItemStack::new(&vanilla_items::MUSIC_DISC_CAT);
        assert_eq!(
            JukeboxBlockEntity::song_event_data(&reference),
            vanilla_jukebox_songs::CAT
                .try_id()
                .and_then(|id| i32::try_from(id).ok())
                .unwrap_or(UNKNOWN_SONG_REGISTRY_ID)
        );
    }

    #[test]
    fn value_input_long_uses_each_numeric_tag_long_value() {
        let mut nbt = NbtCompound::new();
        nbt.insert("double", -0.5_f64);
        nbt.insert("float", -0.5_f32);
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("numeric test NBT should reborrow");
        let view: NbtCompoundView<'_, '_> = (&borrowed).into();

        assert_eq!(
            view.get("double")
                .and_then(JukeboxBlockEntity::value_input_long),
            Some(-1)
        );
        assert_eq!(
            view.get("float")
                .and_then(JukeboxBlockEntity::value_input_long),
            Some(0)
        );
    }
}
