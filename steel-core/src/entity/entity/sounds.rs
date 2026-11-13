//! Entity sounds: steps, swimming, splashes and flapping.

use super::*;

/// Entity sounds: steps, swimming, splashes and flapping.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityProps`], never by implementing this trait.
pub trait EntitySounds: Entity {
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

/// `Entity`'s [`EntityProps::fall_sounds`].
pub fn fall_sounds<E: Entity + ?Sized>(_entity: &E) -> (SoundEventRef, SoundEventRef) {
    (
        &sound_events::ENTITY_GENERIC_SMALL_FALL,
        &sound_events::ENTITY_GENERIC_BIG_FALL,
    )
}

/// `Entity`'s [`EntityProps::sound_source`].
pub const fn sound_source<E: Entity + ?Sized>(_entity: &E) -> SoundSource {
    SoundSource::Neutral
}

/// `Entity`'s [`EntityProps::swim_sound`].
pub fn swim_sound<E: Entity + ?Sized>(_entity: &E) -> SoundEventRef {
    &sound_events::ENTITY_GENERIC_SWIM
}

/// `Entity`'s [`EntityProps::play_sound`].
pub fn play_sound<E: Entity + ?Sized>(entity: &E, sound: SoundEventRef, volume: f32, pitch: f32) {
    if entity.is_silent() {
        return;
    }

    if let Some(world) = entity.level() {
        world.play_sound_at(
            sound,
            entity.sound_source(),
            entity.position(),
            volume,
            pitch,
            None,
        );
    }
}

/// `Entity`'s [`EntityProps::swim_splash_sound`].
pub fn swim_splash_sound<E: Entity + ?Sized>(_entity: &E) -> SoundEventRef {
    &sound_events::ENTITY_GENERIC_SPLASH
}

/// `Entity`'s [`EntityProps::swim_high_speed_splash_sound`].
pub fn swim_high_speed_splash_sound<E: Entity + ?Sized>(_entity: &E) -> SoundEventRef {
    &sound_events::ENTITY_GENERIC_SPLASH
}

/// `Entity`'s [`EntityProps::play_step_sound`].
pub fn play_step_sound<E: Entity + ?Sized>(entity: &E, _pos: BlockPos, block_state: BlockStateId) {
    entity.play_block_step_sound(block_state);
}

/// `Entity`'s [`EntityProps::is_flapping`].
pub const fn is_flapping<E: Entity + ?Sized>(_entity: &E) -> bool {
    false
}

/// `Entity`'s [`EntityProps::on_flap`].
pub const fn on_flap<E: Entity + ?Sized>(_entity: &E) {}
