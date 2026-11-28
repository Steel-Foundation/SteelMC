use super::*;
use crate::data_components::vanilla_components::{CONTAINER, DAMAGE, MAX_DAMAGE};
use crate::item_stack::ItemStack;
use crate::{ItemStackTemplate, init_vanilla_registry, vanilla_items};

#[derive(Clone, Debug, PartialEq)]
struct MinimumDamage(i32);

// SAFETY: This test-owned key uniquely identifies MinimumDamage in the process.
unsafe impl DowncastType for MinimumDamage {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:test/data_component_predicate/minimum_damage");
}

impl DataComponentPredicate for MinimumDamage {
    fn matches(&self, components: &dyn DataComponentGetter) -> bool {
        components
            .component(DAMAGE)
            .is_some_and(|damage| *damage >= self.0)
    }
}

impl DataComponentPredicateCodec for MinimumDamage {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        tag.codec_i32().map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        NbtTag::Int(self.0)
    }
}

impl HashComponent for MinimumDamage {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

#[test]
fn custom_predicate_dispatches_without_builtin_type_cases() {
    init_vanilla_registry();
    static PREDICATE_TYPE: DataComponentPredicateType =
        DataComponentPredicateType::of::<MinimumDamage>(Identifier::new_static(
            "steel_test",
            "minimum_damage",
        ));
    let predicate = DataComponentPredicateData::new(&PREDICATE_TYPE, MinimumDamage(7));
    assert!(predicate.downcast_ref::<DamagePredicate>().is_none());
    assert_eq!(
        predicate.downcast_ref::<MinimumDamage>(),
        Some(&MinimumDamage(7))
    );

    let mut stack = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    assert!(!predicate.matches(&stack));
    stack.set(DAMAGE, 7);
    assert!(predicate.matches(&stack));
    let template = ItemStackTemplate::from_stack(&stack).expect("test sword should persist");
    assert!(predicate.matches(&template));
    stack.remove(DAMAGE);
    assert!(!predicate.matches(&stack));
    assert!(predicate.matches(&template));
}

#[test]
fn named_enchantment_predicates_require_a_nonzero_level() {
    init_vanilla_registry();
    let sharpness = &crate::vanilla_enchantments::SHARPNESS;
    let predicate = EnchantmentPredicate::new(
        Some(RegistryHolderSet::Direct(vec![sharpness])),
        IntBounds::ANY,
    );
    let mut enchantments = vanilla_components::ItemEnchantments::empty();
    // The component codecs can retain level zero, unlike ItemEnchantments::set.
    enchantments.levels.insert(sharpness.key.clone(), 0);
    assert!(!predicate.contained_in(&enchantments));
    assert!(EnchantmentPredicate::new(None, IntBounds::exactly(0)).contained_in(&enchantments));
    enchantments.set(sharpness.key.clone(), 1);
    assert!(predicate.contained_in(&enchantments));
}

#[test]
fn nested_predicates_combine_exact_partial_and_presence_conditions() {
    init_vanilla_registry();
    let mut exact = DataComponentMap::new();
    exact.set(DAMAGE, Some(7));
    let max_damage = REGISTRY
        .data_components
        .by_key(MAX_DAMAGE.key())
        .expect("maximum damage component should be registered");
    let components = DataComponentMatchers::new(
        DataComponentExactPredicate::all_of(&exact).expect("damage should persist"),
        vec![
            DataComponentPredicateData::new(
                &vanilla_data_component_predicate_types::DAMAGE,
                DamagePredicate::new(IntBounds::ANY, IntBounds::exactly(7)),
            ),
            DataComponentPredicateData::any(max_damage),
        ],
    )
    .expect("predicate keys should be unique");
    let item = ItemPredicate::new(
        Some(RegistryHolderSet::Direct(vec![
            &vanilla_items::DIAMOND_SWORD,
        ])),
        IntBounds::exactly(1),
        components,
    );
    let predicate = DataComponentPredicateData::new(
        &vanilla_data_component_predicate_types::CONTAINER,
        ContainerPredicate::new(Some(CollectionPredicate::new(
            Some(vec![item]),
            None,
            Some(IntBounds::exactly(1)),
        ))),
    );
    let mut sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    let mut chest = ItemStack::new(&vanilla_items::CHEST);
    for (damage, matches) in [(7, true), (6, false)] {
        sword.set(DAMAGE, damage);
        let template = ItemStackTemplate::from_stack(&sword).expect("test sword should persist");
        chest.set(
            CONTAINER,
            vanilla_components::ItemContainerContents::new(vec![None, Some(template)])
                .expect("test contents should be valid"),
        );
        assert_eq!(predicate.matches(&chest), matches);
    }

    sword.set(DAMAGE, 7);
    sword.remove(MAX_DAMAGE);
    let template = ItemStackTemplate::from_stack(&sword).expect("test sword should persist");
    chest.set(
        CONTAINER,
        vanilla_components::ItemContainerContents::new(vec![Some(template)])
            .expect("test contents should be valid"),
    );
    assert!(!predicate.matches(&chest));
    chest.remove(CONTAINER);
    assert!(!predicate.matches(&chest));
}
