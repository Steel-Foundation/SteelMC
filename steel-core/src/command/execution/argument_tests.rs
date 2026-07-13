use crate::chunk::heightmap::HeightmapType;
use crate::command::{
    brigadier::{
        CommandDispatcher, CommandSyntaxError, CommandSyntaxErrorKind, StringReader, Suggestion,
    },
    execution::{
        BiomeOrTag, BlockPredicate, CommandArgumentSource, CommandPermissionSource,
        CommandResultCallback, Coordinates, ExecutionCommandSource, GameProfileArgument,
        ScoreHolderArgument, SteelArgumentType, SteelCommandRuntime, StructureOrTagKey,
        WorldArgument, argument,
        coordinates::{LocalCoordinates, WorldCoordinate, WorldCoordinates},
        literal,
    },
};
use glam::DVec3;
use steel_protocol::packets::game::{
    ArgumentType as ProtocolArgumentType, SuggestionType as ProtocolSuggestionType,
};
use steel_registry::{
    data_components::{ComponentPatchEntry, vanilla_components},
    item_stack::ItemStack,
    test_support::init_test_registry,
    vanilla_attributes, vanilla_biomes, vanilla_blocks, vanilla_enchantments, vanilla_entities,
    vanilla_items, vanilla_world_clocks,
    world_clock::WorldClockRef,
};
use steel_utils::{DowncastType, DowncastTypeKey, Identifier, types::GameType};
use text_components::{TextComponent, content::Content};

use crate::entity::{EntityAnchor, init_test_entities};
use crate::permission::{PermissionExpr, PermissionState};

use super::argument::SteelArgumentParser;

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
}

impl CommandArgumentSource for TestSource {
    fn default_world_clock(&self) -> Option<WorldClockRef> {
        Some(&vanilla_world_clocks::OVERWORLD)
    }

    fn domain_exists(&self, domain: &str) -> bool {
        matches!(domain, "alpha" | "beta")
    }

    fn domain_names(&self) -> Vec<&str> {
        vec!["alpha", "beta"]
    }

    fn command_world_names(&self) -> Vec<String> {
        [
            "alpha:overworld",
            "overworld",
            "alpha:arena",
            "arena",
            "beta:lobby",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    fn permission_context_world_names(&self) -> Vec<String> {
        vec!["alpha:arena".to_owned(), "alpha:overworld".to_owned()]
    }

    fn command_storage_keys(&self) -> Vec<String> {
        vec!["minecraft:global".to_owned(), "steel:data".to_owned()]
    }

    fn permission_rule_suggestions(&self) -> Vec<String> {
        vec![
            "minecraft.command.gamemode".to_owned(),
            "steel.build{plugin:region=spawn}".to_owned(),
        ]
    }

    fn permission_metadata_suggestions(&self) -> Vec<String> {
        vec!["plugin:max_homes{plugin:region=spawn}".to_owned()]
    }

    fn user_permission_rule_suggestions(&self, _targets: &GameProfileArgument) -> Vec<String> {
        vec!["steel.user_owned".to_owned()]
    }

    fn user_permission_metadata_suggestions(&self, _targets: &GameProfileArgument) -> Vec<String> {
        vec!["plugin:user_owned".to_owned()]
    }

    fn group_permission_rule_suggestions(&self, group: &str) -> Vec<String> {
        if group == "builder" {
            vec!["steel.group_owned".to_owned()]
        } else {
            Vec::new()
        }
    }

    fn group_permission_metadata_suggestions(&self, group: &str) -> Vec<String> {
        if group == "builder" {
            vec!["plugin:group_owned".to_owned()]
        } else {
            Vec::new()
        }
    }

    fn permission_group_names(&self) -> Vec<String> {
        vec!["builder".to_owned(), "default".to_owned()]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExtensionParser;

// SAFETY: This test-only key uniquely identifies `ExtensionParser` in the process.
unsafe impl DowncastType for ExtensionParser {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/command/parser/extension");
}

#[derive(Debug, PartialEq, Eq)]
struct ExtensionValue(i32);

// SAFETY: This test-only key uniquely identifies `ExtensionValue` in the process.
unsafe impl DowncastType for ExtensionValue {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/command/value/extension");
}

#[derive(Debug)]
struct UnrelatedValue;

// SAFETY: This test-only key uniquely identifies `UnrelatedValue` in the process.
unsafe impl DowncastType for UnrelatedValue {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/command/value/unrelated");
}

impl SteelArgumentParser for ExtensionParser {
    type Value = ExtensionValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        reader.read_int().map(ExtensionValue)
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (
            ProtocolArgumentType::Integer {
                min: None,
                max: None,
            },
            None,
        )
    }
}

#[test]
fn keyed_argument_erasure_accepts_new_parser_and_value_types() {
    let argument_type = SteelArgumentType::new(ExtensionParser);
    assert_eq!(argument_type.parser_type_key(), ExtensionParser::TYPE_KEY);
    assert_eq!(argument_type, argument_type.clone());

    let dispatcher = resource_dispatcher(argument_type);
    let parse = dispatcher.parse("resource 42", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("extension argument should parse");
    };
    let Some(value) = chain.top_context().argument("value") else {
        panic!("extension argument value should be retained");
    };

    assert_eq!(value.type_key(), ExtensionValue::TYPE_KEY);
    assert_eq!(
        value.downcast_ref::<ExtensionValue>(),
        Some(&ExtensionValue(42))
    );
    assert!(value.downcast_ref::<UnrelatedValue>().is_none());
}

#[test]
fn keyed_parser_equality_includes_concrete_configuration() {
    let one_tick = SteelArgumentType::time(1);
    let two_ticks = SteelArgumentType::time(2);

    assert_eq!(one_tick.parser_type_key(), two_ticks.parser_type_key());
    assert_eq!(one_tick, SteelArgumentType::time(1));
    assert_ne!(one_tick, two_ticks);
    assert_ne!(
        one_tick.parser_type_key(),
        SteelArgumentType::block_pos().parser_type_key()
    );
}

#[test]
fn component_argument_parses_vanilla_snbt_forms() {
    let dispatcher = resource_dispatcher(SteelArgumentType::component());

    for (argument, expected) in [
        ("\"hello world\"", "hello world"),
        ("'hello world'", "hello world"),
        ("\"\"", ""),
        ("{text:\"hello world\"}", "hello world"),
        ("[\"\"]", ""),
    ] {
        let input = format!("resource {argument}");
        let parse = dispatcher.parse(&input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("component form {argument} should parse");
        };

        assert_eq!(
            chain.top_context().text_component("value"),
            Some(&TextComponent::plain(expected))
        );
    }
}

#[test]
fn permission_arguments_parse_contexts_and_suggest_discovered_values() {
    let dispatcher = resource_dispatcher(SteelArgumentType::permission_rule());
    let parse = dispatcher.parse(
        "resource steel.build{domain=alpha,plugin:region=spawn}",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("contextual permission expression should parse");
    };
    assert_eq!(
        chain
            .top_context()
            .permission_rule_expression("value")
            .map(ToString::to_string)
            .as_deref(),
        Some("steel.build{domain=alpha,plugin:region=spawn}")
    );

    let parse = dispatcher.parse("resource steel.build{plugin:region=s", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("permission suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| { suggestion.text() == "steel.build{plugin:region=spawn}" })
    );

    let parse = dispatcher.parse("resource steel.build{world=alpha:", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("world context suggestions should build");
    };
    assert_eq!(
        suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        [
            "steel.build{world=alpha:arena}",
            "steel.build{world=alpha:overworld}"
        ]
    );
}

#[test]
fn permission_group_argument_can_require_a_configured_group() {
    let dispatcher = resource_dispatcher(SteelArgumentType::permission_group(true));
    assert!(
        dispatcher
            .context_chain(dispatcher.parse("resource builder", TestSource::new()))
            .is_ok()
    );
    assert!(
        dispatcher
            .context_chain(dispatcher.parse("resource missing", TestSource::new()))
            .is_err()
    );
}

#[test]
fn owned_permission_arguments_scope_unset_suggestions_to_prior_arguments() {
    let mut dispatcher = TestDispatcher::new();
    assert!(
        dispatcher
            .register(literal("user").then(
                argument("targets", SteelArgumentType::game_profile()).then(argument(
                    "permission",
                    SteelArgumentType::user_permission_rule(),
                )),
            ))
            .is_ok()
    );
    assert!(
        dispatcher
            .register(literal("group").then(
                argument("group", SteelArgumentType::permission_group(true)).then(argument(
                    "permission",
                    SteelArgumentType::group_permission_rule(),
                )),
            ))
            .is_ok()
    );

    let user_parse = dispatcher.parse("user Steve ", TestSource::new());
    let Ok(user_suggestions) = dispatcher.completion_suggestions(&user_parse) else {
        panic!("user-owned suggestions should build");
    };
    assert_eq!(
        user_suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        ["steel.user_owned"]
    );

    let group_parse = dispatcher.parse("group builder ", TestSource::new());
    let Ok(group_suggestions) = dispatcher.completion_suggestions(&group_parse) else {
        panic!("group-owned suggestions should build");
    };
    assert_eq!(
        group_suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        ["steel.group_owned"]
    );
}

#[test]
fn component_argument_preserves_list_siblings_and_following_nodes() {
    let mut dispatcher = TestDispatcher::new();
    let command = literal("component").then(
        argument("value", SteelArgumentType::component()).then(literal("done").executes(|_| Ok(1))),
    );
    assert!(dispatcher.register(command).is_ok());

    let parse = dispatcher.parse("component ['first','second'] done", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("component parser should leave following command nodes unconsumed");
    };
    let Some(component) = chain.top_context().text_component("value") else {
        panic!("component should be retained");
    };
    let mut expected = TextComponent::plain("first");
    expected.children.push(TextComponent::plain("second"));

    assert_eq!(component, &expected);
}

#[test]
fn component_argument_reports_codec_errors_at_the_argument_start() {
    let dispatcher = resource_dispatcher(SteelArgumentType::component());
    let parse = dispatcher.parse("resource {unknown:1}", TestSource::new());
    let Err(error) = dispatcher.context_chain(parse) else {
        panic!("compound without component content should be rejected");
    };

    assert_eq!(error.cursor(), Some("resource ".len()));
    let CommandSyntaxErrorKind::Dynamic(component) = error.kind() else {
        panic!("component codec failure should be a dynamic command error");
    };
    assert!(matches!(
        &component.content,
        Content::Translate(message) if message.key == "argument.component.invalid"
    ));
}

#[test]
fn component_argument_preserves_translatable_snbt_errors() {
    let dispatcher = resource_dispatcher(SteelArgumentType::component());
    let parse = dispatcher.parse("resource {text:}", TestSource::new());
    let Err(error) = dispatcher.context_chain(parse) else {
        panic!("missing SNBT value should be rejected");
    };

    let CommandSyntaxErrorKind::Dynamic(component) = error.kind() else {
        panic!("SNBT failure should be a dynamic command error");
    };
    assert!(matches!(
        &component.content,
        Content::Translate(message) if message.key == "snbt.parser.expected_unquoted_string"
    ));
}

#[test]
fn component_argument_compiles_command_strings_during_parsing() {
    let dispatcher = resource_dispatcher(SteelArgumentType::component());
    let parse = dispatcher.parse(
        r#"resource {nbt:"value",storage:"default_namespace"}"#,
        TestSource::new(),
    );
    assert!(
        dispatcher.context_chain(parse).is_ok(),
        "vanilla resource identifiers may omit the minecraft namespace"
    );
    let parse = dispatcher.parse(r#"resource {selector:'"Alex Smith"'}"#, TestSource::new());
    assert!(
        dispatcher.context_chain(parse).is_ok(),
        "quoted selector names may contain Brigadier delimiters"
    );

    for argument in [
        r#"{selector:"@e["}"#,
        r#"{selector:"Alex Smith"}"#,
        r#"{nbt:"value[",storage:"minecraft:test"}"#,
        r#"{nbt:"value",block:"~ ~"}"#,
        r#"{selector:"@a",separator:{nbt:"value",storage:"INVALID"}}"#,
        r#"{text:"nested",extra:[{selector:"@e["}]}"#,
    ] {
        let input = format!("resource {argument}");
        let parse = dispatcher.parse(&input, TestSource::new());
        let Err(error) = dispatcher.context_chain(parse) else {
            panic!("component command string in {argument} should be rejected");
        };

        assert_eq!(error.cursor(), Some("resource ".len()), "{argument}");
        assert!(
            matches!(
                error.kind(),
                CommandSyntaxErrorKind::Dynamic(component)
                    if matches!(
                        &component.content,
                        Content::Translate(message)
                            if message.key == "argument.component.invalid"
                    )
            ),
            "{argument}"
        );
    }
}

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
fn block_predicate_argument_parses_blocks_tags_properties_and_nbt() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::block_predicate());

    let parse = dispatcher.parse(
        "resource oak_log[axis=x]{custom:{value:3}}",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("concrete block predicate should parse");
    };
    let Some(predicate) = chain.top_context().block_predicate("value") else {
        panic!("block predicate should be retained");
    };
    let Some(oak_x) = steel_registry::REGISTRY
        .blocks
        .state_id_from_block_defaulted_properties(&vanilla_blocks::OAK_LOG, [("axis", "x")])
    else {
        panic!("oak log x state should exist");
    };
    assert!(predicate.matches_state(oak_x));
    assert!(!predicate.matches_state(vanilla_blocks::OAK_LOG.default_state()));
    let Some(nbt) = predicate.nbt() else {
        panic!("block predicate NBT should be retained");
    };
    assert_eq!(
        nbt.compound("custom")
            .and_then(|custom| custom.int("value")),
        Some(3)
    );

    let parse = dispatcher.parse(
        "resource #c:natural_logs/overworld[axis=y]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("block tag predicate should parse");
    };
    let Some(BlockPredicate::Tag { .. }) = chain.top_context().block_predicate("value") else {
        panic!("block tag predicate should be retained");
    };
    let Some(predicate) = chain.top_context().block_predicate("value") else {
        panic!("block tag predicate should be retained");
    };
    assert!(predicate.matches_state(vanilla_blocks::OAK_LOG.default_state()));
    assert!(!predicate.matches_state(vanilla_blocks::STONE.default_state()));
}

#[test]
fn block_predicate_argument_validates_concrete_properties_but_defers_tag_properties() {
    init_test_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::block_predicate());

    for input in [
        "resource oak_log[missing=value]",
        "resource oak_log[axis=missing]",
        "resource oak_log[axis=x,axis=y]",
        "resource #missing",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject an invalid block predicate"
        );
    }

    let parse = dispatcher.parse(
        "resource #c:natural_logs/overworld[missing=value]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("vague tag properties should parse");
    };
    let Some(predicate) = chain.top_context().block_predicate("value") else {
        panic!("block tag predicate should be retained");
    };
    assert!(!predicate.matches_state(vanilla_blocks::OAK_LOG.default_state()));
}

#[test]
fn nbt_path_argument_retains_vanilla_path_nodes() {
    let dispatcher = resource_dispatcher(SteelArgumentType::nbt_path());
    let parse = dispatcher.parse(
        "resource items[{id:\"minecraft:stone\"}].Count",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("NBT path should parse");
    };
    let Some(path) = chain.top_context().nbt_path("value") else {
        panic!("NBT path should be retained");
    };

    assert_eq!(path.as_str(), "items[{id:\"minecraft:stone\"}].Count");
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
        "resource stone[use_cooldown={seconds:5.5,cooldown_group:'minecraft:test'},max_stack_size=16]",
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

    assert_eq!(cooldown.seconds.to_bits(), 5.5_f32.to_bits());
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
fn item_arguments_propagate_translatable_snbt_errors() {
    init_test_registry();
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
