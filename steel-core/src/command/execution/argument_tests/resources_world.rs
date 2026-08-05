use super::*;

#[test]
fn biome_or_tag_argument_resolves_registry_entries_and_tags() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::biome_or_tag());

    let parse = dispatcher.parse("resource plains", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("registered biome should parse");
    };
    assert!(matches!(
        chain.top_context().biome_or_tag("value"),
        Some(BiomeOrTag::Biome(biome)) if *biome == &*vanilla_biomes::PLAINS
    ));

    let parse = dispatcher.parse("resource #is_overworld", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("registered biome tag should parse");
    };
    let Some(tag) = chain.top_context().biome_or_tag("value") else {
        panic!("biome tag should be retained");
    };
    assert!(tag.matches(&vanilla_biomes::PLAINS));
    assert!(!tag.matches(&vanilla_biomes::NETHER_WASTES));

    for input in ["resource missing", "resource #missing"] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject an unknown biome or tag"
        );
    }

    let parse = dispatcher.parse("resource #is_o", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("biome tag suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "#minecraft:is_overworld")
    );
}

#[test]
fn structure_or_tag_key_argument_defers_registry_resolution_until_execution() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::structure_or_tag_key());

    let parse = dispatcher.parse("resource village_plains", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("structure keys should parse");
    };
    let Some(structure) = chain.top_context().structure_or_tag_key("value") else {
        panic!("structure key should be retained");
    };
    assert!(matches!(
        structure,
        StructureOrTagKey::Structure(key)
            if *key == Identifier::vanilla_static("village_plains")
    ));
    let Some(structures) = structure.resolve() else {
        panic!("registered structure should resolve");
    };
    assert_eq!(structures.len(), 1);
    assert_eq!(
        structures[0].key,
        Identifier::vanilla_static("village_plains")
    );

    let parse = dispatcher.parse("resource #village", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("structure tag keys should parse");
    };
    let Some(tag) = chain.top_context().structure_or_tag_key("value") else {
        panic!("structure tag key should be retained");
    };
    assert!(matches!(
        tag,
        StructureOrTagKey::Tag(key) if *key == Identifier::vanilla_static("village")
    ));
    let Some(structures) = tag.resolve() else {
        panic!("registered structure tag should resolve");
    };
    assert!(
        structures
            .iter()
            .any(|structure| structure.key == Identifier::vanilla_static("village_plains"))
    );
    assert!(
        structures
            .iter()
            .any(|structure| structure.key == Identifier::vanilla_static("village_desert"))
    );

    for input in ["resource missing", "resource #missing"] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("{input} should retain an unresolved key");
        };
        let Some(key) = chain.top_context().structure_or_tag_key("value") else {
            panic!("unresolved structure key should be retained");
        };
        assert!(key.resolve().is_none());
    }

    let parse = dispatcher.parse("resource #villa", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("structure tag suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "#minecraft:village")
    );
}

#[test]
fn heightmap_argument_accepts_vanilla_live_world_names_and_suggests_them() {
    let dispatcher = resource_dispatcher(SteelArgumentType::heightmap());
    let parse = dispatcher.parse("resource MOTION_BLOCKING_NO_LEAVES", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("heightmap names should parse case-insensitively");
    };
    assert_eq!(
        chain.top_context().heightmap("value"),
        Some(HeightmapType::MotionBlockingNoLeaves)
    );

    let parse = dispatcher.parse("resource motion", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("heightmap suggestions should build");
    };
    assert_eq!(
        suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        ["motion_blocking", "motion_blocking_no_leaves"]
    );

    let parse = dispatcher.parse("resource ", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("all kept heightmaps should be suggested");
    };
    assert_eq!(
        suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        [
            "motion_blocking",
            "motion_blocking_no_leaves",
            "ocean_floor",
            "world_surface"
        ]
    );

    let parse = dispatcher.parse("resource world_surface_wg", TestSource::new());
    assert!(dispatcher.context_chain(parse).is_err());
}

#[test]
fn domain_argument_resolves_and_suggests_only_configured_domains() {
    let dispatcher = resource_dispatcher(SteelArgumentType::domain());

    let parse = dispatcher.parse("resource alpha", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("configured domain should parse");
    };
    assert_eq!(chain.top_context().domain("value"), Some("alpha"));

    let parse = dispatcher.parse("resource gamma", TestSource::new());
    assert!(dispatcher.context_chain(parse).is_err());

    let parse = dispatcher.parse("resource b", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("domain suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["beta"]);
}

#[test]
fn world_argument_retains_relative_and_fully_qualified_names() {
    let dispatcher = resource_dispatcher(SteelArgumentType::world());

    let parse = dispatcher.parse("resource overworld", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("relative world should parse");
    };
    assert_eq!(
        chain.top_context().world_argument("value"),
        Some(&WorldArgument::Relative("overworld".into()))
    );

    let parse = dispatcher.parse("resource beta:lobby", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("fully qualified world should parse");
    };
    assert_eq!(
        chain.top_context().world_argument("value"),
        Some(&WorldArgument::Key(Identifier::new_static("beta", "lobby")))
    );

    let parse = dispatcher.parse("resource a", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("world suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["alpha:arena", "alpha:overworld", "arena"]);
}

#[test]
fn storage_key_argument_parses_and_suggests_source_domain_keys() {
    let dispatcher = resource_dispatcher(SteelArgumentType::storage_key());
    let parse = dispatcher.parse("resource steel:data", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("storage key should parse");
    };
    assert_eq!(
        chain.top_context().identifier("value"),
        Some(&Identifier::from_steel("data"))
    );

    let parse = dispatcher.parse("resource st", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("storage key suggestions should build");
    };
    assert_eq!(
        suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        ["steel:data"]
    );
}

#[test]
fn game_mode_argument_parses_only_vanilla_names() {
    let dispatcher = resource_dispatcher(SteelArgumentType::game_mode());

    for (name, expected) in [
        ("survival", GameType::Survival),
        ("creative", GameType::Creative),
        ("adventure", GameType::Adventure),
        ("spectator", GameType::Spectator),
    ] {
        let input = format!("resource {name}");
        let parse = dispatcher.parse(&input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("vanilla game mode name should parse");
        };
        assert_eq!(chain.top_context().game_mode("value"), Some(expected));
    }

    for invalid in ["0", "Creative", "missing"] {
        let input = format!("resource {invalid}");
        let parse = dispatcher.parse(&input, TestSource::new());
        assert!(dispatcher.context_chain(parse).is_err());
    }
}

#[test]
fn game_mode_argument_suggests_vanilla_names() {
    let dispatcher = resource_dispatcher(SteelArgumentType::game_mode());
    let parse = dispatcher.parse("resource s", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("game mode suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();

    assert_eq!(suggestions, ["spectator", "survival"]);
}

#[test]
fn entity_anchor_argument_parses_and_suggests_vanilla_names() {
    let dispatcher = resource_dispatcher(SteelArgumentType::entity_anchor());
    for (name, expected) in [("feet", EntityAnchor::Feet), ("eyes", EntityAnchor::Eyes)] {
        let input = format!("resource {name}");
        let parse = dispatcher.parse(&input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("vanilla entity anchor should parse");
        };
        assert_eq!(chain.top_context().entity_anchor("value"), Some(expected));
    }

    let parse = dispatcher.parse("resource missing", TestSource::new());
    assert!(dispatcher.context_chain(parse).is_err());

    let parse = dispatcher.parse("resource e", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("entity anchor suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["eyes"]);
}

#[test]
fn summonable_entity_argument_resolves_only_registered_factories() {
    init_test_core();
    let dispatcher = resource_dispatcher(SteelArgumentType::summonable_entity());

    for input in ["resource pig", "resource minecraft:pig"] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("registered summonable entity should parse");
        };
        assert_eq!(
            chain.top_context().entity_type("value"),
            Some(&vanilla_entities::PIG)
        );
    }

    for input in ["resource player", "resource minecraft:missing"] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(dispatcher.context_chain(parse).is_err());
    }
}

#[test]
fn summonable_entity_argument_suggests_only_registered_factories() {
    init_test_core();
    let dispatcher = resource_dispatcher(SteelArgumentType::summonable_entity());
    let parse = dispatcher.parse("resource minecraft:pi", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("summonable entity suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();

    assert_eq!(suggestions, ["minecraft:pig"]);
}

#[test]
fn enchantment_argument_resolves_and_suggests_registered_entries() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::enchantment());

    for input in ["resource sharpness", "resource minecraft:sharpness"] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("registered enchantment should parse");
        };
        assert_eq!(
            chain.top_context().enchantment("value"),
            Some(&vanilla_enchantments::SHARPNESS)
        );
    }

    let parse = dispatcher.parse("resource minecraft:missing", TestSource::new());
    assert!(dispatcher.context_chain(parse).is_err());

    let parse = dispatcher.parse("resource minecraft:sharp", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("enchantment suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["minecraft:sharpness"]);
}
