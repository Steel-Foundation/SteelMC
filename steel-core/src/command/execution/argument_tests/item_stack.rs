use super::*;

#[test]
fn item_stack_argument_parses_supported_components_and_registered_removals() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[max_stack_size=16,enchantment_glint_override=true,!lore]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("supported item components should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert!(stack.is(&vanilla_items::STONE));
    assert_eq!(stack.max_stack_size(), 16);
    assert_eq!(
        stack.get(vanilla_components::ENCHANTMENT_GLINT_OVERRIDE),
        Some(&true)
    );
    assert!(matches!(
        stack.patch().get_entry(vanilla_components::LORE.key()),
        Some(ComponentPatchEntry::Removed)
    ));
}

#[test]
fn item_stack_argument_parses_identifier_components() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[item_model='stone',tooltip_style='steel:tooltip',note_block_sound='minecraft:block.note_block.harp']",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("identifier item components should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert_eq!(
        stack.get(vanilla_components::ITEM_MODEL),
        Some(&Identifier::vanilla_static("stone"))
    );
    assert_eq!(
        stack.get(vanilla_components::TOOLTIP_STYLE),
        Some(&Identifier::new_static("steel", "tooltip"))
    );
    assert_eq!(
        stack.get(vanilla_components::NOTE_BLOCK_SOUND),
        Some(&Identifier::vanilla_static("block.note_block.harp"))
    );
}

#[test]
fn item_stack_argument_parses_custom_data_codecs() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[custom_data={value:7,nested:{name:'steel'}},bucket_entity_data='{Health:4.0f}']",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("custom data components should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    let custom_data = stack
        .get(vanilla_components::CUSTOM_DATA)
        .expect("custom data should be retained");
    assert_eq!(custom_data.as_compound().int("value"), Some(7));
    assert_eq!(
        custom_data
            .as_compound()
            .compound("nested")
            .and_then(|nested| nested.string("name"))
            .map(|name| name.to_str()),
        Some("steel".into())
    );
    assert_eq!(
        stack
            .get(vanilla_components::BUCKET_ENTITY_DATA)
            .and_then(|data| data.as_compound().float("Health")),
        Some(4.0)
    );
}

#[test]
fn item_stack_argument_parses_custom_model_data_and_enchantability() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource golden_sword[custom_model_data={floats:[1.5f],flags:[1b,0b],strings:['steel'],colors:[[1f,0.5f,0f]]},enchantable={value:5}]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("custom model data and enchantability should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    let model_data = stack
        .get(vanilla_components::CUSTOM_MODEL_DATA)
        .expect("custom model data should be retained");
    assert_eq!(model_data.floats(), &[1.5]);
    assert_eq!(model_data.flags(), &[true, false]);
    assert_eq!(model_data.get_string(0), Some("steel"));
    assert_eq!(model_data.colors(), &[0xffff_7f00_u32 as i32]);
    assert_eq!(
        stack
            .get(vanilla_components::ENCHANTABLE)
            .map(|value| value.value()),
        Some(5)
    );
    assert!(stack.is_enchantable());
}

#[test]
fn item_stack_argument_parses_color_map_and_amplifier_components() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[dye='red',dyed_color=[1f,0.5f,0f],map_color=4603950,map_id=7,ominous_bottle_amplifier=4,base_color='blue',wolf/collar='green']",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("color, map, and amplifier components should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert_eq!(stack.get(vanilla_components::DYE), Some(&DyeColor::Red));
    assert_eq!(
        stack
            .get(vanilla_components::DYED_COLOR)
            .map(|color| color.rgb()),
        Some(0xffff_7f00_u32 as i32)
    );
    assert_eq!(
        stack
            .get(vanilla_components::MAP_COLOR)
            .map(|color| color.rgb()),
        Some(4_603_950)
    );
    assert_eq!(
        stack.get(vanilla_components::MAP_ID).map(|map| map.id()),
        Some(7)
    );
    assert_eq!(
        stack
            .get(vanilla_components::OMINOUS_BOTTLE_AMPLIFIER)
            .map(|amplifier| amplifier.value()),
        Some(4)
    );
    assert_eq!(
        stack.get(vanilla_components::BASE_COLOR),
        Some(&DyeColor::Blue)
    );
    assert_eq!(
        stack.get(vanilla_components::WOLF_COLLAR),
        Some(&DyeColor::Green)
    );
}

#[test]
fn item_stack_argument_parses_direct_entity_variant_components() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[fox/variant='snow',salmon/size='large',parrot/variant='gray',tropical_fish/pattern='clayfish',mooshroom/variant='brown',rabbit/variant='evil',horse/variant='dark_brown',llama/variant='gray',axolotl/variant='blue']",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("direct entity variant components should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert_eq!(
        stack.get(vanilla_components::FOX_VARIANT),
        Some(&FoxVariant::Snow)
    );
    assert_eq!(
        stack.get(vanilla_components::SALMON_SIZE),
        Some(&SalmonVariant::Large)
    );
    assert_eq!(
        stack.get(vanilla_components::PARROT_VARIANT),
        Some(&ParrotVariant::Gray)
    );
    assert_eq!(
        stack.get(vanilla_components::TROPICAL_FISH_PATTERN),
        Some(&TropicalFishPattern::Clayfish)
    );
    assert_eq!(
        stack.get(vanilla_components::MOOSHROOM_VARIANT),
        Some(&MooshroomVariant::Brown)
    );
    assert_eq!(
        stack.get(vanilla_components::RABBIT_VARIANT),
        Some(&RabbitVariant::Evil)
    );
    assert_eq!(
        stack.get(vanilla_components::HORSE_VARIANT),
        Some(&HorseVariant::DarkBrown)
    );
    assert_eq!(
        stack.get(vanilla_components::LLAMA_VARIANT),
        Some(&LlamaVariant::Gray)
    );
    assert_eq!(
        stack.get(vanilla_components::AXOLOTL_VARIANT),
        Some(&AxolotlVariant::Blue)
    );
}

#[test]
fn item_stack_argument_parses_registry_holder_set_components() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[damage_resistant={types:'#minecraft:is_fire'},repairable={items:'minecraft:phantom_membrane'}]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("registry holder-set components should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert!(!stack.can_be_hurt_by(&vanilla_damage_types::IN_FIRE));
    assert!(stack.can_be_hurt_by(&vanilla_damage_types::GENERIC));
    assert!(stack.is_valid_repair_item(&ItemStack::new(&vanilla_items::PHANTOM_MEMBRANE)));
    assert!(!stack.is_valid_repair_item(&ItemStack::new(&vanilla_items::BREEZE_ROD)));
}

#[test]
fn item_stack_argument_uses_vanilla_numeric_codec_coercions() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[max_stack_size=16.9d,enchantment_glint_override=2,potion_duration_scale=1]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("vanilla numeric component coercions should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert_eq!(stack.max_stack_size(), 16);
    assert_eq!(
        stack.get(vanilla_components::ENCHANTMENT_GLINT_OVERRIDE),
        Some(&true)
    );
    assert_eq!(
        stack.get(vanilla_components::POTION_DURATION_SCALE),
        Some(&1.0)
    );
}

#[test]
fn item_stack_argument_parses_compound_component_values() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[use_cooldown={seconds:5.5,cooldown_group:'minecraft:test'},lore=[],max_stack_size=16]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("supported compound component should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };
    let Some(cooldown) = stack.get(vanilla_components::USE_COOLDOWN) else {
        panic!("use cooldown should be retained");
    };

    assert_eq!(cooldown.seconds.to_bits(), 5.5_f32.to_bits());
    assert_eq!(
        cooldown.cooldown_group,
        Some(Identifier::vanilla_static("test"))
    );
    assert!(
        stack
            .get(vanilla_components::LORE)
            .is_some_and(|lore| lore.lines().is_empty())
    );
    assert_eq!(stack.max_stack_size(), 16);
}

#[test]
fn item_stack_argument_rejects_unsupported_transient_and_invalid_components() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());

    for input in [
        "resource stone[creative_slot_lock={}]",
        "resource stone[additional_trade_cost={}]",
        "resource stone[map_post_processing={}]",
        "resource stone[missing={}]",
        "resource stone[max_stack_size=16,max_stack_size=8]",
        "resource stone[max_stack_size=0]",
        "resource stone[max_damage=10]",
        "resource stone[potion_duration_scale=-0.0f]",
        "resource stone[enchantable={value:0}]",
        "resource stone[custom_model_data={strings:[1]}]",
        "resource stone[dye='not_a_color']",
        "resource stone[dyed_color=[1f,0f]]",
        "resource stone[ominous_bottle_amplifier=5]",
        "resource stone[fox/variant='not_a_variant']",
        "resource stone[damage_resistant={types:'#minecraft:missing'}]",
        "resource stone[repairable={items:'minecraft:missing'}]",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should be rejected"
        );
    }
}

#[test]
fn item_stack_argument_rejects_invalid_recursive_contents() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        r"resource stone[bundle_contents=[{id:'minecraft:stone',count:2,components:{'minecraft:max_stack_size':1}}]]",
        TestSource::new(),
    );

    assert!(dispatcher.context_chain(parse).is_err());
}

#[test]
fn item_stack_argument_sanitizes_redundant_component_changes() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[max_stack_size=64,!custom_data]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("redundant component changes should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert!(stack.components_patch().is_empty());
}

#[test]
fn item_arguments_propagate_translatable_snbt_errors() {
    init_vanilla_registry();
    for argument in [
        SteelArgumentType::item_stack(),
        SteelArgumentType::item_predicate(),
    ] {
        let dispatcher = resource_dispatcher(argument);
        let parse = dispatcher.parse("resource stone[max_stack_size=]", TestSource::new());
        let Err(error) = dispatcher.context_chain(parse) else {
            panic!("missing component value should be rejected");
        };

        let CommandSyntaxErrorKind::Dynamic(component) = error.kind() else {
            panic!("component failure should be a dynamic command error");
        };
        assert!(matches!(
            &component.content,
            Content::Translate(message)
                if message.key == "snbt.parser.expected_unquoted_string"
        ));
    }
}

#[test]
fn removing_max_stack_size_uses_vanillas_fallback_of_one() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse("resource stone[!max_stack_size]", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("registered component removal should parse");
    };
    let Ok(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert_eq!(stack.max_stack_size(), 1);
}

#[test]
fn item_stack_argument_suggests_items_and_supported_component_operations() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());

    let parse = dispatcher.parse("resource minecraft:diamond_sw", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("item suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "minecraft:diamond_sword")
    );

    let parse = dispatcher.parse("resource stone[dam", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("component suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "stone[minecraft:damage=")
    );

    let parse = dispatcher.parse("resource stone[!lo", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("component removal suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "stone[!minecraft:lore")
    );

    let parse = dispatcher.parse("resource stone[  dam", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("component suggestions after whitespace should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "stone[  minecraft:damage=")
    );

    let parse = dispatcher.parse("resource stone[!lore", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("component removal delimiter suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "stone[!lore,")
    );
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "stone[!lore]")
    );

    let input = "resource stone[use_cooldown={seconds:1.0f,cooldown_group:'minecraft:test'},wea";
    let parse = dispatcher.parse(input, TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("component suggestions after compound values should build");
    };
    assert!(suggestions.list().iter().any(|suggestion| {
        suggestion.text()
            == "stone[use_cooldown={seconds:1.0f,cooldown_group:'minecraft:test'},minecraft:weapon="
    }));
}
