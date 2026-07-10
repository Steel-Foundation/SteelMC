use crate::chunk::heightmap::HeightmapType;
use crate::command::{
    brigadier::{CommandDispatcher, CommandSyntaxError, CommandSyntaxErrorKind, Suggestion},
    execution::{
        BiomeOrTag, CommandPermissionSource, CommandResultCallback, Coordinates,
        ExecutionCommandSource, ScoreHolderArgument, SteelArgumentType, SteelCommandRuntime,
        argument,
        coordinates::{LocalCoordinates, WorldCoordinate, WorldCoordinates},
        literal,
    },
};
use glam::DVec3;
use steel_registry::{
    data_components::{ComponentPatchEntry, vanilla_components},
    item_stack::ItemStack,
    test_support::init_test_registry,
    vanilla_attributes, vanilla_biomes, vanilla_enchantments, vanilla_entities, vanilla_items,
    vanilla_world_clocks,
    world_clock::WorldClockRef,
};
use steel_utils::{Identifier, types::GameType};

use crate::entity::{EntityAnchor, init_test_entities};
use crate::permission::{PermissionExpr, PermissionState};

struct TestSource {
    callback: CommandResultCallback,
}

impl TestSource {
    const fn new() -> Self {
        Self {
            callback: CommandResultCallback::empty(),
        }
    }
}

impl ExecutionCommandSource for TestSource {
    fn with_callback(&self, callback: CommandResultCallback) -> Self {
        Self { callback }
    }

    fn callback(&self) -> CommandResultCallback {
        self.callback.clone()
    }

    fn handle_error(&self, _error: &CommandSyntaxError, _forked: bool) {}

    fn default_world_clock(&self) -> Option<WorldClockRef> {
        Some(&vanilla_world_clocks::OVERWORLD)
    }

    fn domain_exists(&self, domain: &str) -> bool {
        matches!(domain, "alpha" | "beta")
    }

    fn domain_names(&self) -> Vec<&str> {
        vec!["alpha", "beta"]
    }

    fn selector_player_names(&self) -> Vec<String> {
        vec!["Steve".to_owned()]
    }

    fn scoreboard_objective_names(&self) -> Vec<String> {
        vec!["kills".to_owned(), "points".to_owned()]
    }

    fn allows_entity_selectors(&self) -> bool {
        true
    }

    fn allows_advanced_entity_selectors(&self) -> bool {
        true
    }
}

impl CommandPermissionSource for TestSource {
    fn permission_state(&self, _permission: &PermissionExpr) -> Option<PermissionState> {
        Some(PermissionState::Allow)
    }
}

type TestDispatcher = CommandDispatcher<TestSource, SteelCommandRuntime>;

fn dispatcher(minimum: i32) -> TestDispatcher {
    let mut dispatcher = TestDispatcher::new();
    let command = literal("duration").then(
        argument("value", SteelArgumentType::time(minimum)).executes(|context| {
            let Some(value) = context.time("value") else {
                panic!("time argument should be retained");
            };
            Ok(value)
        }),
    );
    assert!(dispatcher.register(command).is_ok());
    dispatcher
}

fn parsed_time(dispatcher: &TestDispatcher, input: &str) -> Result<i32, CommandSyntaxError> {
    let parse = dispatcher.parse(input, TestSource::new());
    let chain = dispatcher.context_chain(parse)?;
    chain
        .top_context()
        .time("value")
        .ok_or_else(|| CommandSyntaxError::dynamic("time argument was not retained"))
}

fn coordinate_dispatcher(argument_type: SteelArgumentType) -> TestDispatcher {
    let mut dispatcher = TestDispatcher::new();
    let command = literal("coordinates").then(argument("value", argument_type).executes(|_| Ok(1)));
    assert!(dispatcher.register(command).is_ok());
    dispatcher
}

fn parsed_coordinates(
    dispatcher: &TestDispatcher,
    input: &str,
) -> Result<Coordinates, CommandSyntaxError> {
    let parse = dispatcher.parse(input, TestSource::new());
    let chain = dispatcher.context_chain(parse)?;
    chain
        .top_context()
        .coordinates("value")
        .ok_or_else(|| CommandSyntaxError::dynamic("coordinates were not retained"))
}

#[test]
fn score_holder_argument_retains_deferred_names_uuids_selectors_and_wildcards() {
    let single = resource_dispatcher(SteelArgumentType::score_holder());

    let parse = single.parse("resource Player", TestSource::new());
    let Ok(chain) = single.context_chain(parse) else {
        panic!("direct score holder names should parse");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Some(ScoreHolderArgument::Name(name)) if name.as_ref() == "Player"
    ));

    let raw_uuid = "00000000-0000-0000-0000-000000000001";
    let uuid_command = format!("resource {raw_uuid}");
    let parse = single.parse(&uuid_command, TestSource::new());
    let Ok(chain) = single.context_chain(parse) else {
        panic!("UUID score holders should parse");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Some(ScoreHolderArgument::Uuid { raw, .. }) if raw.as_ref() == raw_uuid
    ));

    let parse = single.parse("resource @s", TestSource::new());
    let Ok(chain) = single.context_chain(parse) else {
        panic!("single-result entity selectors should parse as score holders");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Some(ScoreHolderArgument::Selector(_))
    ));
    let parse = single.parse("resource @a", TestSource::new());
    assert!(single.context_chain(parse).is_err());

    let multiple = resource_dispatcher(SteelArgumentType::score_holders());
    let parse = multiple.parse("resource *", TestSource::new());
    let Ok(chain) = multiple.context_chain(parse) else {
        panic!("wildcard score holders should parse");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Some(ScoreHolderArgument::Wildcard)
    ));

    let parse = single.parse("resource S", TestSource::new());
    let Ok(suggestions) = single.completion_suggestions(&parse) else {
        panic!("score holder suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "Steve")
    );
}

#[test]
fn objective_and_integer_range_arguments_retain_vanilla_values() {
    let objective = resource_dispatcher(SteelArgumentType::objective());
    let parse = objective.parse("resource kills", TestSource::new());
    let Ok(chain) = objective.context_chain(parse) else {
        panic!("objective names should parse");
    };
    assert_eq!(chain.top_context().objective_name("value"), Some("kills"));
    let parse = objective.parse("resource k", TestSource::new());
    let Ok(suggestions) = objective.completion_suggestions(&parse) else {
        panic!("objective suggestions should build");
    };
    assert_eq!(
        suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        ["kills"]
    );

    let range = resource_dispatcher(SteelArgumentType::int_range());
    for (input, matches, misses) in [
        ("resource 5", 5, 4),
        ("resource -5..10", 0, 11),
        ("resource ..10", i32::MIN, 11),
        ("resource -5..", i32::MAX, -6),
    ] {
        let parse = range.parse(input, TestSource::new());
        let Ok(chain) = range.context_chain(parse) else {
            panic!("{input} should parse as an integer range");
        };
        let Some(value) = chain.top_context().int_range("value") else {
            panic!("integer range should be retained");
        };
        assert!(value.matches(matches));
        assert!(!value.matches(misses));
    }

    for input in ["resource ..", "resource 5..2", "resource 1.5"] {
        let parse = range.parse(input, TestSource::new());
        assert!(
            range.context_chain(parse).is_err(),
            "{input} should reject an invalid integer range"
        );
    }
}

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
fn swizzle_argument_retains_unique_axes_and_rejects_duplicates() {
    let dispatcher = resource_dispatcher(SteelArgumentType::swizzle());
    let parse = dispatcher.parse("resource zx", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("unique swizzle axes should parse");
    };
    let Some(axes) = chain.top_context().swizzle("value") else {
        panic!("swizzle axes should be retained");
    };

    assert!(axes.x());
    assert!(!axes.y());
    assert!(axes.z());
    assert_eq!(
        axes.align(DVec3::new(1.9, 2.9, -1.1)),
        DVec3::new(1.0, 2.9, -2.0)
    );

    for input in ["resource xx", "resource q"] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject an invalid swizzle"
        );
    }
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
fn block_position_retains_world_coordinates_until_execution() {
    let dispatcher = coordinate_dispatcher(SteelArgumentType::block_pos());

    assert_eq!(
        parsed_coordinates(&dispatcher, "coordinates ~0.5 64 ~-3"),
        Ok(Coordinates::World(WorldCoordinates::new(
            WorldCoordinate::new(true, 0.5),
            WorldCoordinate::new(false, 64.0),
            WorldCoordinate::new(true, -3.0),
        )))
    );
}

#[test]
fn vec3_centers_absolute_integer_x_and_z_components() {
    let centered = coordinate_dispatcher(SteelArgumentType::vec3(true));
    let exact = coordinate_dispatcher(SteelArgumentType::vec3(false));

    assert_eq!(
        parsed_coordinates(&centered, "coordinates 1 2 3"),
        Ok(Coordinates::World(WorldCoordinates::new(
            WorldCoordinate::new(false, 1.5),
            WorldCoordinate::new(false, 2.0),
            WorldCoordinate::new(false, 3.5),
        )))
    );
    assert_eq!(
        parsed_coordinates(&exact, "coordinates 1 2 3"),
        Ok(Coordinates::World(WorldCoordinates::new(
            WorldCoordinate::new(false, 1.0),
            WorldCoordinate::new(false, 2.0),
            WorldCoordinate::new(false, 3.0),
        )))
    );
}

#[test]
fn coordinate_arguments_parse_local_components_and_reject_mixed_types() {
    let dispatcher = coordinate_dispatcher(SteelArgumentType::block_pos());

    assert_eq!(
        parsed_coordinates(&dispatcher, "coordinates ^1 ^ ^-5"),
        Ok(Coordinates::Local(LocalCoordinates::new(1.0, 0.0, -5.0)))
    );
    assert!(parsed_coordinates(&dispatcher, "coordinates ^1 ~ ^-5").is_err());
    assert!(parsed_coordinates(&dispatcher, "coordinates ~ 1 ^-5").is_err());
}

#[test]
fn block_position_requires_integers_only_for_absolute_components() {
    let dispatcher = coordinate_dispatcher(SteelArgumentType::block_pos());

    assert!(parsed_coordinates(&dispatcher, "coordinates 0.5 64 0").is_err());
    assert!(parsed_coordinates(&dispatcher, "coordinates ~0.5 64 ~").is_ok());
}

#[test]
fn rotation_argument_retains_yaw_then_pitch_expressions() {
    let dispatcher = coordinate_dispatcher(SteelArgumentType::rotation());

    assert_eq!(
        parsed_coordinates(&dispatcher, "coordinates 90 ~5"),
        Ok(Coordinates::World(WorldCoordinates::new(
            WorldCoordinate::new(true, 5.0),
            WorldCoordinate::new(false, 90.0),
            WorldCoordinate::new(true, 0.0),
        )))
    );
    assert!(parsed_coordinates(&dispatcher, "coordinates 90").is_err());
    assert!(parsed_coordinates(&dispatcher, "coordinates ^ ^").is_err());
}

#[test]
fn coordinate_suggestions_include_vanilla_partial_prefixes() {
    let dispatcher = coordinate_dispatcher(SteelArgumentType::block_pos());
    let parse = dispatcher.parse("coordinates ", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("coordinate suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();

    assert_eq!(suggestions, ["~", "~ ~", "~ ~ ~"]);

    let parse = dispatcher.parse("coordinates ^", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("local coordinate suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["^ ^", "^ ^ ^"]);
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
    init_test_entities();
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
    init_test_entities();
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

#[test]
fn item_stack_argument_parses_supported_components_and_registered_removals() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[max_stack_size=16,enchantment_glint_override=true,!lore]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("supported item components should parse");
    };
    let Some(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert!(stack.is(&vanilla_items::ITEMS.stone));
    assert_eq!(stack.max_stack_size(), 16);
    assert_eq!(
        stack.get(vanilla_components::ENCHANTMENT_GLINT_OVERRIDE),
        Some(&true)
    );
    assert!(matches!(
        stack.patch().get_entry(&vanilla_components::LORE.key),
        Some(ComponentPatchEntry::Removed)
    ));
}

#[test]
fn item_stack_argument_uses_vanilla_numeric_codec_coercions() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[max_stack_size=16.9d,enchantment_glint_override=2,potion_duration_scale=1]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("vanilla numeric component coercions should parse");
    };
    let Some(stack) = chain.top_context().item_stack("value") else {
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
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse(
        "resource stone[use_cooldown={seconds:1.0f,cooldown_group:'minecraft:test'},max_stack_size=16]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("supported compound component should parse");
    };
    let Some(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };
    let Some(cooldown) = stack.get(vanilla_components::USE_COOLDOWN) else {
        panic!("use cooldown should be retained");
    };

    assert_eq!(cooldown.seconds.to_bits(), 1.0_f32.to_bits());
    assert_eq!(
        cooldown.cooldown_group,
        Some(Identifier::vanilla_static("test"))
    );
    assert_eq!(stack.max_stack_size(), 16);
}

#[test]
fn item_stack_argument_rejects_placeholder_transient_and_invalid_components() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());

    for input in [
        "resource stone[lore=[]]",
        "resource stone[creative_slot_lock={}]",
        "resource stone[additional_trade_cost={}]",
        "resource stone[map_post_processing={}]",
        "resource stone[missing={}]",
        "resource stone[max_stack_size=16,max_stack_size=8]",
        "resource stone[max_stack_size=0]",
        "resource stone[max_damage=10]",
        "resource stone[potion_duration_scale=-0.0f]",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should be rejected"
        );
    }
}

#[test]
fn removing_max_stack_size_uses_vanillas_fallback_of_one() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_stack());
    let parse = dispatcher.parse("resource stone[!max_stack_size]", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("registered component removal should parse");
    };
    let Some(stack) = chain.top_context().item_stack("value") else {
        panic!("item stack should be retained");
    };

    assert_eq!(stack.max_stack_size(), 1);
}

#[test]
fn item_stack_argument_suggests_items_and_supported_component_operations() {
    init_test_registry();
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

#[test]
fn item_predicate_argument_matches_targets_boolean_terms_and_count_ranges() {
    init_test_registry();
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

    assert!(predicate.matches(&ItemStack::with_count(&vanilla_items::ITEMS.oak_log, 3)));
    assert!(!predicate.matches(&ItemStack::with_count(&vanilla_items::ITEMS.oak_log, 4)));
    assert!(!predicate.matches(&ItemStack::with_count(&vanilla_items::ITEMS.stone, 3)));
}

#[test]
fn item_predicate_argument_decodes_exact_components_before_matching() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse("resource stone[max_stack_size=64b]", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("numeric component value should use the registered codec");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };

    assert!(predicate.matches(&ItemStack::new(&vanilla_items::ITEMS.stone)));
}

#[test]
fn item_predicate_argument_supports_damage_and_enchantment_predicates() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let input = "resource diamond_sword[damage~{damage:7,durability:{min:1}},enchantments~[{enchantments:'minecraft:sharpness',levels:{min:2}}]]";
    let parse = dispatcher.parse(input, TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("implemented data component predicates should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };
    let mut sword = ItemStack::new(&vanilla_items::ITEMS.diamond_sword);
    sword.set_damage_value(7);
    sword.set_enchantments(&[(Identifier::vanilla_static("sharpness"), 3)], false);

    assert!(predicate.matches(&sword));
    sword.set_damage_value(6);
    assert!(!predicate.matches(&sword));
}

#[test]
fn item_predicate_argument_supports_attribute_modifier_collection_predicates() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let input = "resource stone[attribute_modifiers~{modifiers:{contains:[{attribute:'minecraft:attack_damage',id:'minecraft:test',amount:{min:2.5,max:3.5},operation:'add_value',slot:'mainhand'}],count:[{test:{attribute:'minecraft:attack_damage'},count:1}],size:1}}]";
    let parse = dispatcher.parse(input, TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("attribute modifier collection predicate should parse");
    };
    let Some(predicate) = chain.top_context().item_predicate("value") else {
        panic!("item predicate should be retained");
    };
    let mut stack = ItemStack::new(&vanilla_items::ITEMS.stone);
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
    init_test_registry();
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
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());

    for (path, component) in [
        ("creative_slot_lock", vanilla_components::CREATIVE_SLOT_LOCK),
        (
            "additional_trade_cost",
            vanilla_components::ADDITIONAL_TRADE_COST,
        ),
        (
            "map_post_processing",
            vanilla_components::MAP_POST_PROCESSING,
        ),
    ] {
        let input = format!("resource stone[{path}~{{ignored:1}}]");
        let parse = dispatcher.parse(&input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("transient component existence predicate should accept a compound");
        };
        let Some(predicate) = chain.top_context().item_predicate("value") else {
            panic!("item predicate should be retained");
        };
        let mut stack = ItemStack::new(&vanilla_items::ITEMS.stone);
        stack.set(component, ());

        assert!(predicate.matches(&stack));
    }
}

#[test]
fn item_predicate_argument_rejects_unsupported_predicates_during_parsing() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::item_predicate());
    let parse = dispatcher.parse(
        "resource potion[potion_contents~{potion:'minecraft:water'}]",
        TestSource::new(),
    );

    assert!(dispatcher.context_chain(parse).is_err());
}

#[test]
fn item_predicate_argument_rejects_transient_components_as_component_tests() {
    init_test_registry();
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
    init_test_registry();
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

#[test]
fn entity_selector_argument_is_retained_for_deferred_resolution() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::players());
    let parse = dispatcher.parse("resource @a[distance=..10]", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("selector should parse");
    };

    assert!(chain.top_context().entity_selector("value").is_some());
}

#[test]
fn entity_selector_argument_suggests_source_domain_players() {
    let dispatcher = resource_dispatcher(SteelArgumentType::players());
    let parse = dispatcher.parse("resource S", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("selector suggestions should build");
    };

    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "Steve")
    );
}

#[test]
fn time_argument_parses_vanilla_units_and_defaults_to_ticks() {
    let dispatcher = dispatcher(0);

    assert_eq!(parsed_time(&dispatcher, "duration 2d"), Ok(48_000));
    assert_eq!(parsed_time(&dispatcher, "duration 1.5s"), Ok(30));
    assert_eq!(parsed_time(&dispatcher, "duration 7t"), Ok(7));
    assert_eq!(parsed_time(&dispatcher, "duration 7"), Ok(7));
}

#[test]
fn time_argument_uses_java_half_up_rounding() {
    let dispatcher = dispatcher(i32::MIN);

    assert_eq!(parsed_time(&dispatcher, "duration 0.5t"), Ok(1));
    assert_eq!(parsed_time(&dispatcher, "duration -0.5t"), Ok(0));
    assert_eq!(parsed_time(&dispatcher, "duration -1.5t"), Ok(-1));
}

#[test]
fn time_argument_rejects_invalid_units_and_values_below_its_minimum() {
    let dispatcher = dispatcher(1);

    let invalid_unit = parsed_time(&dispatcher, "duration 1x");
    assert!(matches!(
        invalid_unit,
        Err(error) if matches!(error.kind(), CommandSyntaxErrorKind::Dynamic(_))
    ));
    let too_low = parsed_time(&dispatcher, "duration 0t");
    assert!(matches!(
        too_low,
        Err(error) if matches!(error.kind(), CommandSyntaxErrorKind::Dynamic(_))
    ));
}

#[test]
fn time_argument_suggests_units_for_a_numeric_prefix() {
    let dispatcher = dispatcher(0);
    let parse = dispatcher.parse("duration 10", TestSource::new());
    let suggestions = dispatcher.completion_suggestions(&parse);
    let Ok(suggestions) = suggestions else {
        panic!("time suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();

    assert_eq!(suggestions, ["10d", "10s", "10t"]);
}

fn resource_dispatcher(argument_type: SteelArgumentType) -> TestDispatcher {
    let mut dispatcher = TestDispatcher::new();
    let command = literal("resource").then(argument("value", argument_type).executes(|_| Ok(1)));
    assert!(dispatcher.register(command).is_ok());
    dispatcher
}

#[test]
fn world_clock_argument_resolves_default_and_explicit_namespaces() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::world_clock());

    for input in ["resource overworld", "resource minecraft:overworld"] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("registered world clock should parse");
        };
        assert_eq!(
            chain.top_context().world_clock("value"),
            Some(&vanilla_world_clocks::OVERWORLD)
        );
    }
}

#[test]
fn world_clock_argument_rejects_unknown_resources() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::world_clock());
    let parse = dispatcher.parse("resource missing", TestSource::new());
    let error = dispatcher.context_chain(parse);

    assert!(matches!(
        error,
        Err(error) if matches!(error.kind(), CommandSyntaxErrorKind::Dynamic(_))
    ));
}

#[test]
fn time_marker_argument_retains_default_namespace_identifier() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::time_marker(None));
    let parse = dispatcher.parse("resource day", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("time marker identifier should parse");
    };

    assert_eq!(
        chain.top_context().identifier("value"),
        Some(&Identifier::vanilla_static("day"))
    );
}

#[test]
fn time_marker_argument_suggests_only_visible_markers_for_selected_clock() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::time_marker(None));
    let parse = dispatcher.parse("resource d", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("time marker suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();

    assert_eq!(suggestions, ["minecraft:day"]);
}

#[test]
fn timeline_suggestions_use_the_preceding_clock_argument() {
    init_test_registry();
    let mut dispatcher = TestDispatcher::new();
    let command =
        literal("timeline").then(argument("clock", SteelArgumentType::world_clock()).then(
            argument("value", SteelArgumentType::timeline(Some("clock"))).executes(|_| Ok(1)),
        ));
    assert!(dispatcher.register(command).is_ok());

    let parse = dispatcher.parse("timeline overworld d", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("overworld timeline suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["minecraft:day"]);

    let parse = dispatcher.parse("timeline the_end ", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("end timeline suggestions should build");
    };
    assert!(suggestions.is_empty());
}
