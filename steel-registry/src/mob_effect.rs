use std::hash::{Hash, Hasher};

use crate::attribute::{AttributeModifierOperation, AttributeRef};
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

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

#[derive(Debug)]
pub struct MobEffect {
    pub key: Identifier,
    pub category: MobEffectCategory,
    pub color: i32,
    pub attribute_modifiers: &'static [MobEffectAttributeModifier],
}

impl MobEffect {
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

crate::impl_tagged_registry!(MobEffectRegistry, effects_by_key, "mob effect");
