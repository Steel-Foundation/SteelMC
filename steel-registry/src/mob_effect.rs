use std::hash::{Hash, Hasher};

use crate::attribute::{AttributeModifierOperation, AttributeRef};
use crate::particle_type::{ColorParticleOption, ParticleData, ParticleTypeRef};
use rustc_hash::FxHashMap;
use steel_utils::{ArgbColor, Identifier, RgbColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobEffectCategory {
    Beneficial,
    Harmful,
    Neutral,
}

#[derive(Debug)]
pub struct MobEffectAttributeModifier {
    pub attribute: AttributeRef,
    pub id: Identifier,
    pub amount: f64,
    pub operation: AttributeModifierOperation,
}

/// The Vanilla particle factory associated with a mob effect.
#[derive(Debug, Clone, Copy)]
pub enum MobEffectParticle {
    /// Builds a color payload from the effect color and instance ambience.
    EffectColor {
        particle_type: ParticleTypeRef,
        regular_alpha: u8,
        ambient_alpha: u8,
    },
    /// Uses a simple particle without a payload.
    Simple(ParticleTypeRef),
    /// Uses one fixed ARGB color payload for every instance.
    FixedColor {
        particle_type: ParticleTypeRef,
        color: ArgbColor,
    },
}

impl MobEffectParticle {
    #[must_use]
    pub fn create(self, effect_color: RgbColor, ambient: bool) -> ParticleData {
        match self {
            Self::EffectColor {
                particle_type,
                regular_alpha,
                ambient_alpha,
            } => {
                let alpha = if ambient {
                    ambient_alpha
                } else {
                    regular_alpha
                };
                ParticleData::new(
                    particle_type,
                    ColorParticleOption::new(effect_color.with_alpha(alpha)),
                )
            }
            Self::Simple(particle_type) => ParticleData::simple(particle_type),
            Self::FixedColor {
                particle_type,
                color,
            } => ParticleData::new(particle_type, ColorParticleOption::new(color)),
        }
    }
}

#[derive(Debug)]
pub struct MobEffect {
    pub key: Identifier,
    pub category: MobEffectCategory,
    pub color: RgbColor,
    pub particle: MobEffectParticle,
    pub attribute_modifiers: &'static [MobEffectAttributeModifier],
}

impl MobEffect {
    /// Creates the particle options synchronized for one effect instance.
    #[must_use]
    pub fn create_particle_options(&self, ambient: bool) -> ParticleData {
        self.particle.create(self.color, ambient)
    }

    /// Returns the `VarInt` payload used by vanilla mob-effect holder-registry packets.
    #[must_use]
    pub fn packet_holder_id(&self) -> i32 {
        let id = crate::RegistryEntry::id(self);
        debug_assert!(i32::try_from(id).is_ok());
        id as i32
    }
}

impl Hash for MobEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

pub type MobEffectRef = &'static MobEffect;

pub struct MobEffectRegistry {
    effects_by_id: Vec<MobEffectRef>,
    effects_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl Default for MobEffectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MobEffectRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            effects_by_id: Vec::new(),
            effects_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, effect: MobEffectRef) {
        assert!(
            self.allows_registering,
            "Cannot register mob effects after the registry has been frozen"
        );
        let idx = self.effects_by_id.len();
        self.effects_by_key.insert(effect.key.clone(), idx);
        self.effects_by_id.push(effect);
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, MobEffectRef)> + '_ {
        self.effects_by_id
            .iter()
            .enumerate()
            .map(|(id, &effect)| (id, effect))
    }
}

crate::impl_registry!(
    MobEffectRegistry,
    MobEffect,
    effects_by_id,
    effects_by_key,
    mob_effects
);

#[cfg(test)]
mod tests {
    use crate::particle_type::{ColorParticleOption, SimpleParticleOptions};
    use crate::{vanilla_mob_effects, vanilla_particle_types};

    #[test]
    fn generated_effect_particles_match_vanilla_factories() {
        let speed = vanilla_mob_effects::SPEED;
        let regular = speed.create_particle_options(false);
        let ambient = speed.create_particle_options(true);
        let Some(regular_color) = regular.downcast_ref::<ColorParticleOption>() else {
            panic!("speed should use color particle options");
        };
        let Some(ambient_color) = ambient.downcast_ref::<ColorParticleOption>() else {
            panic!("ambient speed should use color particle options");
        };

        assert_eq!(
            regular.particle_type().key,
            vanilla_particle_types::ENTITY_EFFECT.key
        );
        assert_eq!(regular_color.color().alpha(), 255);
        assert_eq!(ambient_color.color().alpha(), 38);
        assert_eq!(
            regular_color.color().rgb().raw() & 0x00ff_ffff,
            speed.color.raw()
        );

        let trial_omen = vanilla_mob_effects::TRIAL_OMEN;
        let trial_particle = trial_omen.create_particle_options(false);
        assert_eq!(
            trial_particle.particle_type().key,
            vanilla_particle_types::TRIAL_OMEN.key
        );
        assert!(
            trial_particle
                .downcast_ref::<SimpleParticleOptions>()
                .is_some()
        );
    }
}

crate::impl_tagged_registry!(MobEffectRegistry, effects_by_key, "mob effect");
