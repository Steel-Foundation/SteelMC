use super::*;

#[test]
fn item_predicate_argument_matches_targets_boolean_terms_and_count_ranges() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse(
        "resource #logs[count={min:2,max:3},!damage|enchantment_glint_override]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("vanilla item predicate grammar should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };

    assert!(predicate.matches(&ItemStack::with_count(&vanilla_items::OAK_LOG, 3)));
    assert!(!predicate.matches(&ItemStack::with_count(&vanilla_items::OAK_LOG, 4)));
    assert!(!predicate.matches(&ItemStack::with_count(&vanilla_items::STONE, 3)));
}

#[test]
fn item_predicate_argument_decodes_exact_components_before_matching() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse("resource stone[max_stack_size=64b]", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("numeric component value should use the registered codec");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };

    assert!(predicate.matches(&ItemStack::new(&vanilla_items::STONE)));
}

#[test]
fn item_predicate_argument_supports_damage_and_enchantment_predicates() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let input = "resource diamond_sword[damage~{damage:7,durability:{min:1}},enchantments~[{enchantments:'minecraft:sharpness',levels:{min:2}}]]";
    let parse = dispatcher.parse(input, TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("implemented data component predicates should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };
    let mut sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    sword.set_damage_value(7);
    sword.set_enchantments(&[(Identifier::vanilla_static("sharpness"), 3)], false);

    assert!(predicate.matches(&sword));
    sword.set_damage_value(6);
    assert!(!predicate.matches(&sword));
}

#[test]
fn item_predicate_argument_supports_partial_custom_data_matching() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse(
        "resource stone[custom_data~{nested:{value:2}}]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("custom data predicate should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };

    let mut nested = NbtCompound::new();
    nested.insert("value", 2);
    nested.insert("extra", 3);
    let mut compound = NbtCompound::new();
    compound.insert("nested", nested);
    let mut stack = ItemStack::new(&vanilla_items::STONE);
    stack.set(
        vanilla_components::CUSTOM_DATA,
        vanilla_components::CustomData::try_from_compound(compound)
            .expect("test custom data should be valid"),
    );

    assert!(predicate.matches(&stack));
    stack.remove(vanilla_components::CUSTOM_DATA);
    assert!(!predicate.matches(&stack));
}

#[test]
fn item_predicate_argument_decodes_every_registered_vanilla_partial_predicate() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    for expression in [
        "potion_contents~'minecraft:water'",
        "container~{}",
        "bundle_contents~{}",
        "firework_explosion~{}",
        "fireworks~{}",
        "writable_book_content~{}",
        "written_book_content~{}",
        "trim~{}",
        "jukebox_playable~{}",
        "villager/variant~'minecraft:plains'",
    ] {
        let input = format!("resource stone[{expression}]");
        let parse = dispatcher.parse(&input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("registered data component predicate should parse: {expression}");
        };
        assert!(chain.top_context().item_predicate("value").is_some());
    }
}

#[test]
fn item_predicate_argument_matches_registered_firework_explosion_predicate() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse(
        "resource firework_star[firework_explosion~{shape:'star',has_twinkle:true}]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("firework explosion predicate should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };
    let mut stack = ItemStack::new(&vanilla_items::FIREWORK_STAR);
    stack.set(
        vanilla_components::FIREWORK_EXPLOSION,
        vanilla_components::FireworkExplosion::new(
            vanilla_components::FireworkExplosionShape::Star,
            Vec::new(),
            Vec::new(),
            false,
            true,
        ),
    );

    assert!(predicate.matches(&stack));
    stack.remove(vanilla_components::FIREWORK_EXPLOSION);
    assert!(!predicate.matches(&stack));
}

#[test]
fn item_predicate_argument_matches_stream_only_nested_template_without_materializing_a_stack() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse(
        "resource chest[container~{items:{contains:[{items:'minecraft:stick',count:100}]}}]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("nested container predicate should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };

    let mut encoded = Vec::new();
    VarInt(i32::try_from(vanilla_items::STICK.id()).expect("test item id should fit"))
        .write(&mut encoded)
        .expect("item id should encode");
    VarInt(100)
        .write(&mut encoded)
        .expect("count should encode");
    VarInt(0)
        .write(&mut encoded)
        .expect("set component count should encode");
    VarInt(0)
        .write(&mut encoded)
        .expect("removed component count should encode");
    let template = ItemStackTemplate::read(&mut Cursor::new(encoded.as_slice()))
        .expect("count 100 is valid in the stream codec");

    let mut stack = ItemStack::new(&vanilla_items::CHEST);
    stack.set(
        vanilla_components::CONTAINER,
        vanilla_components::ItemContainerContents::new(vec![Some(template)])
            .expect("one container slot should be valid"),
    );
    assert!(predicate.matches(&stack));
}

#[test]
fn item_predicate_argument_supports_attribute_modifier_collection_predicates() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let input = "resource stone[attribute_modifiers~{modifiers:{contains:[{attribute:'minecraft:attack_damage',id:'minecraft:test',amount:{min:2.5,max:3.5},operation:'add_value',slot:'mainhand'}],count:[{test:{attribute:'minecraft:attack_damage'},count:1}],size:1}}]";
    let parse = dispatcher.parse(input, TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("attribute modifier collection predicate should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };
    let mut stack = ItemStack::new(&vanilla_items::STONE);
    stack.set(
        vanilla_components::ATTRIBUTE_MODIFIERS,
        vanilla_components::ItemAttributeModifiers {
            modifiers: vec![vanilla_components::ItemAttributeModifierEntry {
                attribute: vanilla_attributes::ATTACK_DAMAGE,
                id: Identifier::vanilla_static("test"),
                amount: 3.0,
                operation: vanilla_components::AttributeModifierOperation::AddValue,
                slot: vanilla_components::EquipmentSlotGroup::MainHand,
                display: vanilla_components::ItemAttributeModifierDisplay::Default,
            }],
        },
    );

    assert!(predicate.matches(&stack));
    stack.set(
        vanilla_components::ATTRIBUTE_MODIFIERS,
        vanilla_components::ItemAttributeModifiers::empty(),
    );
    assert!(!predicate.matches(&stack));
}

#[test]
fn item_predicate_argument_rejects_noncanonical_holder_and_slot_values() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());

    for input in [
        "resource diamond_sword[enchantments~[{enchantments:['#minecraft:curse']}]]",
        "resource stone[attribute_modifiers~{modifiers:{contains:[{slot:'main_hand'}]}}]",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject a value outside the vanilla codec"
        );
    }
}

#[test]
fn item_predicate_argument_uses_map_codec_for_component_existence_predicates() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());

    for path in [
        "creative_slot_lock",
        "additional_trade_cost",
        "map_post_processing",
    ] {
        let input = format!("resource stone[{path}~{{ignored:1}}]");
        let parse = dispatcher.parse(&input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("transient component existence predicate should accept a compound");
        };
        let Some(predicate) = chain.top_context().item_predicate("value") else {
            panic!("item predicate should be retained");
        };
        let mut stack = ItemStack::new(&vanilla_items::STONE);
        if path == "creative_slot_lock" {
            stack.set(vanilla_components::CREATIVE_SLOT_LOCK, ());
        }

        assert_eq!(predicate.matches(&stack), path == "creative_slot_lock");
    }
}

#[test]
fn item_predicate_argument_rejects_malformed_potion_predicate() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse(
        "resource potion[potion_contents~{potion:'minecraft:water'}]",
        TestSource::new(),
    );

    assert!(dispatcher.context_chain(parse).is_err());
}

#[test]
fn item_predicate_argument_rejects_transient_components_as_component_tests() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());

    for input in [
        "resource stone[creative_slot_lock]",
        "resource stone[additional_trade_cost]",
        "resource stone[map_post_processing]",
        "resource stone[creative_slot_lock={}]",
        "resource stone[additional_trade_cost={}]",
        "resource stone[map_post_processing={}]",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject a transient component test"
        );
    }
}

#[test]
fn item_predicate_argument_suggests_items_tags_and_condition_types() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());

    for (input, expected) in [
        ("resource minecraft:diamond_sw", "minecraft:diamond_sword"),
        ("resource #log", "#minecraft:logs"),
        ("resource stone[co", "stone[minecraft:count"),
        (
            "resource stone[villager",
            "stone[minecraft:villager/variant",
        ),
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
            panic!("item predicate suggestions should build");
        };
        assert!(
            suggestions
                .list()
                .iter()
                .any(|suggestion| suggestion.text() == expected),
            "{input} should suggest {expected}"
        );
    }
}
