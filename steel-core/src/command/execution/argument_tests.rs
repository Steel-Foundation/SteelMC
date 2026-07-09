use crate::command::{
    brigadier::{CommandDispatcher, CommandSyntaxError, CommandSyntaxErrorKind, Suggestion},
    execution::{
        CommandResultCallback, Coordinates, ExecutionCommandSource, SteelArgumentType,
        SteelCommandRuntime, argument,
        coordinates::{LocalCoordinates, WorldCoordinate, WorldCoordinates},
        literal,
    },
};
use steel_registry::{test_support::init_test_registry, vanilla_entities, vanilla_world_clocks};
use steel_utils::Identifier;

use crate::entity::init_test_entities;

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

    fn default_world_clock(&self) -> Option<steel_registry::world_clock::WorldClockRef> {
        Some(&vanilla_world_clocks::OVERWORLD)
    }

    fn domain_exists(&self, domain: &str) -> bool {
        matches!(domain, "alpha" | "beta")
    }

    fn domain_names(&self) -> Vec<&str> {
        vec!["alpha", "beta"]
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
        .map(|suggestion| suggestion.text())
        .collect::<Vec<_>>();

    assert_eq!(suggestions, ["~", "~ ~", "~ ~ ~"]);

    let parse = dispatcher.parse("coordinates ^", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("local coordinate suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(|suggestion| suggestion.text())
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
        .map(|suggestion| suggestion.text())
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["beta"]);
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
        .map(|suggestion| suggestion.text())
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
        .map(|suggestion| suggestion.text())
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
        .map(|suggestion| suggestion.text())
        .collect::<Vec<_>>();
    assert_eq!(suggestions, ["minecraft:day"]);

    let parse = dispatcher.parse("timeline the_end ", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("end timeline suggestions should build");
    };
    assert!(suggestions.is_empty());
}
