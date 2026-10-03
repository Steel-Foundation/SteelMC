use super::patch_persistence::hash_entry;
use super::*;
use crate::{
    REGISTRY, RegistryExt as _,
    data_components::CustomData,
    data_components::vanilla_components::{
        ADDITIONAL_TRADE_COST, BREAK_SOUND, BUCKET_ENTITY_DATA, CHICKEN_VARIANT,
        CREATIVE_SLOT_LOCK, CUSTOM_NAME, DYE, ENCHANTABLE, ENCHANTMENT_GLINT_OVERRIDE, ITEM_MODEL,
        ITEM_NAME, LORE, MAP_COLOR, MAP_POST_PROCESSING, MAX_STACK_SIZE, OMINOUS_BOTTLE_AMPLIFIER,
        POTION_DURATION_SCALE, RARITY, STORED_ENCHANTMENTS, SWING_ANIMATION, SwingAnimationType,
        TOOLTIP_DISPLAY, USE_EFFECTS,
    },
    init_vanilla_registry,
    item_stack::ItemStack,
    sound_events, vanilla_chicken_variants, vanilla_items,
};
use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
use steel_utils::Identifier;
use text_components::content::Content;

fn with_borrowed_tag<R>(tag: OwnedNbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
    let mut bytes = Vec::new();
    tag.write(&mut bytes);
    let borrowed =
        read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
    visitor(borrowed.as_tag())
}

fn parse_patch(tag: OwnedNbtTag) -> Option<DataComponentPatch> {
    with_borrowed_tag(tag, DataComponentPatch::from_nbt_tag)
}

#[test]
fn duplicate_component_registration_is_rejected_without_mutation() {
    let mut registry = DataComponentRegistry::new();
    let key = Identifier::new("test".to_owned(), "duplicate".to_owned());
    let original = DataComponentType::<i32>::new(key.clone());
    registry.register(original.clone());

    let duplicate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        registry.register(DataComponentType::<bool>::new(key.clone()));
    }));

    assert!(duplicate.is_err());
    assert_eq!(registry.len(), 1);
    assert_eq!(registry.get_id(original), Some(0));
    assert_eq!(registry.get_key_by_id(0), Some(&key));
    assert_eq!(registry.by_key(&key).map(|entry| &entry.key), Some(&key));

    let second = DataComponentType::<i32>::new(Identifier::new(
        "test".to_owned(),
        "same_type_different_key".to_owned(),
    ));
    registry.register(second.clone());
    assert_eq!(registry.get_id(second), Some(1));
}

#[test]
fn persistent_hash_rejects_values_rejected_by_the_persistent_codec() {
    let mut registry = DataComponentRegistry::new();
    super::super::vanilla_components::register_vanilla_data_components(&mut registry);

    let invalid_values = [
        (MAX_STACK_SIZE.key().clone(), ComponentData::new(0_i32)),
        (
            super::super::vanilla_components::MAX_DAMAGE.key().clone(),
            ComponentData::new(0_i32),
        ),
        (
            super::super::vanilla_components::MINIMUM_ATTACK_CHARGE
                .key()
                .clone(),
            ComponentData::new(1.5_f32),
        ),
        (
            POTION_DURATION_SCALE.key().clone(),
            ComponentData::new(-0.5_f32),
        ),
    ];
    for (key, value) in invalid_values {
        let entry = registry
            .by_key(&key)
            .expect("component should be registered");
        assert!(entry.compute_hash(&value).is_err(), "{key}");
    }

    let max_stack_size = registry
        .by_key(MAX_STACK_SIZE.key())
        .expect("max_stack_size should be registered");
    assert_eq!(
        max_stack_size
            .compute_hash(&ComponentData::new(99_i32))
            .expect("boundary value should hash"),
        99_i32.compute_hash()
    );
}

#[test]
fn persistent_patch_nbt_omits_transient_components() {
    init_vanilla_registry();
    let mut patch = DataComponentPatch::new();
    patch.set(MAX_STACK_SIZE, 16);
    patch.set(CREATIVE_SLOT_LOCK, ());
    patch.remove(ADDITIONAL_TRADE_COST);
    patch.remove(MAP_POST_PROCESSING);

    let OwnedNbtTag::Compound(compound) = patch.to_nbt_tag_ref() else {
        panic!("component patch should serialize as a compound");
    };
    assert!(compound.get("minecraft:max_stack_size").is_some());
    assert!(compound.get("minecraft:creative_slot_lock").is_none());
    assert!(compound.get("!minecraft:additional_trade_cost").is_none());
    assert!(compound.get("minecraft:map_post_processing").is_none());
}

#[test]
fn persistent_patch_hash_uses_each_component_codec_hash() {
    init_vanilla_registry();
    let mut patch = DataComponentPatch::new();
    patch.set(ENCHANTMENT_GLINT_OVERRIDE, true);

    let entry = hash_entry(
        ENCHANTMENT_GLINT_OVERRIDE.key.to_string().compute_hash(),
        true.compute_hash(),
    );
    let mut expected = ComponentHasher::new();
    expected.start_map();
    expected.put_raw_bytes(&entry.key_bytes);
    expected.put_raw_bytes(&entry.value_bytes);
    expected.end_map();
    assert_eq!(
        patch
            .compute_persistent_hash()
            .expect("valid patch should hash"),
        expected.finish()
    );

    // NbtOps stores Codec.BOOL as a byte while HashOps preserves a boolean.
    assert_ne!(
        patch
            .compute_persistent_hash()
            .expect("valid patch should hash"),
        patch.to_nbt_tag_ref().compute_hash()
    );
}

#[test]
fn persistent_patch_decode_fails_on_invalid_entries() {
    init_vanilla_registry();

    let mut valid = NbtCompound::new();
    valid.insert("minecraft:max_stack_size", OwnedNbtTag::Double(16.9));
    let patch = parse_patch(OwnedNbtTag::Compound(valid))
        .expect("numeric component value should use codec coercion");
    assert_eq!(
        patch.get_entry(&MAX_STACK_SIZE.key),
        Some(&ComponentPatchEntry::Set(ComponentData::new(16_i32)))
    );

    let mut out_of_range = NbtCompound::new();
    out_of_range.insert("minecraft:max_stack_size", 0);
    assert!(parse_patch(OwnedNbtTag::Compound(out_of_range)).is_none());

    let mut unknown = NbtCompound::new();
    unknown.insert("minecraft:not_a_component", NbtCompound::new());
    assert!(parse_patch(OwnedNbtTag::Compound(unknown)).is_none());

    let mut malformed_removal = NbtCompound::new();
    malformed_removal.insert("!minecraft:max_stack_size", 1);
    assert!(parse_patch(OwnedNbtTag::Compound(malformed_removal)).is_none());
}

#[test]
fn text_component_persistent_codec_collapses_plain_text() {
    init_vanilla_registry();
    let entry = REGISTRY
        .data_components
        .by_key(&CUSTOM_NAME.key)
        .expect("custom_name should be registered");
    let value = ComponentData::new(text_components::TextComponent::plain("name"));

    assert_eq!(
        entry
            .write_nbt(&value)
            .expect("plain custom name should encode"),
        OwnedNbtTag::String("name".into())
    );
}

#[test]
fn common_defaults_and_extracted_item_overrides_match_vanilla() {
    init_vanilla_registry();

    let common = DataComponentMap::common_item_components();
    assert_eq!(common.len(), 10);
    assert_eq!(common.get_ref(LORE), Some(&ItemLore::empty()));
    assert_eq!(common.get_ref(USE_EFFECTS), Some(&UseEffects::DEFAULT));
    assert_eq!(common.get_ref(RARITY), Some(&Rarity::Common));
    assert_eq!(
        common
            .get_ref(BREAK_SOUND)
            .and_then(SoundEventHolder::registry_ref),
        Some(&sound_events::ENTITY_ITEM_BREAK)
    );
    assert_eq!(
        common.get_ref(TOOLTIP_DISPLAY),
        Some(&TooltipDisplay::DEFAULT)
    );
    assert_eq!(
        common.get_ref(SWING_ANIMATION),
        Some(&SwingAnimation::DEFAULT)
    );

    let wooden_spear = ItemStack::new(&vanilla_items::WOODEN_SPEAR);
    assert_eq!(
        wooden_spear.get(USE_EFFECTS),
        Some(&UseEffects::new(true, false, 1.0))
    );
    assert_eq!(
        wooden_spear.get(SWING_ANIMATION),
        Some(&SwingAnimation::new(SwingAnimationType::Stab, 13))
    );

    let heavy_core = ItemStack::new(&vanilla_items::HEAVY_CORE);
    assert_eq!(heavy_core.get(RARITY), Some(&Rarity::Epic));

    let stone = ItemStack::new(&vanilla_items::STONE);
    assert_eq!(
        stone.get(ITEM_MODEL),
        Some(&Identifier::vanilla_static("stone"))
    );
    let Some(Content::Translate(stone_name)) = stone.get(ITEM_NAME).map(|name| &name.content)
    else {
        panic!("stone should have a translated item name");
    };
    assert_eq!(stone_name.key, "block.minecraft.stone");

    let redstone = ItemStack::new(&vanilla_items::REDSTONE);
    assert_eq!(
        redstone.get(ITEM_MODEL),
        Some(&Identifier::vanilla_static("redstone"))
    );
    let Some(Content::Translate(redstone_name)) = redstone.get(ITEM_NAME).map(|name| &name.content)
    else {
        panic!("redstone should have a translated item name");
    };
    assert_eq!(redstone_name.key, "item.minecraft.redstone");

    let shield = ItemStack::new(&vanilla_items::SHIELD);
    assert_eq!(
        shield
            .get(BREAK_SOUND)
            .and_then(SoundEventHolder::registry_ref),
        Some(&sound_events::ITEM_SHIELD_BREAK)
    );

    let pufferfish_bucket = ItemStack::new(&vanilla_items::PUFFERFISH_BUCKET);
    assert!(
        pufferfish_bucket
            .get(BUCKET_ENTITY_DATA)
            .is_some_and(CustomData::is_empty)
    );

    let golden_sword = ItemStack::new(&vanilla_items::GOLDEN_SWORD);
    assert_eq!(
        golden_sword.get(ENCHANTABLE).map(|value| value.value()),
        Some(22)
    );
    assert!(golden_sword.is_enchantable());
    assert!(!ItemStack::new(&vanilla_items::STONE).is_enchantable());

    for (item, variant) in [
        (&vanilla_items::EGG, &vanilla_chicken_variants::TEMPERATE),
        (&vanilla_items::BLUE_EGG, &vanilla_chicken_variants::COLD),
        (&vanilla_items::BROWN_EGG, &vanilla_chicken_variants::WARM),
    ] {
        assert!(
            ItemStack::new(item)
                .get(CHICKEN_VARIANT)
                .is_some_and(|reference| reference.value().key == variant.key),
            "{}",
            item.key
        );
    }

    assert_eq!(
        ItemStack::new(&vanilla_items::TIPPED_ARROW).get(POTION_DURATION_SCALE),
        Some(&0.125)
    );
    assert_eq!(
        ItemStack::new(&vanilla_items::LINGERING_POTION).get(POTION_DURATION_SCALE),
        Some(&0.25)
    );
    assert!(
        ItemStack::new(&vanilla_items::ENCHANTED_BOOK)
            .get(STORED_ENCHANTMENTS)
            .is_some_and(ItemEnchantments::is_empty)
    );

    let music_disc_cat = ItemStack::new(&vanilla_items::MUSIC_DISC_CAT);
    assert_eq!(
        music_disc_cat
            .get(crate::data_components::vanilla_components::JUKEBOX_PLAYABLE)
            .and_then(|playable| playable.song().as_reference()),
        Some(&crate::vanilla_jukebox_songs::CAT)
    );

    for (item, color) in [
        (&vanilla_items::WHITE_DYE, crate::DyeColor::White),
        (&vanilla_items::ORANGE_DYE, crate::DyeColor::Orange),
        (&vanilla_items::MAGENTA_DYE, crate::DyeColor::Magenta),
        (&vanilla_items::LIGHT_BLUE_DYE, crate::DyeColor::LightBlue),
        (&vanilla_items::YELLOW_DYE, crate::DyeColor::Yellow),
        (&vanilla_items::LIME_DYE, crate::DyeColor::Lime),
        (&vanilla_items::PINK_DYE, crate::DyeColor::Pink),
        (&vanilla_items::GRAY_DYE, crate::DyeColor::Gray),
        (&vanilla_items::LIGHT_GRAY_DYE, crate::DyeColor::LightGray),
        (&vanilla_items::CYAN_DYE, crate::DyeColor::Cyan),
        (&vanilla_items::PURPLE_DYE, crate::DyeColor::Purple),
        (&vanilla_items::BLUE_DYE, crate::DyeColor::Blue),
        (&vanilla_items::BROWN_DYE, crate::DyeColor::Brown),
        (&vanilla_items::GREEN_DYE, crate::DyeColor::Green),
        (&vanilla_items::RED_DYE, crate::DyeColor::Red),
        (&vanilla_items::BLACK_DYE, crate::DyeColor::Black),
    ] {
        assert_eq!(ItemStack::new(item).get(DYE), Some(&color), "{}", item.key);
    }

    assert_eq!(
        ItemStack::new(&vanilla_items::FILLED_MAP)
            .get(MAP_COLOR)
            .map(|color| color.rgb()),
        Some(4_603_950)
    );
    assert_eq!(
        ItemStack::new(&vanilla_items::OMINOUS_BOTTLE)
            .get(OMINOUS_BOTTLE_AMPLIFIER)
            .map(|amplifier| amplifier.value()),
        Some(0)
    );
}
