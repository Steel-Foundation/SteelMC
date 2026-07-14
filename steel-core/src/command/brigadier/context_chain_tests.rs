use std::sync::Arc;

use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use super::{
    ArgumentType, CommandContext, CommandDispatcher, CommandNodeBuilder, CommandSyntaxError,
    CommandSyntaxErrorKind, ContextChain, ContextChainStage, NodeId, argument, literal,
};

fn register(
    dispatcher: &mut CommandDispatcher<String>,
    builder: CommandNodeBuilder<String>,
) -> NodeId {
    let Ok(node) = dispatcher.register(builder) else {
        panic!("command registration should succeed");
    };
    node
}

fn context_chain(
    dispatcher: &CommandDispatcher<String>,
    input: &str,
    source: &str,
) -> ContextChain<String> {
    let parse = dispatcher.parse(input, source.to_owned());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("complete executable input should produce a context chain");
    };
    chain
}

fn no_result(_: &CommandContext<String>, _: bool, _: i32) {}

#[test]
fn executable_context_uses_runtime_source_and_parsed_arguments() {
    let invocations = Arc::new(SyncMutex::new(Vec::new()));
    let command_invocations = Arc::clone(&invocations);
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("add").then(argument("amount", ArgumentType::integer(0, 10)).executes(
            move |context: &CommandContext<String>| {
                command_invocations.lock().push((
                    context.source().to_owned(),
                    context.integer("amount"),
                    context.input().to_owned(),
                ));
                Ok(4)
            },
        )),
    );
    let chain = context_chain(&dispatcher, "add 3", "parse source");

    let result = chain.execute_all("runtime source".to_owned(), &no_result);

    assert_eq!(result, Ok(4));
    assert_eq!(
        *invocations.lock(),
        [("runtime source".to_owned(), Some(3), "add 3".to_owned())]
    );
}

#[test]
fn executable_context_exposes_all_primitive_arguments_and_metadata() {
    let observed = Arc::new(SyncMutex::new(false));
    let command_observed = Arc::clone(&observed);
    let mut dispatcher = CommandDispatcher::new();
    let root = dispatcher.root();
    let input = "values true -7 1.5 2.5 'hello world'";
    register(
        &mut dispatcher,
        literal("values").then(argument("boolean", ArgumentType::bool()).then(
            argument("long", ArgumentType::long(i64::MIN, i64::MAX)).then(
                argument("float", ArgumentType::float(f32::MIN, f32::MAX)).then(
                    argument("double", ArgumentType::double(f64::MIN, f64::MAX)).then(
                        argument("string", ArgumentType::string()).executes(
                            move |context: &CommandContext<String>| {
                                assert_eq!(context.root(), root);
                                assert_eq!(context.range().start(), 0);
                                assert_eq!(context.boolean("boolean"), Some(true));
                                assert_eq!(context.long("long"), Some(-7));
                                assert_eq!(context.float("float"), Some(1.5));
                                assert_eq!(context.double("double"), Some(2.5));
                                assert_eq!(context.string("string"), Some("hello world"));
                                assert!(context.child().is_none());
                                *command_observed.lock() = true;
                                Ok(1)
                            },
                        ),
                    ),
                ),
            ),
        )),
    );
    let chain = context_chain(&dispatcher, input, "parse source");

    assert_eq!(chain.execute_all("runtime".to_owned(), &no_result), Ok(1));
    assert!(*observed.lock());
}

#[test]
fn identity_redirects_form_distinct_context_chain_stages() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("run").executes(|_| Ok(1)));
    let root = dispatcher.root();
    register(&mut dispatcher, literal("again").redirects(root));
    let chain = context_chain(&dispatcher, "again again run", "source");

    assert_eq!(chain.stage(), ContextChainStage::Modify);
    assert_eq!(chain.top_context().nodes().len(), 1);
    let Some(second) = chain.next_stage() else {
        panic!("first redirect should have a following stage");
    };
    assert_eq!(second.stage(), ContextChainStage::Modify);
    let Some(terminal) = second.next_stage() else {
        panic!("second redirect should have a following stage");
    };
    assert_eq!(terminal.stage(), ContextChainStage::Execute);
    assert!(terminal.next_stage().is_none());
    assert_eq!(chain.execute_all("runtime".to_owned(), &no_result), Ok(1));
}

#[test]
fn redirect_modifier_replaces_the_runtime_source() {
    let sources = Arc::new(SyncMutex::new(Vec::new()));
    let command_sources = Arc::clone(&sources);
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("run").executes(move |context: &CommandContext<String>| {
            command_sources.lock().push(context.source().to_owned());
            Ok(7)
        }),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal("redirect").redirects_with(root, |context| {
            Ok(format!("{} redirected", context.source()))
        }),
    );
    let chain = context_chain(&dispatcher, "redirect run", "parse source");

    assert_eq!(chain.execute_all("runtime".to_owned(), &no_result), Ok(7));
    assert_eq!(*sources.lock(), ["runtime redirected"]);
}

#[test]
fn forked_execution_counts_successes_instead_of_command_results() {
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let consumer_events = Arc::clone(&events);
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("run").executes(|_| Ok(9)));
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal("fork").forks(root, |_| Ok(vec!["first".to_owned(), "second".to_owned()])),
    );
    let chain = context_chain(&dispatcher, "fork run", "parse source");

    let result = chain.execute_all("runtime".to_owned(), &move |context, success, result| {
        consumer_events
            .lock()
            .push((context.source().to_owned(), success, result));
    });

    assert_eq!(result, Ok(2));
    assert_eq!(
        *events.lock(),
        [
            ("first".to_owned(), true, 9),
            ("second".to_owned(), true, 9),
        ]
    );
}

#[test]
fn forked_command_errors_are_reported_and_suppressed() {
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let consumer_events = Arc::clone(&events);
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("run").executes(|context| {
            if context.source() == "first" {
                return Err(CommandSyntaxError::dynamic(TextComponent::const_plain(
                    "failed",
                )));
            }
            Ok(5)
        }),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal("fork").forks(root, |_| Ok(vec!["first".to_owned(), "second".to_owned()])),
    );
    let chain = context_chain(&dispatcher, "fork run", "parse source");

    let result = chain.execute_all("runtime".to_owned(), &move |context, success, result| {
        consumer_events
            .lock()
            .push((context.source().to_owned(), success, result));
    });

    assert_eq!(result, Ok(1));
    assert_eq!(
        *events.lock(),
        [
            ("first".to_owned(), false, 0),
            ("second".to_owned(), true, 5),
        ]
    );
}

#[test]
fn fork_modifier_errors_are_suppressed_before_the_terminal_stage() {
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let consumer_events = Arc::clone(&events);
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("run").executes(|_| Ok(1)));
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal("fork").forks(root, |_| {
            Err(CommandSyntaxError::dynamic(TextComponent::const_plain(
                "failed",
            )))
        }),
    );
    let chain = context_chain(&dispatcher, "fork run", "parse source");

    let result = chain.execute_all("runtime".to_owned(), &move |context, success, result| {
        consumer_events
            .lock()
            .push((context.source().to_owned(), success, result));
    });

    assert_eq!(result, Ok(0));
    assert_eq!(*events.lock(), [("runtime".to_owned(), false, 0)]);
}

#[test]
fn non_forked_command_errors_are_reported_and_propagated() {
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let consumer_events = Arc::clone(&events);
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("fail").executes(|_| {
            Err(CommandSyntaxError::dynamic(TextComponent::const_plain(
                "failed",
            )))
        }),
    );
    let chain = context_chain(&dispatcher, "fail", "parse source");

    let result = chain.execute_all("runtime".to_owned(), &move |context, success, result| {
        consumer_events
            .lock()
            .push((context.source().to_owned(), success, result));
    });

    assert!(matches!(
        result,
        Err(error) if matches!(error.kind(), CommandSyntaxErrorKind::Dynamic(_))
    ));
    assert_eq!(*events.lock(), [("runtime".to_owned(), false, 0)]);
}

#[test]
fn incomplete_input_does_not_build_an_executable_chain() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("parent").then(literal("child").executes(|_| Ok(1))),
    );
    let parse = dispatcher.parse("parent", "source".to_owned());

    let result = dispatcher.context_chain(parse);

    assert!(matches!(
        result,
        Err(error) if error.kind() == &CommandSyntaxErrorKind::UnknownCommand
    ));
}

#[test]
fn parse_finalization_distinguishes_unknown_commands_and_arguments() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("known").executes(|_| Ok(1)));

    let unknown = dispatcher.parse("missing", "source".to_owned());
    assert!(matches!(
        dispatcher.context_chain(unknown),
        Err(error) if error.kind() == &CommandSyntaxErrorKind::UnknownCommand
    ));

    let trailing = dispatcher.parse("known trailing", "source".to_owned());
    assert!(matches!(
        dispatcher.context_chain(trailing),
        Err(error) if error.kind() == &CommandSyntaxErrorKind::UnknownArgument
    ));
}

#[test]
fn parse_finalization_preserves_a_single_specific_parser_error() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("set").then(argument("value", ArgumentType::integer(0, 10)).executes(|_| Ok(1))),
    );
    let parse = dispatcher.parse("set 11", "source".to_owned());

    assert!(matches!(
        dispatcher.context_chain(parse),
        Err(error)
            if error.kind()
                == &CommandSyntaxErrorKind::IntegerTooHigh {
                    found: 11,
                    maximum: 10,
                }
    ));
}
