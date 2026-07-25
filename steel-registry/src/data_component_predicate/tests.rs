use std::io::Cursor;

use super::core::write_registry_id;
use super::*;
use crate::data_components::components::OminousBottleAmplifier;
use crate::data_components::vanilla_components::{
    CAN_BREAK, DAMAGE, LOCK, OMINOUS_BOTTLE_AMPLIFIER,
};
use crate::data_components::{ComponentData, DataComponentMap};
use crate::item_predicate::{AdventureModePredicate, BlockPredicate, LockCode};
use crate::test_support::init_test_registry;
use crate::{RegistryHolderSet, vanilla_blocks, vanilla_items};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};

#[test]
fn vanilla_predicate_types_follow_registry_order() {
    init_test_registry();
    let expected = [
        "damage",
        "enchantments",
        "stored_enchantments",
        "potion_contents",
        "custom_data",
        "container",
        "bundle_contents",
        "firework_explosion",
        "fireworks",
        "writable_book_content",
        "written_book_content",
        "attribute_modifiers",
        "trim",
        "jukebox_playable",
        "villager/variant",
    ];

    assert_eq!(
        REGISTRY.data_component_predicate_types.len(),
        expected.len()
    );
    for (id, path) in expected.into_iter().enumerate() {
        assert_eq!(
            REGISTRY
                .data_component_predicate_types
                .by_id(id)
                .map(|entry| entry.key.clone()),
            Some(Identifier::vanilla_static(path))
        );
    }
}

#[test]
fn every_builtin_predicate_payload_round_trips_persistence_and_network() {
    init_test_registry();

    let mut twinkle = NbtCompound::new();
    twinkle.insert("has_twinkle", true);
    let samples = [
        ("damage", NbtTag::Compound(NbtCompound::new())),
        ("enchantments", NbtTag::List(NbtList::Compound(Vec::new()))),
        (
            "stored_enchantments",
            NbtTag::List(NbtList::Compound(Vec::new())),
        ),
        ("potion_contents", NbtTag::String("minecraft:water".into())),
        ("custom_data", NbtTag::String("{}".into())),
        ("container", NbtTag::Compound(NbtCompound::new())),
        ("bundle_contents", NbtTag::Compound(NbtCompound::new())),
        ("firework_explosion", NbtTag::Compound(twinkle)),
        ("fireworks", NbtTag::Compound(NbtCompound::new())),
        (
            "writable_book_content",
            NbtTag::Compound(NbtCompound::new()),
        ),
        ("written_book_content", NbtTag::Compound(NbtCompound::new())),
        ("attribute_modifiers", NbtTag::Compound(NbtCompound::new())),
        ("trim", NbtTag::Compound(NbtCompound::new())),
        ("jukebox_playable", NbtTag::Compound(NbtCompound::new())),
        (
            "villager/variant",
            NbtTag::String("minecraft:plains".into()),
        ),
    ];

    let predicates = samples
        .into_iter()
        .map(|(path, tag)| {
            DataComponentPredicateData::from_persistent_entry(
                &Identifier::vanilla_static(path),
                &tag,
            )
            .unwrap_or_else(|| panic!("{path} sample should decode"))
        })
        .collect::<Vec<_>>();
    let matchers = DataComponentMatchers::new(DataComponentExactPredicate::EMPTY, predicates)
        .expect("builtin predicate keys are unique");

    let mut fields = NbtCompound::new();
    matchers.write_fields(&mut fields);
    assert_eq!(
        DataComponentMatchers::from_fields(&fields),
        Some(matchers.clone())
    );

    let mut encoded = Vec::new();
    matchers
        .write(&mut encoded)
        .expect("predicate matcher network codec should encode");
    assert_eq!(
        DataComponentMatchers::read(&mut Cursor::new(encoded.as_slice()))
            .expect("predicate matcher network codec should decode"),
        matchers
    );
}

#[test]
fn adventure_and_lock_components_round_trip_both_codecs() {
    init_test_registry();

    let block = BlockPredicate::new(
        Some(RegistryHolderSet::Direct(vec![&vanilla_blocks::STONE])),
        None,
        None,
        DataComponentMatchers::ANY,
    );
    let adventure =
        AdventureModePredicate::new(vec![block]).expect("one block predicate is persistable");
    round_trip_component(CAN_BREAK.key, ComponentData::new(adventure));

    let mut exact_components = DataComponentMap::new();
    exact_components.set(DAMAGE, Some(3));
    let damage_type = REGISTRY
        .data_component_predicate_types
        .by_key(&Identifier::vanilla_static("damage"))
        .expect("damage predicate type should exist");
    let partial = DataComponentPredicateData::new(
        damage_type,
        DamagePredicate::new(IntBounds::ANY, IntBounds::exactly(3)),
    );
    let matchers = DataComponentMatchers::new(
        DataComponentExactPredicate::all_of(&exact_components)
            .expect("exact components should persist"),
        vec![partial],
    )
    .expect("exact and partial maps use separate namespaces");
    let item = ItemPredicate::new(
        Some(RegistryHolderSet::Direct(vec![&vanilla_items::STONE])),
        IntBounds::exactly(1),
        matchers,
    );
    round_trip_component(LOCK.key, ComponentData::new(LockCode::new(item)));
}

#[test]
fn exact_predicates_reject_component_values_that_cannot_persist() {
    init_test_registry();
    let entry = REGISTRY
        .data_components
        .by_key(&OMINOUS_BOTTLE_AMPLIFIER.key)
        .expect("ominous bottle amplifier should be registered");
    let value = ComponentData::new(OminousBottleAmplifier::new(5));

    assert!(DataComponentExactPredicate::new(vec![(entry, value.clone())]).is_none());

    let mut network = Vec::new();
    write_len(1, &mut network).expect("one exact predicate should encode");
    write_registry_id(entry, &mut network, "data component")
        .expect("registered component ID should encode");
    entry
        .write_network(&value, &mut network)
        .expect("Vanilla's stream codec accepts amplifier 5");
    assert!(
        DataComponentExactPredicate::read(&mut Cursor::new(network.as_slice())).is_err(),
        "nested network values must not create a lock that fails persistent encoding"
    );
}

#[test]
fn boolean_predicate_fields_hash_as_codec_booleans() {
    let predicate = FireworkPredicate::new(None, Some(true), None);
    let mut key_hasher = ComponentHasher::new();
    "has_twinkle".hash_component(&mut key_hasher);
    let mut value_hasher = ComponentHasher::new();
    true.hash_component(&mut value_hasher);
    let mut entries = vec![HashEntry::new(key_hasher, value_hasher)];
    let mut expected = ComponentHasher::new();
    hash_entries(&mut expected, &mut entries);

    assert_eq!(predicate.compute_hash(), expected.finish());
    assert_ne!(
        predicate.compute_hash(),
        predicate.to_nbt_value().compute_hash(),
        "Codec.BOOL and an NBT byte intentionally have different HashOps tags"
    );
}

fn round_trip_component(key: Identifier, value: ComponentData) {
    let entry = REGISTRY
        .data_components
        .by_key(&key)
        .unwrap_or_else(|| panic!("missing component {key}"));
    let tag = entry
        .write_nbt(&value)
        .unwrap_or_else(|error| panic!("{key} persistent encode failed: {error}"));
    assert_eq!(entry.read_nbt_owned(&tag), Some(value.clone()));

    let mut encoded = Vec::new();
    entry
        .write_network(&value, &mut encoded)
        .unwrap_or_else(|error| panic!("{key} network encode failed: {error}"));
    assert_eq!(
        entry
            .read_network(&mut Cursor::new(encoded.as_slice()))
            .unwrap_or_else(|error| panic!("{key} network decode failed: {error}")),
        value
    );
    assert_eq!(
        entry
            .compute_hash(&value)
            .unwrap_or_else(|error| panic!("{key} hash failed: {error}")),
        value.downcast_ref::<AdventureModePredicate>().map_or_else(
            || {
                value
                    .downcast_ref::<LockCode>()
                    .expect("test only uses adventure and lock values")
                    .compute_hash()
            },
            HashComponent::compute_hash,
        )
    );
}
