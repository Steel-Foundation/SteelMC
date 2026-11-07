//! Fire, lava damage, powder snow and freezing.

use super::*;

/// Fire, lava damage, powder snow and freezing.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityHooks`], never by implementing this trait.
pub trait EntityFireAndFreeze: Entity {
    /// Returns whether this entity is currently inside powder snow.
    fn is_in_powder_snow(&self) -> bool {
        self.base().is_in_powder_snow()
    }

    /// Returns whether this entity was inside powder snow during the previous base tick.
    fn was_in_powder_snow(&self) -> bool {
        self.base().was_in_powder_snow()
    }

    /// Returns the current vanilla fire/freeze state.
    fn fire_freeze_state(&self) -> EntityFireFreezeState {
        self.base().fire_freeze_state()
    }

    /// Returns the ticks remaining before this entity stops burning.
    fn remaining_fire_ticks(&self) -> i32 {
        self.base().remaining_fire_ticks()
    }

    /// Sets the ticks remaining before this entity stops burning, clamped to
    /// this entity's fire-ticks cap if any.
    fn set_remaining_fire_ticks(&self, remaining_fire_ticks: i32) {
        self.base().set_remaining_fire_ticks(
            self.remaining_fire_ticks_cap()
                .map_or(remaining_fire_ticks, |cap| remaining_fire_ticks.min(cap)),
        );
        self.sync_base_fire_freeze_entity_data();
    }

    /// Returns ticks this entity has spent freezing in powder snow.
    fn ticks_frozen(&self) -> i32 {
        self.base().ticks_frozen()
    }

    /// Sets ticks this entity has spent freezing in powder snow, syncing the
    /// change to clients.
    fn set_ticks_frozen(&self, ticks_frozen: i32) {
        self.base().set_ticks_frozen(ticks_frozen);
        self.sync_base_fire_freeze_entity_data();
    }

    /// Default for [`Entity::fire_immune`].
    fn fire_immune_default(&self) -> bool {
        self.entity_type().fire_immune
    }

    /// Default for [`Entity::fire_immune_ticks`].
    fn fire_immune_ticks_default(&self) -> i32 {
        0
    }

    /// Default for [`Entity::should_play_lava_hurt_sound`].
    fn should_play_lava_hurt_sound_default(&self) -> bool {
        true
    }

    /// Applies vanilla lava-contact damage after lava ignition effects.
    fn lava_hurt(&self) {
        if self.fire_immune() {
            return;
        }
        let Some(world) = self.level() else {
            return;
        };

        if self.hurt(
            &world,
            &DamageSource::environment(&vanilla_damage_types::LAVA),
            4.0,
        ) && self.should_play_lava_hurt_sound()
        {
            let pitch = rand::random_range(2.0..2.4);
            self.play_sound(&sound_events::ENTITY_GENERIC_BURN, 0.4, pitch);
        }
    }

    /// Default for [`Entity::remaining_fire_ticks_cap`].
    fn remaining_fire_ticks_cap_default(&self) -> Option<i32> {
        None
    }

    /// Default for [`Entity::fire_ignite_extra_ticks`].
    fn fire_ignite_extra_ticks_default(&self) -> i32 {
        0
    }

    /// Returns whether the entity is on fire on the server.
    fn is_on_fire(&self) -> bool {
        self.base().is_on_fire(self.fire_immune())
    }

    /// Returns whether this entity currently renders as visually on fire.
    fn has_visual_fire(&self) -> bool {
        self.base().has_visual_fire()
    }

    /// Returns whether the entity has any frozen ticks.
    fn is_freezing(&self) -> bool {
        self.base().is_freezing()
    }

    /// Default for [`Entity::can_freeze`].
    fn can_freeze_default(&self) -> bool {
        !REGISTRY.entity_types.is_in_tag(
            self.entity_type(),
            &EntityTypeTag::FREEZE_IMMUNE_ENTITY_TYPES,
        )
    }

    /// Returns the ticks of powder-snow exposure required before this entity
    /// starts taking freeze damage.
    fn ticks_required_to_freeze(&self) -> i32 {
        DEFAULT_TICKS_REQUIRED_TO_FREEZE
    }

    /// Returns whether this entity has reached full-freeze duration.
    fn is_fully_frozen(&self) -> bool {
        self.base().is_fully_frozen(self.ticks_required_to_freeze())
    }

    /// Returns how frozen this entity is, from 0.0 to 1.0, based on ticks
    /// frozen versus ticks required.
    fn percent_frozen(&self) -> f32 {
        let ticks_required = self.ticks_required_to_freeze();
        self.ticks_frozen().min(ticks_required) as f32 / ticks_required as f32
    }

    /// Clears accumulated freezing.
    fn clear_freeze(&self) {
        self.base().clear_freeze();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Clears fire without resetting the vanilla fire immunity cooldown.
    fn clear_fire(&self) {
        self.base().clear_fire();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Plays the extinguish sound when currently on fire, then clears fire.
    fn extinguish_fire(&self) {
        if self.is_on_fire() {
            self.play_entity_on_fire_extinguished_sound();
        }
        self.clear_fire();
    }

    /// Ignites this entity for a vanilla tick duration.
    fn ignite_for_ticks(&self, number_of_ticks: i32) {
        self.base()
            .ignite_for_ticks(number_of_ticks, self.remaining_fire_ticks_cap());
        self.sync_base_fire_freeze_entity_data();
    }

    /// Projects base fire/freeze state into generated synced entity data.
    fn sync_base_fire_freeze_entity_data(&self) {
        let Some(synced_data) = self.synced_data() else {
            return;
        };

        synced_data.set_base_ticks_frozen(self.ticks_frozen());
        synced_data.set_base_on_fire_flag(self.is_on_fire() || self.has_visual_fire());
    }

    /// Plays vanilla's extinguished-on-fire entity sound.
    fn play_entity_on_fire_extinguished_sound(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let pitch = 1.6 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
        world.play_sound_at(
            &sound_events::ENTITY_GENERIC_EXTINGUISH_FIRE,
            self.sound_source(),
            self.position(),
            0.7,
            pitch,
            None,
        );
    }
}

impl<T: Entity + ?Sized> EntityFireAndFreeze for T {}
