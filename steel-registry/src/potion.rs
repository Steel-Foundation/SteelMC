//! Potion registry values extracted from Vanilla.

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::mob_effect::MobEffectRef;

/// One base effect supplied by a registered potion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionEffect {
    pub effect: MobEffectRef,
    pub duration: i32,
    pub amplifier: i32,
}

/// Registered potion definition.
#[derive(Debug)]
pub struct Potion {
    pub key: Identifier,
    pub effects: &'static [PotionEffect],
}

impl Potion {
    #[must_use]
    pub const fn new(key: Identifier, effects: &'static [PotionEffect]) -> Self {
        Self { key, effects }
    }
}

pub type PotionRef = &'static Potion;

pub struct PotionRegistry {
    potions_by_id: Vec<PotionRef>,
    potions_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl PotionRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            potions_by_id: Vec::new(),
            potions_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    PotionRegistry,
    PotionRef,
    potions_by_id,
    potions_by_key,
    allows_registering
);

crate::impl_registry!(
    PotionRegistry,
    Potion,
    potions_by_id,
    potions_by_key,
    potions
);
crate::impl_tagged_registry!(PotionRegistry, potions_by_key, "potion");

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;

    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt, TaggedRegistryExt};

    #[test]
    fn extracted_potions_follow_vanilla_ids_and_effects() {
        init_test_registry();
        assert_eq!(REGISTRY.potions.len(), 46);
        assert_eq!(
            REGISTRY.potions.by_id(0).map(|potion| &potion.key),
            Some(&Identifier::vanilla_static("water"))
        );
        let turtle_master = REGISTRY
            .potions
            .by_key(&Identifier::vanilla_static("turtle_master"))
            .expect("turtle master should be registered");
        assert_eq!(turtle_master.effects.len(), 2);
        assert_eq!(
            turtle_master.effects[0].effect.key.path.as_ref(),
            "slowness"
        );
        assert_eq!(
            turtle_master.effects[1].effect.key.path.as_ref(),
            "resistance"
        );
    }

    #[test]
    fn vanilla_potion_tags_are_generated_from_builtin_data() {
        init_test_registry();
        let tradeable = Identifier::vanilla_static("tradeable");
        assert!(REGISTRY.potions.get_tag(&tradeable).is_some());
    }
}
