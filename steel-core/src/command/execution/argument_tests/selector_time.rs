use super::*;

fn dispatcher(minimum: i32) -> TestDispatcher {
    let mut dispatcher = TestDispatcher::new();
    let command = literal("duration").then(
        argument("value", SteelArgumentType::time(minimum)).executes(|context| {
            let Ok(value) = context.time("value") else {
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
    chain.top_context().time("value")
}

#[test]
fn entity_selector_argument_is_retained_for_deferred_resolution() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::players());
    let parse = dispatcher.parse("resource @a[distance=..10]", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("selector should parse");
    };

    assert!(chain.top_context().entity_selector("value").is_ok());
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

#[test]
fn world_clock_argument_resolves_default_and_explicit_namespaces() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::world_clock());

    for input in ["resource overworld", "resource minecraft:overworld"] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Ok(chain) = dispatcher.context_chain(parse) else {
            panic!("registered world clock should parse");
        };
        assert_eq!(
            chain.top_context().world_clock("value"),
            Ok(&vanilla_world_clocks::OVERWORLD)
        );
    }
}

#[test]
fn world_clock_argument_rejects_unknown_resources() {
    init_vanilla_registry();
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
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::time_marker(None));
    let parse = dispatcher.parse("resource day", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("time marker identifier should parse");
    };

    assert_eq!(
        chain.top_context().identifier("value"),
        Ok(&Identifier::vanilla_static("day"))
    );
}

#[test]
fn time_marker_argument_suggests_only_visible_markers_for_selected_clock() {
    init_vanilla_registry();
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
    init_vanilla_registry();
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
