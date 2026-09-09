use super::*;

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
    chain.top_context().coordinates("value")
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
fn vec2_centers_absolute_integer_components_and_preserves_source_y() {
    let dispatcher = coordinate_dispatcher(SteelArgumentType::vec2(true));

    assert_eq!(
        parsed_coordinates(&dispatcher, "coordinates 1 ~-3"),
        Ok(Coordinates::World(WorldCoordinates::new(
            WorldCoordinate::new(false, 1.5),
            WorldCoordinate::new(true, 0.0),
            WorldCoordinate::new(true, -3.0),
        )))
    );
    assert!(parsed_coordinates(&dispatcher, "coordinates 1").is_err());
    assert!(parsed_coordinates(&dispatcher, "coordinates ^1 ^2").is_err());
}

#[test]
fn vec2_suggestions_stop_after_two_components() {
    let dispatcher = coordinate_dispatcher(SteelArgumentType::vec2(true));
    let parse = dispatcher.parse("coordinates ", TestSource::new());
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("coordinate suggestions should build");
    };
    let suggestions = suggestions
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();

    assert_eq!(suggestions, ["~", "~ ~"]);
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
