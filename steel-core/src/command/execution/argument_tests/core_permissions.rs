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
