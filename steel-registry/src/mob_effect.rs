use rustc_hash::FxHashMap;
use steel_utils::Identifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobEffectCategory {
    Beneficial,
    Harmful,
    Neutral,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct MobEffect {
    pub key: Identifier,
    pub category: MobEffectCategory,
    pub color: i32,
}

pub type MobEffectRef = &'static MobEffect;

pub struct MobEffectRegistry {
    effects_by_id: Vec<MobEffectRef>,
    effects_by_key: FxHashMap<Identifier, usize>,
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
