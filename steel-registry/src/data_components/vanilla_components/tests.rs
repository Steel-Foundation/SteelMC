use super::*;
use crate::RegistryExt;
use serde::Deserialize;
use simdnbt::owned::{NbtCompound, NbtTag};
use std::io::Cursor;

#[derive(Deserialize)]
struct ExtractedComponentCatalog {
    components: Vec<ExtractedComponent>,
}

#[derive(Deserialize)]
struct ExtractedComponent {
    id: usize,
    key: String,
    persistent: bool,
    ignore_swap_animation: bool,
}

#[test]
fn registry_matches_extracted_vanilla_catalog() {
    let catalog: ExtractedComponentCatalog =
        serde_json::from_str(include_str!("../../../build_assets/data_components.json"))
            .expect("extracted component catalog should be valid");
    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);

    assert_eq!(catalog.components.len(), 111);
    assert_eq!(registry.len(), catalog.components.len());
    for (expected_id, component) in catalog.components.into_iter().enumerate() {
        assert_eq!(component.id, expected_id, "{}", component.key);
        let entry = registry
            .by_id(component.id)
            .unwrap_or_else(|| panic!("missing component registry ID {}", component.id));
        assert_eq!(entry.key.to_string(), component.key);
        assert_eq!(entry.is_persistent(), component.persistent, "{}", entry.key);
        assert_eq!(
            entry.ignore_swap_animation(),
            component.ignore_swap_animation,
            "{}",
            entry.key
        );
    }
}

#[test]
fn vanilla_transient_components_are_marked_non_persistent() {
    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);

    for (key, value) in [
        (&CREATIVE_SLOT_LOCK.key, ComponentData::new(())),
        (&ADDITIONAL_TRADE_COST.key, ComponentData::new(3_i32)),
        (
            &MAP_POST_PROCESSING.key,
            ComponentData::new(MapPostProcessing::Lock),
        ),
    ] {
        let entry = registry
            .by_key(key)
            .unwrap_or_else(|| panic!("missing transient component {key}"));
        assert!(!entry.is_persistent(), "{key}");
        assert!(entry.write_nbt(&value).is_err(), "{key}");
        assert!(entry.compute_hash(&value).is_err(), "{key}");
    }
    assert!(matches!(
        registry.by_key(&MAX_STACK_SIZE.key),
        Some(entry) if entry.is_persistent()
    ));
}

#[test]
fn transient_component_network_codecs_match_vanilla() {
    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);

    let additional_trade_cost = registry
        .by_key(&ADDITIONAL_TRADE_COST.key)
        .expect("additional_trade_cost should be registered");
    let mut encoded = Vec::new();
    additional_trade_cost
        .write_network(&ComponentData::new(-7_i32), &mut encoded)
        .expect("additional_trade_cost should encode");
    assert_eq!(
        additional_trade_cost
            .read_network(&mut std::io::Cursor::new(encoded.as_slice()))
            .expect("additional_trade_cost should decode"),
        ComponentData::new(-7_i32)
    );

    let map_post_processing = registry
        .by_key(&MAP_POST_PROCESSING.key)
        .expect("map_post_processing should be registered");
    let mut encoded = Vec::new();
    map_post_processing
        .write_network(&ComponentData::new(MapPostProcessing::Scale), &mut encoded)
        .expect("map_post_processing should encode");
    assert_eq!(
        map_post_processing
            .read_network(&mut std::io::Cursor::new(encoded.as_slice()))
            .expect("map_post_processing should decode"),
        ComponentData::new(MapPostProcessing::Scale)
    );
}

#[test]
fn identifier_component_codecs_use_vanilla_namespace_rules() {
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::PrefixedWrite as _;

    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);
    let expected = Identifier::vanilla_static("stone");

    for component in [ITEM_MODEL, TOOLTIP_STYLE, NOTE_BLOCK_SOUND] {
        let entry = registry
            .by_key(&component.key)
            .unwrap_or_else(|| panic!("missing identifier component {}", component.key));
        let data = ComponentData::new(expected.clone());
        assert_eq!(
            entry.read_nbt_owned(&NbtTag::String("stone".into())),
            Some(ComponentData::new(expected.clone())),
            "{}",
            component.key
        );
        assert_eq!(
            entry
                .write_nbt(&data)
                .expect("persistent identifier should encode"),
            NbtTag::String("minecraft:stone".into()),
            "{}",
            component.key
        );

        let mut abbreviated = Vec::new();
        "stone"
            .write_prefixed::<VarInt>(&mut abbreviated)
            .expect("abbreviated identifier should encode");
        assert_eq!(
            entry
                .read_network(&mut std::io::Cursor::new(abbreviated.as_slice()))
                .expect("abbreviated identifier should decode"),
            ComponentData::new(expected.clone()),
            "{}",
            component.key
        );
        let mut encoded = Vec::new();
        entry
            .write_network(&data, &mut encoded)
            .expect("network identifier should encode");
        let mut canonical = Vec::new();
        "minecraft:stone"
            .write_prefixed::<VarInt>(&mut canonical)
            .expect("canonical identifier should encode");
        assert_eq!(encoded, canonical, "{}", component.key);

        assert_eq!(
            entry
                .compute_hash(&data)
                .expect("persistent identifier should hash"),
            expected.compute_hash(),
            "{}",
            component.key
        );
    }
}

#[test]
fn persistent_scalar_codecs_coerce_numeric_tags_and_enforce_ranges() {
    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);

    let max_stack_size = registry
        .by_key(&MAX_STACK_SIZE.key)
        .expect("max_stack_size should be registered");
    assert_eq!(
        max_stack_size.read_nbt_owned(&NbtTag::Double(16.9)),
        Some(ComponentData::new(16_i32))
    );
    assert_eq!(max_stack_size.read_nbt_owned(&NbtTag::Int(0)), None);

    let minimum_attack_charge = registry
        .by_key(&MINIMUM_ATTACK_CHARGE.key)
        .expect("minimum_attack_charge should be registered");
    assert_eq!(
        minimum_attack_charge.read_nbt_owned(&NbtTag::Double(0.5)),
        Some(ComponentData::new(0.5_f32))
    );
    assert_eq!(
        minimum_attack_charge.read_nbt_owned(&NbtTag::Double(1.5)),
        None
    );

    let glint = registry
        .by_key(&ENCHANTMENT_GLINT_OVERRIDE.key)
        .expect("enchantment_glint_override should be registered");
    assert_eq!(
        glint.read_nbt_owned(&NbtTag::Long(2)),
        Some(ComponentData::new(true))
    );
}

#[test]
fn unit_component_persistence_requires_a_compound() {
    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);
    let unbreakable = registry
        .by_key(&UNBREAKABLE.key)
        .expect("unbreakable should be registered");

    assert_eq!(
        unbreakable.read_nbt_owned(&NbtTag::Compound(NbtCompound::new())),
        Some(ComponentData::new(()))
    );
    assert_eq!(unbreakable.read_nbt_owned(&NbtTag::Byte(1)), None);
}

#[test]
fn unit_component_network_codecs_match_vanilla() {
    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);
    let value = ComponentData::new(());

    for component in [UNBREAKABLE, CREATIVE_SLOT_LOCK, GLIDER] {
        let entry = registry
            .by_key(component.key())
            .unwrap_or_else(|| panic!("{} should be registered", component.key()));
        let mut encoded = Vec::new();
        entry
            .write_network(&value, &mut encoded)
            .unwrap_or_else(|error| panic!("{} should encode: {error}", component.key()));
        assert!(encoded.is_empty(), "{}", component.key());
    }

    let intangible = registry
        .by_key(INTANGIBLE_PROJECTILE.key())
        .expect("intangible_projectile should be registered");
    let mut encoded = Vec::new();
    intangible
        .write_network(&value, &mut encoded)
        .expect("intangible_projectile should encode");

    let mut expected = Vec::new();
    NbtTag::Compound(NbtCompound::new()).write(&mut expected);
    assert_ne!(expected.len(), 0);
    assert_eq!(encoded, expected);

    let mut cursor = Cursor::new(encoded.as_slice());
    assert_eq!(
        intangible
            .read_network(&mut cursor)
            .expect("intangible_projectile should decode"),
        value
    );
    assert_eq!(cursor.position(), encoded.len() as u64);

    let mut invalid = Vec::new();
    NbtTag::Byte(1).write(&mut invalid);
    assert!(
        intangible
            .read_network(&mut Cursor::new(invalid.as_slice()))
            .is_err()
    );
}

#[test]
fn registry_validation_uses_concrete_downcast_keys() {
    let mut registry = DataComponentRegistry::new();
    register_vanilla_data_components(&mut registry);

    let max_stack_size = registry
        .by_key(&MAX_STACK_SIZE.key)
        .expect("max_stack_size should be registered");
    assert!(max_stack_size.validates(&ComponentData::new(16_i32)));
    assert!(!max_stack_size.validates(&ComponentData::new(16.0_f32)));

    let custom_data = registry
        .by_key(&CUSTOM_DATA.key)
        .expect("custom_data should be registered");
    assert!(custom_data.validates(&ComponentData::new(CustomData::default())));
    assert!(!custom_data.validates(&ComponentData::new(())));

    let custom_model_data = registry
        .by_key(&CUSTOM_MODEL_DATA.key)
        .expect("custom_model_data should be registered");
    assert!(custom_model_data.validates(&ComponentData::new(CustomModelData::EMPTY)));
    assert!(!custom_model_data.validates(&ComponentData::new(CustomData::default())));

    let enchantable = registry
        .by_key(&ENCHANTABLE.key)
        .expect("enchantable should be registered");
    assert!(enchantable.validates(&ComponentData::new(
        Enchantable::new(15).expect("15 is positive")
    )));
    assert!(!enchantable.validates(&ComponentData::new(15_i32)));

    for component in [
        DYE,
        BASE_COLOR,
        WOLF_COLLAR,
        TROPICAL_FISH_BASE_COLOR,
        TROPICAL_FISH_PATTERN_COLOR,
        CAT_COLLAR,
        SHEEP_COLOR,
        SHULKER_COLOR,
    ] {
        let entry = registry
            .by_key(&component.key)
            .unwrap_or_else(|| panic!("missing dye color component {}", component.key));
        assert!(entry.validates(&ComponentData::new(DyeColor::Red)));
        assert!(!entry.validates(&ComponentData::new(14_i32)));
    }

    for (key, value) in [
        (
            &DYED_COLOR.key,
            ComponentData::new(DyedItemColor::new(0x123456)),
        ),
        (&MAP_COLOR.key, ComponentData::new(MapItemColor::DEFAULT)),
        (&MAP_ID.key, ComponentData::new(MapId::new(7))),
        (
            &FOOD.key,
            ComponentData::new(FoodProperties::new(4, 2.4, false).expect("food should be valid")),
        ),
        (
            &OMINOUS_BOTTLE_AMPLIFIER.key,
            ComponentData::new(OminousBottleAmplifier::new(2)),
        ),
    ] {
        let entry = registry
            .by_key(key)
            .unwrap_or_else(|| panic!("missing component {key}"));
        assert!(entry.validates(&value), "{key}");
        assert!(!entry.validates(&ComponentData::new(())), "{key}");
    }

    for (key, value) in [
        (&FOX_VARIANT.key, ComponentData::new(FoxVariant::Snow)),
        (&SALMON_SIZE.key, ComponentData::new(SalmonVariant::Large)),
        (&PARROT_VARIANT.key, ComponentData::new(ParrotVariant::Gray)),
        (
            &TROPICAL_FISH_PATTERN.key,
            ComponentData::new(TropicalFishPattern::Clayfish),
        ),
        (
            &MOOSHROOM_VARIANT.key,
            ComponentData::new(MooshroomVariant::Brown),
        ),
        (&RABBIT_VARIANT.key, ComponentData::new(RabbitVariant::Evil)),
        (
            &HORSE_VARIANT.key,
            ComponentData::new(HorseVariant::DarkBrown),
        ),
        (&LLAMA_VARIANT.key, ComponentData::new(LlamaVariant::Gray)),
        (
            &AXOLOTL_VARIANT.key,
            ComponentData::new(AxolotlVariant::Blue),
        ),
    ] {
        let entry = registry
            .by_key(key)
            .unwrap_or_else(|| panic!("missing variant component {key}"));
        assert!(entry.validates(&value), "{key}");
        assert!(!entry.validates(&ComponentData::new(())), "{key}");
    }

    let consumable = registry
        .by_key(&CONSUMABLE.key)
        .expect("consumable should be registered");
    assert!(
        consumable.validates(&ComponentData::new(
            Consumable::new(
                Consumable::DEFAULT_CONSUME_SECONDS,
                ItemUseAnimation::Eat,
                SoundEventHolder::registry(&crate::sound_events::ENTITY_GENERIC_EAT),
                true,
                Vec::new(),
            )
            .expect("default consumable should be valid"),
        ))
    );
    assert!(!consumable.validates(&ComponentData::new(())));
}
