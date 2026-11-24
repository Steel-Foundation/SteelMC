use super::*;

// SAFETY: This test-only key uniquely identifies `ExtensionParser` in the process.
unsafe impl DowncastType for ExtensionParser {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/command/parser/extension");
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ExtensionValue(i32);

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
    let Ok(value) = chain.top_context().argument("value") else {
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
            Ok(&TextComponent::plain(expected))
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
        Ok("steel.build{domain=alpha,plugin:region=spawn}")
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
fn permission_group_suggestions_filter_special_group_roles() {
    let suggest = |parser: SteelArgumentType| {
        let dispatcher = resource_dispatcher(parser);
        let parse = dispatcher.parse("resource ", TestSource::new());
        match dispatcher.completion_suggestions(&parse) {
            Ok(suggestions) => suggestions
                .list()
                .iter()
                .map(Suggestion::text)
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            Err(error) => panic!("group suggestions should build: {error}"),
        }
    };

    assert_eq!(suggest(SteelArgumentType::default_group_add()), ["builder"]);
    assert_eq!(
        suggest(SteelArgumentType::default_group_remove()),
        ["default"]
    );
    assert_eq!(
        suggest(SteelArgumentType::command_block_group_add()),
        ["default"]
    );
    assert_eq!(
        suggest(SteelArgumentType::command_block_group_remove()),
        ["builder"]
    );
    assert_eq!(
        suggest(SteelArgumentType::op_group_set()),
        ["builder", "default"]
    );
}

#[test]
fn permission_collection_argument_reads_namespaced_identifiers() {
    let dispatcher = resource_dispatcher(SteelArgumentType::permission_collection());
    let parse = dispatcher.parse("resource minecraft:gamemasters", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("namespaced collection identifier should parse");
    };
    assert!(chain.top_context().argument("value").is_ok());
}

#[test]
fn permission_collection_member_argument_reads_permissions_and_references() {
    let dispatcher = resource_dispatcher(SteelArgumentType::add_permission_collection_member());

    for (input, expected) in [
        (
            "resource steel:build",
            PermissionGroupRule::permission(Identifier::new("steel", "build")),
        ),
        (
            "resource #minecraft:gamemasters",
            PermissionCollectionMember::Collection(Identifier::new("minecraft", "gamemasters")),
        ),
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("collection member {input} should parse");
        };
        let value = chain.top_context().argument("value");
        let Ok(value) = value else {
            panic!("collection member {input} value should be retained");
        };
        assert_eq!(
            value.downcast_ref::<PermissionCollectionMember>(),
            Some(&expected)
        );
    }
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
                    SteelArgumentType::remove_group_rule(),
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

    let restricted_user_parse = dispatcher.parse("user Steve ", TestSource::restricted());
    let Ok(restricted_user_suggestions) = dispatcher.completion_suggestions(&restricted_user_parse)
    else {
        panic!("restricted user-owned suggestions should build");
    };
    assert_eq!(
        restricted_user_suggestions
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
fn collection_member_suggestions_filter_direct_members_for_add_and_remove() {
    let mut dispatcher = TestDispatcher::new();
    for (name, parser) in [
        ("add", SteelArgumentType::add_permission_collection_member()),
        (
            "remove",
            SteelArgumentType::remove_permission_collection_member(),
        ),
    ] {
        assert!(
            dispatcher
                .register(
                    literal(name).then(
                        argument("collection", SteelArgumentType::permission_collection())
                            .then(argument("member", parser))
                    )
                )
                .is_ok()
        );
    }

    let source = || {
        TestSource::with_collection_members(vec![
            "steel.build{plugin:region=spawn}".to_owned(),
            "#minecraft:gamemasters".to_owned(),
        ])
    };
    let suggest = |input: &str| {
        let parse = dispatcher.parse(input, source());
        match dispatcher.completion_suggestions(&parse) {
            Ok(suggestions) => suggestions
                .list()
                .iter()
                .map(Suggestion::text)
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            Err(error) => panic!("{input} suggestions should build: {error}"),
        }
    };

    let add = suggest("add minecraft:gamemasters ");
    assert!(add.contains(&"minecraft.command.gamemode".to_owned()));
    assert!(!add.contains(&"steel.build{plugin:region=spawn}".to_owned()));
    assert!(!add.contains(&"#minecraft:gamemasters".to_owned()));

    let remove = suggest("remove minecraft:gamemasters ");
    assert_eq!(
        remove,
        ["#minecraft:gamemasters", "steel.build{plugin:region=spawn}"]
    );
}

#[test]
fn group_rule_suggestions_filter_existing_rules_for_add_and_unset() {
    let mut dispatcher = TestDispatcher::new();
    for (name, parser) in [
        ("allow", SteelArgumentType::add_group_rule()),
        ("unset", SteelArgumentType::remove_group_rule()),
    ] {
        assert!(
            dispatcher
                .register(
                    literal(name).then(
                        argument("group", SteelArgumentType::permission_group(true))
                            .then(argument("permission", parser))
                    )
                )
                .is_ok()
        );
    }

    let suggest = |input: &str| {
        let parse = dispatcher.parse(input, TestSource::new());
        match dispatcher.completion_suggestions(&parse) {
            Ok(suggestions) => suggestions
                .list()
                .iter()
                .map(Suggestion::text)
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            Err(error) => panic!("{input} suggestions should build: {error}"),
        }
    };

    let add = suggest("allow builder ");
    assert!(add.contains(&"minecraft.command.gamemode".to_owned()));
    assert!(add.contains(&"steel.build{plugin:region=spawn}".to_owned()));
    assert!(add.contains(&"#minecraft:gamemasters".to_owned()));
    assert!(!add.contains(&"steel.group_owned".to_owned()));

    assert_eq!(suggest("unset builder "), ["steel.group_owned"]);
}

#[test]
fn permission_suggestions_list_known_keys_without_management_authority() {
    let mut dispatcher = TestDispatcher::new();
    for (name, argument_type) in [
        ("group", SteelArgumentType::permission_group(true)),
        ("collection", SteelArgumentType::permission_collection()),
        ("permission", SteelArgumentType::permission_rule()),
        ("metadata", SteelArgumentType::permission_metadata()),
    ] {
        assert!(
            dispatcher
                .register(literal(name).then(argument("value", argument_type)))
                .is_ok()
        );
    }
    assert!(
        dispatcher
            .register(
                literal("grouprule").then(
                    argument("group", SteelArgumentType::permission_group(true))
                        .then(argument("value", SteelArgumentType::add_group_rule()))
                )
            )
            .is_ok()
    );
    assert!(
        dispatcher
            .register(literal("member").then(
                argument("collection", SteelArgumentType::permission_collection()).then(argument(
                    "value",
                    SteelArgumentType::add_permission_collection_member()
                ))
            ))
            .is_ok()
    );

    let suggest = |input: &str, source: TestSource| {
        let parse = dispatcher.parse(input, source);
        match dispatcher.completion_suggestions(&parse) {
            Ok(suggestions) => suggestions
                .list()
                .iter()
                .map(Suggestion::text)
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            Err(error) => panic!("{input} suggestions should build: {error}"),
        }
    };

    // Group and collection names still require the matching management authority.
    for input in ["group ", "collection "] {
        assert!(!suggest(input, TestSource::new()).is_empty());
        assert!(
            suggest(input, TestSource::restricted()).is_empty(),
            "{input} should hide unmanageable values"
        );
    }

    // Permission and metadata keys are always discoverable; execution still checks authority.
    for input in ["permission ", "metadata "] {
        let allowed = suggest(input, TestSource::new());
        assert!(!allowed.is_empty(), "{input} should suggest known values");
        assert_eq!(suggest(input, TestSource::restricted()), allowed);
    }

    // Scoped rule arguments keep their known permission keys; only collection references
    // remain authority-gated.
    for input in ["grouprule builder ", "member minecraft:gamemasters "] {
        let allowed = suggest(input, TestSource::new());
        let restricted = suggest(input, TestSource::restricted());
        assert!(allowed.iter().any(|value| value.starts_with('#')));
        assert!(restricted.iter().any(|value| !value.starts_with('#')));
        assert!(restricted.iter().all(|value| !value.starts_with('#')));
    }
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
    let Ok(component) = chain.top_context().text_component("value") else {
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
