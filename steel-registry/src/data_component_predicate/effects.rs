use rustc_hash::FxHashMap;

use super::{Debug, IntBounds, NbtCompound, NbtTag, decode_optional};
use crate::mob_effect::MobEffectRef;
use crate::mob_effect::instance::MobEffectInstance;
use crate::{REGISTRY, RegistryExt};
use steel_utils::nbt::NbtNumeric;

/// Accepted status effects and per-effect instance constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobEffectsPredicate {
    effect_map: FxHashMap<MobEffectRef, MobEffectInstancePredicate>,
}

impl MobEffectsPredicate {
    #[must_use]
    pub const fn new(effect_map: FxHashMap<MobEffectRef, MobEffectInstancePredicate>) -> Self {
        Self { effect_map }
    }

    /// Tests one instance against the entry for its effect.
    #[must_use]
    pub fn test(&self, instance: &MobEffectInstance) -> bool {
        self.effect_map
            .get(&instance.effect())
            .is_some_and(|predicate| predicate.matches(instance))
    }

    pub(super) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let mut effect_map = FxHashMap::default();
        effect_map.reserve(compound.len());
        for (key, value) in compound.iter() {
            let key = key.to_owned().try_into_string().ok()?.parse().ok()?;
            let effect = REGISTRY.mob_effects.by_key(&key)?;
            effect_map.insert(effect, MobEffectInstancePredicate::from_owned_nbt(value)?);
        }
        Some(Self::new(effect_map))
    }

    pub(super) fn to_nbt_value(&self) -> NbtTag {
        let mut entries = self.effect_map.iter().collect::<Vec<_>>();
        entries.sort_unstable_by(|(left, _), (right, _)| left.key.cmp(&right.key));
        let mut compound = NbtCompound::new();
        for (effect, predicate) in entries {
            compound.insert(effect.key.to_string(), predicate.to_nbt_value());
        }
        NbtTag::Compound(compound)
    }
}

/// Bounds on the amplifier and duration of one required effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobEffectInstancePredicate {
    amplifier: IntBounds,
    duration: IntBounds,
    ambient: Option<bool>,
    visible: Option<bool>,
}

impl MobEffectInstancePredicate {
    #[must_use]
    pub const fn new(
        amplifier: IntBounds,
        duration: IntBounds,
        ambient: Option<bool>,
        visible: Option<bool>,
    ) -> Self {
        Self {
            amplifier,
            duration,
            ambient,
            visible,
        }
    }

    #[must_use]
    pub fn matches(&self, instance: &MobEffectInstance) -> bool {
        self.amplifier.matches(instance.amplifier())
            && self.duration.matches(instance.duration())
            && self
                .ambient
                .is_none_or(|ambient| ambient == instance.ambient())
            && self
                .visible
                .is_none_or(|visible| visible == instance.show_particles())
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            compound
                .get("amplifier")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
            compound
                .get("duration")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
            decode_optional(compound, "ambient", NbtNumeric::codec_bool)?,
            decode_optional(compound, "visible", NbtNumeric::codec_bool)?,
        ))
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !self.amplifier.is_any() {
            compound.insert("amplifier", self.amplifier.as_nbt_tag());
        }
        if !self.duration.is_any() {
            compound.insert("duration", self.duration.as_nbt_tag());
        }
        if let Some(ambient) = self.ambient {
            compound.insert("ambient", ambient);
        }
        if let Some(visible) = self.visible {
            compound.insert("visible", visible);
        }
        NbtTag::Compound(compound)
    }
}
