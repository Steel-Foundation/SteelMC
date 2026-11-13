//! Entity sounds: steps, swimming, splashes and flapping.

use super::*;

/// Entity sounds: steps, swimming, splashes and flapping.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityProps`], never by implementing this trait.
pub trait EntitySounds: Entity {
    /// Default for [`EntityHooks::fall_sounds`].
    fn fall_sounds_default(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_GENERIC_SMALL_FALL,
            &sound_events::ENTITY_GENERIC_BIG_FALL,
        )
    }

    /// Default for [`EntityHooks::sound_source`].
    fn sound_source_default(&self) -> SoundSource {
        SoundSource::Neutral
    }

    /// Default for [`EntityHooks::swim_sound`].
    fn swim_sound_default(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SWIM
    }

    /// Returns whether sounds from this entity are suppressed.
    fn is_silent(&self) -> bool {
        self.base().silent()
    }

    /// Sets whether sounds from this entity are suppressed.
    fn set_silent(&self, silent: bool) {
        self.base().set_silent(silent);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_silent(silent);
        }
    }

    /// Default for [`EntityHooks::play_sound`].
    fn play_sound_default(&self, sound: SoundEventRef, volume: f32, pitch: f32) {
        if self.is_silent() {
            return;
        }

        if let Some(world) = self.level() {
            world.play_sound_at(
                sound,
                self.sound_source(),
                self.position(),
                volume,
                pitch,
                None,
            );
        }
    }

    /// Default for [`EntityHooks::swim_splash_sound`].
    fn swim_splash_sound_default(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SPLASH
    }

    /// Default for [`EntityHooks::swim_high_speed_splash_sound`].
    fn swim_high_speed_splash_sound_default(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SPLASH
    }

    /// Default for [`EntityHooks::play_step_sound`].
    fn play_step_sound_default(&self, _pos: BlockPos, block_state: BlockStateId) {
        self.play_block_step_sound(block_state);
    }

    /// Plays a vanilla block step sound at this entity's current position.
    fn play_block_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.15,
            sound_type.pitch,
        );
    }

    /// Plays vanilla's muffled secondary step sound.
    fn play_muffled_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.05,
            sound_type.pitch * 0.8,
        );
    }

    /// Plays vanilla's combination primary and secondary step sounds.
    fn play_combination_step_sounds(
        &self,
        primary_step_sound: BlockStateId,
        secondary_step_sound: BlockStateId,
    ) {
        self.play_block_step_sound(primary_step_sound);
        self.play_muffled_step_sound(secondary_step_sound);
    }

    /// Plays vanilla walking step sounds, including amethyst chimes.
    fn walking_step_sound(&self, pos: BlockPos, block_state: BlockStateId) {
        self.play_step_sound(pos, block_state);
        if block_state
            .get_block()
            .has_tag(&BlockTag::CRYSTAL_SOUND_BLOCKS)
        {
            self.play_amethyst_step_sound();
        }
    }

    /// Plays vanilla amethyst step chime when its cooldown permits it.
    fn play_amethyst_step_sound(&self) {
        let Some(sound) = self.base().amethyst_step_sound(self.tick_count()) else {
            return;
        };
        self.play_sound(
            &sound_events::BLOCK_AMETHYST_BLOCK_CHIME,
            sound.volume,
            sound.pitch,
        );
    }

    /// Plays vanilla swim sound from movement emission.
    fn water_swim_sound(&self) {
        let velocity = self.velocity();
        let volume = ((velocity.x * velocity.x * 0.2
            + velocity.y * velocity.y
            + velocity.z * velocity.z * 0.2)
            .sqrt() as f32
            * 0.35)
            .min(1.0);
        self.play_swim_sound(volume);
    }

    /// Plays this entity's swim sound at the given volume.
    fn play_swim_sound(&self, volume: f32) {
        let pitch = 1.0 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
        self.play_sound(self.swim_sound(), volume, pitch);
    }

    /// Default for [`EntityHooks::is_flapping`].
    fn is_flapping_default(&self) -> bool {
        false
    }

    /// Default for [`EntityHooks::on_flap`].
    fn on_flap_default(&self) {}

    /// Processes vanilla flap movement side effects.
    fn process_flapping_movement(&self) {
        if !self.is_flapping() {
            return;
        }

        self.on_flap();
        if self.movement_emission().emits_events() {
            self.game_event(&vanilla_game_events::FLAP);
        }
    }

    /// Returns the next step threshold after movement side effects are produced.
    fn next_step(&self) -> f32 {
        self.base().movement_progress().move_dist().floor() + 1.0
    }
}

impl<T: Entity + ?Sized> EntitySounds for T {}
