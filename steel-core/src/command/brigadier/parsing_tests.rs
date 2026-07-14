use super::{
    ArgumentType, CommandDispatcher, CommandNodeBuilder, CommandRequirement,
    CommandSyntaxErrorKind, NodeId, StringRange, argument, literal, node::CommandNode,
};

#[derive(Debug)]
struct TestSource {
    allowed: bool,
}

fn register(
    dispatcher: &mut CommandDispatcher<TestSource>,
    builder: CommandNodeBuilder<TestSource>,
) -> NodeId {
    let Ok(node) = dispatcher.register(builder) else {
        panic!("command registration should succeed");
    };
    node
}

fn parsed_names<'dispatcher>(
    dispatcher: &'dispatcher CommandDispatcher<TestSource>,
    nodes: &[super::ParsedCommandNode],
) -> Vec<&'dispatcher str> {
    nodes
        .iter()
        .map(|parsed| {
            let Some(node) = dispatcher.node(parsed.node()) else {
                panic!("parsed node should belong to the dispatcher");
            };
            node.name()
        })
        .collect()
}

#[test]
fn parses_literal_commands_and_tracks_ranges() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("ping").executes(|_| Ok(1)));

    let parse = dispatcher.parse("ping", TestSource { allowed: true });

    assert!(!parse.reader().can_read());
    assert!(parse.errors().is_empty());
    assert!(parse.context().is_executable());
    assert_eq!(parse.context().range(), StringRange::between(0, 4));
    assert_eq!(parsed_names(&dispatcher, parse.context().nodes()), ["ping"]);
    assert_eq!(
        parse.context().nodes()[0].range(),
        StringRange::between(0, 4)
    );
}

#[test]
fn literal_ranges_and_failure_cursors_use_utf16_units() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("say").then(literal("\u{1f600}").executes(|_| Ok(1))),
    );

    let parse = dispatcher.parse("say \u{1f600}", TestSource { allowed: true });
    assert_eq!(parse.reader().cursor(), 6);
    assert_eq!(
        parse.context().nodes()[1].range(),
        StringRange::between(4, 6)
    );

    let failed = dispatcher.parse("say \u{1f603}", TestSource { allowed: true });
    assert_eq!(failed.reader().cursor(), 4);
    assert!(failed.errors().is_empty());
}

#[test]
fn requirements_hide_unavailable_nodes_from_parsing() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("secure")
            .requires(CommandRequirement::contextual(|source: &TestSource| {
                source.allowed
            }))
            .executes(|_| Ok(1)),
    );

    let denied = dispatcher.parse("secure", TestSource { allowed: false });
    assert_eq!(denied.reader().cursor(), 0);
    assert!(denied.context().nodes().is_empty());
    assert!(denied.errors().is_empty());

    let allowed = dispatcher.parse("secure", TestSource { allowed: true });
    assert!(!allowed.reader().can_read());
    assert!(allowed.context().is_executable());
}

#[test]
fn execution_requirements_do_not_hide_descendants() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("route")
            .executes(|_| Ok(1))
            .also_requires_execution(CommandRequirement::authorization(|source: &TestSource| {
                source.allowed
            }))
            .then(literal("child").executes(|_| Ok(2))),
    );

    let parent = dispatcher.parse("route", TestSource { allowed: false });
    assert!(!parent.reader().can_read());
    assert!(!parent.context().is_executable());

    let child = dispatcher.parse("route child", TestSource { allowed: false });
    assert!(!child.reader().can_read());
    assert!(child.context().is_executable());
}

#[test]
fn incomplete_commands_stop_before_the_trailing_separator() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("foo").then(literal("bar").executes(|_| Ok(1))),
    );

    let parse = dispatcher.parse("foo ", TestSource { allowed: true });

    assert_eq!(parse.reader().cursor(), 3);
    assert_eq!(parse.reader().remaining(), " ");
    assert_eq!(parsed_names(&dispatcher, parse.context().nodes()), ["foo"]);
    assert!(!parse.context().is_executable());
}

#[test]
fn unknown_subcommands_keep_the_last_successful_context() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("foo")
            .executes(|_| Ok(1))
            .then(literal("bar").executes(|_| Ok(2))),
    );

    let parse = dispatcher.parse("foo baz", TestSource { allowed: true });

    assert_eq!(parse.reader().cursor(), 4);
    assert_eq!(parse.reader().remaining(), "baz");
    assert_eq!(parsed_names(&dispatcher, parse.context().nodes()), ["foo"]);
    assert!(parse.context().is_executable());
    assert!(parse.errors().is_empty());
}

#[test]
fn parses_boolean_and_bounded_integer_arguments() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("set")
            .then(argument("enabled", ArgumentType::bool()).executes(|_| Ok(1)))
            .then(argument("count", ArgumentType::integer(1, 10)).executes(|_| Ok(1))),
    );

    let boolean = dispatcher.parse("set true", TestSource { allowed: true });
    assert_eq!(boolean.context().boolean("enabled"), Some(true));
    assert_eq!(boolean.context().integer("count"), None);

    let integer = dispatcher.parse("set 7", TestSource { allowed: true });
    assert_eq!(integer.context().integer("count"), Some(7));
    assert_eq!(integer.context().boolean("enabled"), None);
}

#[test]
fn bounded_integer_errors_reset_to_the_argument_start() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("set").then(argument("count", ArgumentType::integer(1, 10))),
    );

    let too_high = dispatcher.parse("set 11", TestSource { allowed: true });
    assert_eq!(too_high.reader().cursor(), 4);
    assert_eq!(
        too_high.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::IntegerTooHigh {
            found: 11,
            maximum: 10,
        }
    );
    assert_eq!(too_high.errors()[0].error().cursor(), Some(4));

    let too_low = dispatcher.parse("set 0", TestSource { allowed: true });
    assert_eq!(
        too_low.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::IntegerTooLow {
            found: 0,
            minimum: 1,
        }
    );
}

#[test]
fn argument_parsing_requires_a_space_before_trailing_data() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("set").then(argument("count", ArgumentType::integer(1, 10))),
    );

    let parse = dispatcher.parse("set 1tail", TestSource { allowed: true });

    assert_eq!(parse.reader().cursor(), 4);
    assert_eq!(
        parse.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::ExpectedArgumentSeparator
    );
    assert_eq!(parse.errors()[0].error().cursor(), Some(5));
    let error_node = parse.errors()[0].node();
    assert_eq!(
        dispatcher.node(error_node).map(CommandNode::name),
        Some("count")
    );
}

#[test]
fn complete_potential_wins_over_an_incomplete_sibling() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("test")
            .then(argument("short", ArgumentType::integer(i32::MIN, i32::MAX)).executes(|_| Ok(1)))
            .then(
                argument("first", ArgumentType::integer(i32::MIN, i32::MAX)).then(
                    argument("second", ArgumentType::integer(i32::MIN, i32::MAX))
                        .executes(|_| Ok(2)),
                ),
            ),
    );

    let parse = dispatcher.parse("test 1 2", TestSource { allowed: true });

    assert!(!parse.reader().can_read());
    assert_eq!(
        parsed_names(&dispatcher, parse.context().nodes()),
        ["test", "first", "second"]
    );
    assert_eq!(parse.context().integer("short"), None);
    assert_eq!(parse.context().integer("first"), Some(1));
    assert_eq!(parse.context().integer("second"), Some(2));
}

#[test]
fn matching_literals_take_priority_over_argument_siblings() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("choose")
            .then(literal("1").executes(|_| Ok(1)))
            .then(
                argument("number", ArgumentType::integer(i32::MIN, i32::MAX)).executes(|_| Ok(2)),
            ),
    );

    let parse = dispatcher.parse("choose 1", TestSource { allowed: true });

    assert_eq!(
        parsed_names(&dispatcher, parse.context().nodes()),
        ["choose", "1"]
    );
    assert_eq!(parse.context().integer("number"), None);
}

#[test]
fn identity_redirects_create_a_child_parse_context() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("actual").executes(|_| Ok(1)));
    let root = dispatcher.root();
    register(&mut dispatcher, literal("alias").redirects(root));

    let parse = dispatcher.parse("alias actual", TestSource { allowed: true });

    assert!(!parse.reader().can_read());
    assert_eq!(
        parsed_names(&dispatcher, parse.context().nodes()),
        ["alias"]
    );
    let Some(child) = parse.context().child() else {
        panic!("redirect should produce a child parse context");
    };
    assert_eq!(parsed_names(&dispatcher, child.nodes()), ["actual"]);
    assert!(child.is_executable());
}
