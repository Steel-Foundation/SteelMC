use super::{
    ArgumentType, CommandDispatcher, CommandNodeBuilder, CommandSyntaxErrorKind, NodeId,
    RegistrationErrorKind, argument, literal,
};

fn register(dispatcher: &mut CommandDispatcher<()>, builder: CommandNodeBuilder<()>) -> NodeId {
    let Ok(node) = dispatcher.register(builder) else {
        panic!("command registration should succeed");
    };
    node
}

#[test]
fn parses_bounded_long_arguments() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("long").then(argument("value", ArgumentType::long(-10, 10)).executes(|_| Ok(1))),
    );

    let parse = dispatcher.parse("long -7", ());

    assert!(!parse.reader().can_read());
    assert_eq!(parse.context().long("value"), Some(-7));
    assert!(parse.context().is_executable());
}

#[test]
fn long_bounds_reset_the_error_cursor() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("long").then(argument("value", ArgumentType::long(-10, 10))),
    );

    let too_low = dispatcher.parse("long -11", ());
    assert_eq!(too_low.reader().cursor(), 5);
    assert_eq!(too_low.errors()[0].error().cursor(), Some(5));
    assert_eq!(
        too_low.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::LongTooLow {
            found: -11,
            minimum: -10,
        }
    );

    let too_high = dispatcher.parse("long 11", ());
    assert_eq!(
        too_high.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::LongTooHigh {
            found: 11,
            maximum: 10,
        }
    );
}

#[test]
fn parses_bounded_float_and_double_arguments() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("float")
            .then(argument("value", ArgumentType::float(-10.0, 10.0)).executes(|_| Ok(1))),
    );
    register(
        &mut dispatcher,
        literal("double")
            .then(argument("value", ArgumentType::double(-10.0, 10.0)).executes(|_| Ok(1))),
    );

    let float = dispatcher.parse("float -.5", ());
    assert_eq!(float.context().float("value"), Some(-0.5));

    let double = dispatcher.parse("double 1.25", ());
    assert_eq!(double.context().double("value"), Some(1.25));
}

#[test]
fn float_and_double_bounds_reset_the_error_cursor() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("float").then(argument("value", ArgumentType::float(-1.0, 1.0))),
    );
    register(
        &mut dispatcher,
        literal("double").then(argument("value", ArgumentType::double(-1.0, 1.0))),
    );

    let float = dispatcher.parse("float 2", ());
    assert_eq!(float.errors()[0].error().cursor(), Some(6));
    assert_eq!(
        float.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::FloatTooHigh {
            found: 2.0,
            maximum: 1.0,
        }
    );

    let double = dispatcher.parse("double -2", ());
    assert_eq!(double.errors()[0].error().cursor(), Some(7));
    assert_eq!(
        double.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::DoubleTooLow {
            found: -2.0,
            minimum: -1.0,
        }
    );
}

#[test]
fn parses_word_quotable_and_greedy_strings() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("word").then(argument("value", ArgumentType::word()).executes(|_| Ok(1))),
    );
    register(
        &mut dispatcher,
        literal("string").then(argument("value", ArgumentType::string()).executes(|_| Ok(1))),
    );
    register(
        &mut dispatcher,
        literal("greedy")
            .then(argument("value", ArgumentType::greedy_string()).executes(|_| Ok(1))),
    );

    let word = dispatcher.parse("word hello", ());
    assert_eq!(word.context().string("value"), Some("hello"));

    let quoted = dispatcher.parse("string \"hello world\"", ());
    assert_eq!(quoted.context().string("value"), Some("hello world"));

    let single_quoted = dispatcher.parse("string 'hello world'", ());
    assert_eq!(single_quoted.context().string("value"), Some("hello world"));

    let greedy = dispatcher.parse("greedy hello world", ());
    assert_eq!(greedy.context().string("value"), Some("hello world"));
    assert!(!greedy.reader().can_read());
}

#[test]
fn quotable_strings_preserve_reader_escape_errors() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("string").then(argument("value", ArgumentType::string())),
    );

    let parse = dispatcher.parse("string \"bad\\nvalue\"", ());

    assert_eq!(parse.reader().cursor(), 7);
    assert_eq!(parse.errors()[0].error().cursor(), Some(12));
    assert_eq!(
        parse.errors()[0].error().kind(),
        &CommandSyntaxErrorKind::InvalidEscape('n')
    );
}

#[test]
fn empty_quoted_strings_are_valid_arguments() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("string").then(argument("value", ArgumentType::string()).executes(|_| Ok(1))),
    );

    let parse = dispatcher.parse("string \"\"", ());

    assert_eq!(parse.context().string("value"), Some(""));
    assert!(parse.context().is_executable());
}

#[test]
fn differently_configured_string_arguments_collide() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("base").then(argument("value", ArgumentType::word())),
    );

    let Err(error) =
        dispatcher.register(literal("base").then(argument("value", ArgumentType::greedy_string())))
    else {
        panic!("incompatible argument parsers should collide");
    };
    assert_eq!(
        error.kind(),
        &RegistrationErrorKind::ArgumentTypeCollision {
            name: "value".into(),
        }
    );
}
