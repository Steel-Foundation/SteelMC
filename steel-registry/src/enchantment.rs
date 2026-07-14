use crate::enchantment_effect::EnchantmentEffects;
use crate::equipment::EquipmentSlot;
pub use crate::equipment::EquipmentSlotGroup;
use crate::items::ItemRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt, TaggedRegistryExt};
use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::Identifier;

/// Enchanting cost formula: `base + per_level_above_first * (level - 1)`.
#[derive(Debug, Clone, Copy)]
pub struct EnchantmentCost {
    pub base: i32,
    pub per_level_above_first: i32,
}

#[derive(Debug)]
pub struct Enchantment {
    pub key: Identifier,
    pub max_level: u32,
    pub min_cost: EnchantmentCost,
    pub max_cost: EnchantmentCost,
    pub anvil_cost: i32,
    pub weight: u32,
    pub slots: &'static [EquipmentSlotGroup],
    pub supported_items: &'static str,
    pub primary_items: Option<&'static str>,
    pub exclusive_set: Option<&'static str>,
    pub effects_nbt: fn() -> NbtCompound,
    pub effects: EnchantmentEffects,
}

crate::impl_registry_entry_eq!(Enchantment);

impl RegistryEntry for Enchantment {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        REGISTRY.enchantments.id_from_key(&self.key)
    }
}

impl ToNbtTag for &Enchantment {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();

        // description: translatable text component {"translate": "enchantment.minecraft.<key>"}
        let mut desc = NbtCompound::new();
        desc.insert(
            "translate",
            format!("enchantment.{}.{}", self.key.namespace, self.key.path).as_str(),
        );
        compound.insert("description", NbtTag::Compound(desc));

        // Definition fields (inlined, not nested)
        compound.insert("supported_items", self.supported_items);
        if let Some(primary) = self.primary_items {
            compound.insert("primary_items", primary);
        }
        compound.insert("weight", self.weight as i32);
        compound.insert("max_level", self.max_level as i32);

        let mut min_cost = NbtCompound::new();
        min_cost.insert("base", self.min_cost.base);
        min_cost.insert("per_level_above_first", self.min_cost.per_level_above_first);
        compound.insert("min_cost", NbtTag::Compound(min_cost));

        let mut max_cost = NbtCompound::new();
        max_cost.insert("base", self.max_cost.base);
        max_cost.insert("per_level_above_first", self.max_cost.per_level_above_first);
        compound.insert("max_cost", NbtTag::Compound(max_cost));

        compound.insert("anvil_cost", self.anvil_cost);

        let slots: Vec<String> = self.slots.iter().map(|s| s.as_str().to_owned()).collect();
        compound.insert("slots", NbtTag::List(NbtList::from(slots)));

        if let Some(exclusive) = self.exclusive_set {
            compound.insert("exclusive_set", exclusive);
        }

        let effects = (self.effects_nbt)();
        if !effects.is_empty() {
            compound.insert("effects", NbtTag::Compound(effects));
        }

        NbtTag::Compound(compound)
    }
}

/// Parses a tag reference string like `"#minecraft:foo"` into an `Identifier`.
fn parse_tag_ref(tag_ref: &str) -> Option<Identifier> {
    let without_hash = tag_ref.strip_prefix('#')?;
    Some(if let Some((ns, path)) = without_hash.split_once(':') {
        Identifier::new(ns.to_owned(), path.to_owned())
    } else {
        Identifier::vanilla(without_hash.to_owned())
    })
}

impl Enchantment {
    /// Vanilla `Enchantment::matchingSlot`.
    #[must_use]
    pub fn matching_slot(&self, slot: EquipmentSlot) -> bool {
        self.slots.iter().any(|group| group.test(slot))
    }

    /// Checks if this enchantment can be applied to the given item via `supported_items` tag.
    pub fn can_enchant(&self, item: ItemRef) -> bool {
        let Some(tag) = parse_tag_ref(self.supported_items) else {
            return false;
        };
        REGISTRY.items.is_in_tag(item, &tag)
    }

    /// Checks if two enchantments are compatible (neither's `exclusive_set` contains the other).
    #[must_use]
    pub fn are_compatible(a: EnchantmentRef, b: EnchantmentRef) -> bool {
        if a == b {
            return false;
        }
        if let Some(set) = a.exclusive_set
            && let Some(tag) = parse_tag_ref(set)
            && REGISTRY.enchantments.is_in_tag(b, &tag)
        {
            return false;
        }
        if let Some(set) = b.exclusive_set
            && let Some(tag) = parse_tag_ref(set)
            && REGISTRY.enchantments.is_in_tag(a, &tag)
        {
            return false;
        }
        true
    }

    /// Checks if this enchantment is compatible with all existing enchantments on an item.
    #[must_use]
    pub fn is_compatible_with_existing(
        enchantment: EnchantmentRef,
        item: &crate::item_stack::ItemStack,
    ) -> bool {
        let Some(enchantments) = item.get_enchantments() else {
            return true;
        };
        for (existing_key, _) in enchantments.iter() {
            let Some(existing) = REGISTRY.enchantments.by_key(existing_key) else {
                continue;
            };
            if !Self::are_compatible(enchantment, existing) {
                return false;
            }
        }
        true
    }
}

pub type EnchantmentRef = &'static Enchantment;

pub struct EnchantmentRegistry {
    enchantments_by_id: Vec<EnchantmentRef>,
    enchantments_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl EnchantmentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            enchantments_by_id: Vec::new(),
            enchantments_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_registry_ext!(
    EnchantmentRegistry,
    Enchantment,
    enchantments_by_id,
    enchantments_by_key
);

crate::impl_standard_methods!(
    EnchantmentRegistry,
    EnchantmentRef,
    enchantments_by_id,
    enchantments_by_key,
    allows_registering
);

crate::impl_tagged_registry!(EnchantmentRegistry, enchantments_by_key, "enchantment");

#[cfg(test)]
mod tests {
    use super::Enchantment;
    use crate::enchantment_effect::{
        DamageSourcePredicate, EnchantmentEffectComponent, EnchantmentEffectRequirements,
        EnchantmentEntityEffect, EnchantmentTarget,
    };
    use crate::equipment::EquipmentSlot;
    use crate::item_stack::ItemStack;
    use crate::{test_support::init_test_registry, vanilla_enchantments, vanilla_items};
    use simdnbt::ToNbtTag;
    use simdnbt::owned::{NbtList, NbtTag};
    use steel_utils::Identifier;

    #[test]
    fn binding_curse_has_prevent_armor_change_effect() {
        assert!(
            vanilla_enchantments::BINDING_CURSE
                .effects
                .has(EnchantmentEffectComponent::PreventArmorChange)
        );
    }

    #[test]
    fn enchantment_matching_slot_uses_slot_groups() {
        assert!(vanilla_enchantments::BINDING_CURSE.matching_slot(EquipmentSlot::Head));
        assert!(!vanilla_enchantments::BINDING_CURSE.matching_slot(EquipmentSlot::MainHand));
    }

    #[test]
    fn existing_identical_or_exclusive_enchantments_are_incompatible() {
        init_test_registry();
        let mut sword = ItemStack::new(&vanilla_items::ITEMS.diamond_sword);
        sword.upgrade_enchantment(vanilla_enchantments::SHARPNESS.key.clone(), 1);

        assert!(!Enchantment::is_compatible_with_existing(
            &vanilla_enchantments::SHARPNESS,
            &sword
        ));
        assert!(!Enchantment::is_compatible_with_existing(
            &vanilla_enchantments::SMITE,
            &sword
        ));
        assert!(Enchantment::is_compatible_with_existing(
            &vanilla_enchantments::UNBREAKING,
            &sword
        ));
    }

    #[test]
    fn enchantment_nbt_includes_raw_effect_payloads() {
        let NbtTag::Compound(compound) = (&vanilla_enchantments::LUNGE).to_nbt_tag() else {
            panic!("enchantment NBT should be a compound");
        };
        let Some(NbtTag::Compound(effects)) = compound.get("effects") else {
            panic!("enchantment NBT should include effects");
        };
        let Some(NbtTag::List(NbtList::Compound(post_piercing))) =
            effects.get("minecraft:post_piercing_attack")
        else {
            panic!("Lunge should include post-piercing attack effects");
        };

        assert_eq!(post_piercing.len(), 1);
        let Some(NbtTag::Compound(effect)) = post_piercing[0].get("effect") else {
            panic!("post-piercing entry should include an effect compound");
        };
        assert_eq!(
            effect.string("type").map(ToString::to_string).as_deref(),
            Some("minecraft:all_of")
        );

        let Some(NbtTag::List(NbtList::Compound(children))) = effect.get("effects") else {
            panic!("Lunge all_of effect should include child effects");
        };
        let impulse = children
            .iter()
            .find(|child| {
                child.string("type").map(ToString::to_string).as_deref()
                    == Some("minecraft:apply_impulse")
            })
            .expect("Lunge should include apply_impulse");

        assert!(matches!(
            impulse.get("direction"),
            Some(NbtTag::List(NbtList::Double(values))) if values == &[0.0, 0.0, 1.0]
        ));
        assert!(matches!(
            impulse.get("coordinate_scale"),
            Some(NbtTag::List(NbtList::Double(values))) if values == &[1.0, 0.0, 1.0]
        ));
        assert!(matches!(
            impulse.get("magnitude").and_then(|tag| tag.compound()).and_then(|compound| compound.get("base")),
            Some(NbtTag::Float(value)) if value.to_bits() == 0.458_f32.to_bits()
        ));
    }

    #[test]
    fn movement_requirements_use_vanilla_double_bounds_in_nbt() {
        let NbtTag::Compound(compound) = (&vanilla_enchantments::WIND_BURST).to_nbt_tag() else {
            panic!("enchantment NBT should be a compound");
        };
        let Some(NbtTag::Compound(effects)) = compound.get("effects") else {
            panic!("enchantment NBT should include effects");
        };
        let Some(NbtTag::List(NbtList::Compound(post_attack))) =
            effects.get("minecraft:post_attack")
        else {
            panic!("Wind Burst should include post-attack effects");
        };
        let Some(NbtTag::Compound(requirements)) = post_attack[0].get("requirements") else {
            panic!("Wind Burst effect should include requirements");
        };
        let Some(NbtTag::Compound(predicate)) = requirements.get("predicate") else {
            panic!("entity_properties requirements should include predicate");
        };
        let Some(NbtTag::Compound(movement)) = predicate.get("minecraft:movement") else {
            panic!("Wind Burst predicate should include movement bounds");
        };
        let Some(NbtTag::Compound(fall_distance)) = movement.get("fall_distance") else {
            panic!("movement predicate should include fall distance bounds");
        };

        assert!(matches!(
            fall_distance.get("min"),
            Some(NbtTag::Double(value)) if value.to_bits() == 1.5_f64.to_bits()
        ));
    }

    #[test]
    fn unconditional_value_effects_modify_values() {
        assert_eq!(vanilla_enchantments::KNOCKBACK.effects.knockback.len(), 1);
        let knockback = &vanilla_enchantments::KNOCKBACK.effects.knockback[0];

        assert!(knockback.is_unconditional());
        assert_eq!(
            knockback
                .effect
                .process_without_random(2, 0.0)
                .map(f32::to_bits),
            Some(2.0_f32.to_bits())
        );

        assert_eq!(vanilla_enchantments::SHARPNESS.effects.damage.len(), 1);
        let damage = &vanilla_enchantments::SHARPNESS.effects.damage[0];

        assert!(damage.is_unconditional());
        assert_eq!(
            damage
                .effect
                .process_without_random(5, 7.0)
                .map(f32::to_bits),
            Some(10.0_f32.to_bits())
        );
    }

    #[test]
    fn conditional_value_effects_are_not_applied_without_context() {
        assert_eq!(vanilla_enchantments::PUNCH.effects.knockback.len(), 1);
        assert!(!vanilla_enchantments::PUNCH.effects.knockback[0].is_unconditional());
    }

    #[test]
    fn looting_equipment_drops_preserves_attacker_target() {
        let effects = vanilla_enchantments::LOOTING.effects.equipment_drops;

        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].enchanted, EnchantmentTarget::Attacker);
        assert_eq!(effects[0].affected, EnchantmentTarget::Victim);
    }

    #[test]
    fn frost_walker_damage_immunity_preserves_damage_source_requirements() {
        assert!(
            vanilla_enchantments::FROST_WALKER
                .effects
                .has(EnchantmentEffectComponent::DamageImmunity)
        );

        let effects = vanilla_enchantments::FROST_WALKER.effects.damage_immunity;
        assert_eq!(effects.len(), 1);
        let Some(requirements) = effects[0].requirements else {
            panic!("Frost Walker damage immunity should have requirements");
        };
        let EnchantmentEffectRequirements::DamageSourceProperties(DamageSourcePredicate {
            tags,
            is_direct,
        }) = requirements
        else {
            panic!("Frost Walker damage immunity should use damage-source requirements");
        };

        assert_eq!(*is_direct, None);
        assert!(tags.iter().any(|tag| {
            tag.tag == Identifier::vanilla_static("burn_from_stepping") && tag.expected
        }));
        assert!(tags.iter().any(|tag| {
            tag.tag == Identifier::vanilla_static("bypasses_invulnerability") && !tag.expected
        }));
    }

    #[test]
    fn lunge_post_piercing_attack_preserves_entity_effects() {
        assert!(
            vanilla_enchantments::LUNGE
                .effects
                .has(EnchantmentEffectComponent::PostPiercingAttack)
        );

        let effects = vanilla_enchantments::LUNGE.effects.post_piercing_attack;
        assert_eq!(effects.len(), 1);
        let EnchantmentEntityEffect::AllOf(children) = &effects[0].effect else {
            panic!("Lunge post-piercing effect should be an all_of entity effect");
        };

        assert_eq!(children.len(), 4);
        assert!(
            children
                .iter()
                .any(|effect| matches!(effect, EnchantmentEntityEffect::ChangeItemDamage { .. }))
        );
        assert!(
            children
                .iter()
                .any(|effect| matches!(effect, EnchantmentEntityEffect::ApplyExhaustion { .. }))
        );
        assert!(
            children
                .iter()
                .any(|effect| matches!(effect, EnchantmentEntityEffect::ApplyImpulse { .. }))
        );
        assert!(
            children
                .iter()
                .any(|effect| matches!(effect, EnchantmentEntityEffect::PlaySound { .. }))
        );
        assert!(effects[0].requirements.is_some());
    }

    #[test]
    fn thorns_post_attack_preserves_random_damage_and_item_damage_effects() {
        assert!(
            vanilla_enchantments::THORNS
                .effects
                .has(EnchantmentEffectComponent::PostAttack)
        );

        let effects = vanilla_enchantments::THORNS.effects.post_attack;
        assert!(effects.iter().any(|effect| {
            effect
                .requirements
                .is_some_and(requirements_contain_random_chance)
        }));
        assert!(
            effects
                .iter()
                .any(|effect| entity_effect_contains_damage_entity(&effect.effect))
        );
        assert!(
            effects
                .iter()
                .any(|effect| entity_effect_contains_change_item_damage(&effect.effect))
        );
    }

    fn requirements_contain_random_chance(requirements: &EnchantmentEffectRequirements) -> bool {
        match requirements {
            EnchantmentEffectRequirements::AllOf(children)
            | EnchantmentEffectRequirements::AnyOf(children) => children
                .iter()
                .any(|child| requirements_contain_random_chance(child)),
            EnchantmentEffectRequirements::Inverted(child) => {
                requirements_contain_random_chance(child)
            }
            EnchantmentEffectRequirements::RandomChance { .. } => true,
            EnchantmentEffectRequirements::EntityProperties { .. }
            | EnchantmentEffectRequirements::DamageSourceProperties(_)
            | EnchantmentEffectRequirements::Unsupported { .. } => false,
        }
    }

    fn entity_effect_contains_damage_entity(effect: &EnchantmentEntityEffect) -> bool {
        match effect {
            EnchantmentEntityEffect::AllOf(children) => children
                .iter()
                .any(|child| entity_effect_contains_damage_entity(child)),
            EnchantmentEntityEffect::DamageEntity { .. } => true,
            EnchantmentEntityEffect::ChangeItemDamage { .. }
            | EnchantmentEntityEffect::ApplyExhaustion { .. }
            | EnchantmentEntityEffect::ApplyImpulse { .. }
            | EnchantmentEntityEffect::PlaySound { .. }
            | EnchantmentEntityEffect::Ignite { .. }
            | EnchantmentEntityEffect::ApplyMobEffect { .. }
            | EnchantmentEntityEffect::Unsupported { .. } => false,
        }
    }

    fn entity_effect_contains_change_item_damage(effect: &EnchantmentEntityEffect) -> bool {
        match effect {
            EnchantmentEntityEffect::AllOf(children) => children
                .iter()
                .any(|child| entity_effect_contains_change_item_damage(child)),
            EnchantmentEntityEffect::ChangeItemDamage { .. } => true,
            EnchantmentEntityEffect::ApplyExhaustion { .. }
            | EnchantmentEntityEffect::ApplyImpulse { .. }
            | EnchantmentEntityEffect::PlaySound { .. }
            | EnchantmentEntityEffect::DamageEntity { .. }
            | EnchantmentEntityEffect::Ignite { .. }
            | EnchantmentEntityEffect::ApplyMobEffect { .. }
            | EnchantmentEntityEffect::Unsupported { .. } => false,
        }
    }
}
