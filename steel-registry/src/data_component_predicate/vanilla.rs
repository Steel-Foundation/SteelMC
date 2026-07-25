use super::{
    AttributeModifiersPredicate, BundlePredicate, ContainerPredicate, CustomDataPredicate,
    DamagePredicate, DataComponentPredicateType, DataComponentPredicateTypeRegistry,
    EnchantmentsPredicate, FireworkExplosionPredicate, FireworksPredicate,
    JukeboxPlayablePredicate, PotionsPredicate, StoredEnchantmentsPredicate, TrimPredicate,
    VillagerTypePredicate, WritableBookPredicate, WrittenBookPredicate,
};

/// Vanilla component-predicate types in protocol registry order.
pub mod vanilla_data_component_predicate_types {
    use super::{
        AttributeModifiersPredicate, BundlePredicate, ContainerPredicate, CustomDataPredicate,
        DamagePredicate, DataComponentPredicateType, DataComponentPredicateTypeRegistry,
        EnchantmentsPredicate, FireworkExplosionPredicate, FireworksPredicate,
        JukeboxPlayablePredicate, PotionsPredicate, StoredEnchantmentsPredicate, TrimPredicate,
        VillagerTypePredicate, WritableBookPredicate, WrittenBookPredicate,
    };
    use steel_utils::Identifier;

    pub static DAMAGE: DataComponentPredicateType =
        DataComponentPredicateType::of::<DamagePredicate>(Identifier::vanilla_static("damage"));
    pub static ENCHANTMENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<EnchantmentsPredicate>(Identifier::vanilla_static(
            "enchantments",
        ));
    pub static STORED_ENCHANTMENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<StoredEnchantmentsPredicate>(Identifier::vanilla_static(
            "stored_enchantments",
        ));
    pub static POTION_CONTENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<PotionsPredicate>(Identifier::vanilla_static(
            "potion_contents",
        ));
    pub static CUSTOM_DATA: DataComponentPredicateType =
        DataComponentPredicateType::of::<CustomDataPredicate>(Identifier::vanilla_static(
            "custom_data",
        ));
    pub static CONTAINER: DataComponentPredicateType =
        DataComponentPredicateType::of::<ContainerPredicate>(Identifier::vanilla_static(
            "container",
        ));
    pub static BUNDLE_CONTENTS: DataComponentPredicateType =
        DataComponentPredicateType::of::<BundlePredicate>(Identifier::vanilla_static(
            "bundle_contents",
        ));
    pub static FIREWORK_EXPLOSION: DataComponentPredicateType =
        DataComponentPredicateType::of::<FireworkExplosionPredicate>(Identifier::vanilla_static(
            "firework_explosion",
        ));
    pub static FIREWORKS: DataComponentPredicateType =
        DataComponentPredicateType::of::<FireworksPredicate>(Identifier::vanilla_static(
            "fireworks",
        ));
    pub static WRITABLE_BOOK_CONTENT: DataComponentPredicateType =
        DataComponentPredicateType::of::<WritableBookPredicate>(Identifier::vanilla_static(
            "writable_book_content",
        ));
    pub static WRITTEN_BOOK_CONTENT: DataComponentPredicateType =
        DataComponentPredicateType::of::<WrittenBookPredicate>(Identifier::vanilla_static(
            "written_book_content",
        ));
    pub static ATTRIBUTE_MODIFIERS: DataComponentPredicateType =
        DataComponentPredicateType::of::<AttributeModifiersPredicate>(Identifier::vanilla_static(
            "attribute_modifiers",
        ));
    pub static TRIM: DataComponentPredicateType =
        DataComponentPredicateType::of::<TrimPredicate>(Identifier::vanilla_static("trim"));
    pub static JUKEBOX_PLAYABLE: DataComponentPredicateType =
        DataComponentPredicateType::of::<JukeboxPlayablePredicate>(Identifier::vanilla_static(
            "jukebox_playable",
        ));
    pub static VILLAGER_VARIANT: DataComponentPredicateType =
        DataComponentPredicateType::of::<VillagerTypePredicate>(Identifier::vanilla_static(
            "villager/variant",
        ));

    pub fn register_data_component_predicate_types(
        registry: &mut DataComponentPredicateTypeRegistry,
    ) {
        registry.register(&DAMAGE);
        registry.register(&ENCHANTMENTS);
        registry.register(&STORED_ENCHANTMENTS);
        registry.register(&POTION_CONTENTS);
        registry.register(&CUSTOM_DATA);
        registry.register(&CONTAINER);
        registry.register(&BUNDLE_CONTENTS);
        registry.register(&FIREWORK_EXPLOSION);
        registry.register(&FIREWORKS);
        registry.register(&WRITABLE_BOOK_CONTENT);
        registry.register(&WRITTEN_BOOK_CONTENT);
        registry.register(&ATTRIBUTE_MODIFIERS);
        registry.register(&TRIM);
        registry.register(&JUKEBOX_PLAYABLE);
        registry.register(&VILLAGER_VARIANT);
    }
}
