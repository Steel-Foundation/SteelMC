use crate::command::{
    brigadier::{CommandDispatcher, CommandSyntaxError, CommandSyntaxErrorKind},
    execution::{
        CommandResultCallback, ExecutionCommandSource, SteelArgumentType, SteelCommandRuntime,
        argument, literal,
    },
};

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
