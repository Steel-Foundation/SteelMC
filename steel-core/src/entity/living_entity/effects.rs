//! Mob effects on living entities.

use super::{
    ActiveMobEffect, Entity, EntityTypeTag, Living, MOB_EFFECT_BEHAVIORS, MobEffectInstance,
    MobEffectRef, ParticleList, REGISTRY, TaggedRegistryExt, vanilla_mob_effects,
};

impl Living<'_> {
    /// `LivingEntity.updateInvisibilityStatus` and `updateGlowingStatus` once effects
    /// changed: effect particles, ambience, invisibility and glowing go to synced data.
    /// Spectators (`ServerPlayer`'s override) stay invisible without particles.
    pub fn sync_mob_effect_data(&self) {
        if !self.living_base().take_effects_dirty() {
            return;
        }

        let mut display = self.living_base().mob_effect_display_state();
        if self.is_spectator() {
            display.particles = ParticleList::default();
            display.invisible = true;
        }
        let (particles, ambient) = (display.particles, display.ambient);
        self.with_synced(move |living| {
            living.effect_particles.set(particles);
            living.effect_ambience.set(ambient);
        });
        let Some(synced) = self.synced_data() else {
            return;
        };
        synced.set_base_invisible_flag(display.invisible);
        synced.set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }

    /// Checks if the entity can be affected by potions.
    #[must_use]
    pub fn is_affected_by_potions(&self) -> bool {
        !self.is_dead_or_dying()
    }

    /// Returns whether healing and harm effects are inverted for this entity (e.g. undead mobs).
    #[must_use]
    pub fn is_inverted_heal_and_harm(&self) -> bool {
        REGISTRY.entity_types.is_in_tag(
            self.entity_type(),
            &EntityTypeTag::INVERTED_HEALING_AND_HARM,
        )
    }

    /// Returns vanilla base `LivingEntity.canBeAffected` eligibility.
    #[must_use]
    pub fn default_can_be_affected(&self, effect: &MobEffectInstance) -> bool {
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IMMUNE_TO_INFESTED)
        {
            return effect.effect() != vanilla_mob_effects::INFESTED;
        }
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IMMUNE_TO_OOZING)
        {
            return effect.effect() != vanilla_mob_effects::OOZING;
        }
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IGNORES_POISON_AND_REGEN)
        {
            return effect.effect() != vanilla_mob_effects::REGENERATION
                && effect.effect() != vanilla_mob_effects::POISON;
        }

        true
    }

    /// Returns whether this entity accepts a mob-effect instance.
    ///
    /// Concrete entities override this for vanilla class-specific immunities.
    #[must_use]
    pub fn can_be_affected_default(&self, effect: &MobEffectInstance) -> bool {
        self.default_can_be_affected(effect)
    }

    /// Returns whether this entity currently has `effect` active.
    #[must_use]
    pub fn has_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.living_base().has_mob_effect(effect)
    }

    /// Returns this entity's active instance of `effect`, if any.
    #[must_use]
    pub fn mob_effect(&self, effect: MobEffectRef) -> Option<ActiveMobEffect> {
        self.living_base().mob_effect(effect)
    }

    /// Returns all active vanilla mob effects.
    #[must_use]
    pub fn active_mob_effects(&self) -> Vec<ActiveMobEffect> {
        self.living_base().active_mob_effects()
    }

    /// Sets active vanilla mob-effect state.
    pub fn set_mob_effect(&self, effect: MobEffectRef, amplifier: i32) {
        self.add_mob_effect(MobEffectInstance::new(effect, amplifier));
    }

    /// Adds or updates active vanilla mob-effect state.
    #[expect(
        clippy::must_use_candidate,
        reason = "callers often ignore whether the effect changed"
    )]
    pub fn add_mob_effect(&self, effect: MobEffectInstance) -> bool {
        self.add_mob_effect_with_source(effect, None)
    }

    /// Adds or updates active vanilla mob-effect state, attributing it to
    /// `source`.
    pub fn add_mob_effect_with_source(
        &self,
        effect: MobEffectInstance,
        source: Option<&dyn Entity>,
    ) -> bool {
        if !self.can_be_affected(&effect) {
            return false;
        }
        let (effect_key, amplifier) = (effect.effect(), effect.amplifier());
        let changed = self.living_base().add_mob_effect(effect);
        // TODO: Trigger CriteriaTriggers.EFFECTS_CHANGED with `source` when `changed`,
        // once the advancement-criteria foundation exists.
        let _ = source;
        // Calls on_effect_started unconditionally, even when it didn't replace a stronger instance.
        let dyn_self = *self;
        MOB_EFFECT_BEHAVIORS
            .get_behavior(effect_key)
            .on_effect_started(&dyn_self, amplifier);
        changed
    }

    /// Sets the presence of a vanilla mob effect.
    pub fn set_mob_effect_active(&self, effect: MobEffectRef, active: bool) {
        if active {
            self.set_mob_effect(effect, 0);
        } else {
            self.remove_mob_effect(effect);
        }
    }

    /// Removes active vanilla mob-effect state.
    #[expect(
        clippy::must_use_candidate,
        reason = "callers often ignore whether an effect was removed"
    )]
    pub fn remove_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.living_base().remove_mob_effect(effect)
    }

    /// Ticks vanilla server-side mob-effect behavior and durations.
    pub fn tick_mob_effects(&self) {
        let world = self.level();
        let dyn_self = *self;
        for effect in self.active_mob_effects() {
            if !effect.has_remaining_duration() {
                self.living_base().tick_mob_effect_duration(effect.effect());
                continue;
            }

            if effect.should_apply_effect_tick_this_tick(self.tick_count())
                && world
                    .as_deref()
                    .is_some_and(|world| !effect.apply_effect_tick(world, &dyn_self))
            {
                self.remove_mob_effect(effect.effect());
                continue;
            }

            self.living_base().tick_mob_effect_duration(effect.effect());
        }
    }
}
