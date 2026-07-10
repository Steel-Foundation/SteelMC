use std::sync::Arc;

use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use crate::command::brigadier::{
    ArgumentType, CommandDispatcher, CommandNodeBuilder, CommandSyntaxError, NodeId,
};
use crate::permission::{PermissionExpr, PermissionState};

use super::queue::{EntryAction, Frame};
use super::{
    ChainModifiers, CommandExecutionContext, CommandPermissionSource, CommandResultCallback,
    CustomCommandExecutor, CustomModifierExecutor, ExecutionCommandSource, ExecutionControl,
    ExecutionStop, SteelCommandRuntime, SteelContextChain, argument, literal,
};

#[derive(Default)]
struct Observed {
    invocations: SyncMutex<Vec<&'static str>>,
    results: SyncMutex<Vec<(bool, i32)>>,
    errors: SyncMutex<Vec<(String, bool)>>,
}

struct TestSource {
    name: &'static str,
    callback: CommandResultCallback,
    observed: Arc<Observed>,
}

impl TestSource {
    fn new(name: &'static str, observed: Arc<Observed>) -> Self {
        let callback_observed = Arc::clone(&observed);
        Self {
            name,
            callback: CommandResultCallback::new(move |success, result| {
                callback_observed.results.lock().push((success, result));
            }),
            observed,
        }
    }

    fn with_name(&self, name: &'static str) -> Self {
        Self {
            name,
            callback: self.callback.clone(),
            observed: Arc::clone(&self.observed),
        }
    }
}

impl ExecutionCommandSource for TestSource {
    fn with_callback(&self, callback: CommandResultCallback) -> Self {
        Self {
            name: self.name,
            callback,
            observed: Arc::clone(&self.observed),
        }
    }

    fn callback(&self) -> CommandResultCallback {
        self.callback.clone()
    }

    fn handle_error(&self, error: &CommandSyntaxError, forked: bool) {
        self.observed
            .errors
            .lock()
            .push((error.raw_message(), forked));
    }
}

impl CommandPermissionSource for TestSource {
    fn permission_state(&self, _permission: &PermissionExpr) -> Option<PermissionState> {
        Some(PermissionState::Allow)
    }
}

type TestDispatcher = CommandDispatcher<TestSource, SteelCommandRuntime>;

fn register(
    dispatcher: &mut TestDispatcher,
    builder: CommandNodeBuilder<TestSource, SteelCommandRuntime>,
) -> NodeId {
    let Ok(node) = dispatcher.register(builder) else {
        panic!("command registration should succeed");
    };
    node
}

fn chain(
    dispatcher: &TestDispatcher,
    input: &str,
    observed: Arc<Observed>,
) -> SteelContextChain<TestSource> {
    let parse = dispatcher.parse(input, TestSource::new("parse", observed));
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("complete input should produce an executable context chain");
    };
    chain
}

#[test]
fn queue_runs_standard_commands_with_the_runtime_source_callback() {
    let observed = Arc::new(Observed::default());
    let command_observed = Arc::clone(&observed);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("run").then(
            argument::<TestSource>("value", ArgumentType::integer(0, 10)).executes(
                move |context| {
                    command_observed
                        .invocations
                        .lock()
                        .push(context.source().name);
                    let Some(value) = context.integer("value") else {
                        panic!("parsed integer should be available to the executor");
                    };
                    Ok(value)
                },
            ),
        ),
    );
    let chain = chain(&dispatcher, "run 7", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Completed);
    assert_eq!(*observed.invocations.lock(), ["runtime"]);
    assert_eq!(*observed.results.lock(), [(true, 7)]);
    assert!(observed.errors.lock().is_empty());
}

#[test]
fn command_limit_stops_before_the_next_queued_action() {
    let observed = Arc::new(Observed::default());
    let command_observed = Arc::clone(&observed);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("run").executes(move |context| {
            command_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(1)
        }),
    );
    let first = chain(&dispatcher, "run", Arc::clone(&observed));
    let second = first.clone();
    let mut execution = CommandExecutionContext::new(1, 10);
    execution.queue_initial_command(
        first,
        TestSource::new("first", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );
    execution.queue_initial_command(
        second,
        TestSource::new("second", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::CommandLimit);
    assert_eq!(*observed.invocations.lock(), ["first"]);
}

#[test]
fn forked_sources_execute_in_order() {
    let observed = Arc::new(Observed::default());
    let command_observed = Arc::clone(&observed);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("run").executes(move |context| {
            command_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(9)
        }),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("fork").forks(root, |context| {
            Ok(vec![
                context.source().with_name("first"),
                context.source().with_name("second"),
            ])
        }),
    );
    let chain = chain(&dispatcher, "fork run", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 3);
    execution.queue_initial_command(
        chain,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Completed);
    assert_eq!(*observed.invocations.lock(), ["first", "second"]);
    assert_eq!(*observed.results.lock(), [(true, 9), (true, 9)]);
}

#[test]
fn standard_modifiers_consume_one_sequence_cost() {
    let observed = Arc::new(Observed::default());
    let command_observed = Arc::clone(&observed);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("run").executes(move |context| {
            command_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(1)
        }),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("redirect")
            .redirects_with(root, |context| Ok(context.source().with_name("redirected"))),
    );
    let chain = chain(&dispatcher, "redirect run", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(1, 10);
    execution.queue_initial_command(
        chain,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::CommandLimit);
    assert!(observed.invocations.lock().is_empty());
}

#[test]
fn fork_limit_uses_vanillas_exclusive_boundary() {
    let observed = Arc::new(Observed::default());
    let command_observed = Arc::clone(&observed);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("run").executes(move |context| {
            command_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(1)
        }),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("fork").forks(root, |context| {
            Ok(vec![
                context.source().with_name("first"),
                context.source().with_name("second"),
            ])
        }),
    );
    let chain = chain(&dispatcher, "fork run", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 2);
    execution.queue_initial_command(
        chain,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Completed);
    assert!(observed.invocations.lock().is_empty());
    assert_eq!(
        *observed.errors.lock(),
        [("Command fork limit reached (2)".to_owned(), true)]
    );
}

#[test]
fn modifier_failures_follow_fork_suppression_rules() {
    let observed = Arc::new(Observed::default());
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("run").executes(|_| Ok(1)),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("redirect").redirects_with(root, |_| {
            Err(CommandSyntaxError::dynamic(TextComponent::const_plain(
                "redirect failed",
            )))
        }),
    );
    register(
        &mut dispatcher,
        literal::<TestSource>("fork").forks(root, |_| {
            Err(CommandSyntaxError::dynamic(TextComponent::const_plain(
                "fork failed",
            )))
        }),
    );

    let redirect = chain(&dispatcher, "redirect run", Arc::clone(&observed));
    let mut redirect_execution = CommandExecutionContext::new(10, 10);
    redirect_execution.queue_initial_command(
        redirect,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );
    assert_eq!(redirect_execution.run(), ExecutionStop::Completed);
    assert_eq!(
        *observed.errors.lock(),
        [("redirect failed".to_owned(), false)]
    );

    observed.errors.lock().clear();
    let fork = chain(&dispatcher, "fork run", Arc::clone(&observed));
    let mut fork_execution = CommandExecutionContext::new(10, 10);
    fork_execution.queue_initial_command(
        fork,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );
    assert_eq!(fork_execution.run(), ExecutionStop::Completed);
    assert!(observed.errors.lock().is_empty());
}

#[test]
fn terminal_failures_invoke_callbacks_but_only_non_forks_report_errors() {
    let observed = Arc::new(Observed::default());
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("fail").executes(|_| {
            Err(CommandSyntaxError::dynamic(TextComponent::const_plain(
                "command failed",
            )))
        }),
    );
    let direct = chain(&dispatcher, "fail", Arc::clone(&observed));
    let mut direct_execution = CommandExecutionContext::new(10, 10);
    direct_execution.queue_initial_command(
        direct,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );
    assert_eq!(direct_execution.run(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(false, 0)]);
    assert_eq!(
        *observed.errors.lock(),
        [("command failed".to_owned(), false)]
    );

    observed.results.lock().clear();
    observed.errors.lock().clear();
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("fork").forks(root, |context| {
            Ok(vec![context.source().with_name("forked")])
        }),
    );
    let fork = chain(&dispatcher, "fork fail", Arc::clone(&observed));
    let mut fork_execution = CommandExecutionContext::new(10, 10);
    fork_execution.queue_initial_command(
        fork,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );
    assert_eq!(fork_execution.run(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(false, 0)]);
    assert!(observed.errors.lock().is_empty());
}

struct FrameReturnExecutor {
    result: Option<i32>,
    depths: Arc<SyncMutex<Vec<usize>>>,
}

impl CustomCommandExecutor<TestSource> for FrameReturnExecutor {
    fn run(
        &self,
        _source: Arc<TestSource>,
        _chain: &SteelContextChain<TestSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, TestSource>,
    ) {
        self.depths.lock().push(control.current_frame().depth());
        if let Some(result) = self.result {
            control.return_success(result);
        } else {
            control.return_failure();
        }
    }
}

#[test]
fn custom_executor_returns_from_its_frame_and_discards_queued_work() {
    let observed = Arc::new(Observed::default());
    let frame_results = Arc::new(SyncMutex::new(Vec::new()));
    let callback_results = Arc::clone(&frame_results);
    let depths = Arc::new(SyncMutex::new(Vec::new()));
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("return").executes_custom(FrameReturnExecutor {
            result: Some(42),
            depths: Arc::clone(&depths),
        }),
    );
    let normal_observed = Arc::clone(&observed);
    register(
        &mut dispatcher,
        literal::<TestSource>("normal").executes(move |context| {
            normal_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(1)
        }),
    );
    let returning = chain(&dispatcher, "return", Arc::clone(&observed));
    let normal = chain(&dispatcher, "normal", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        returning,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::new(move |success, result| {
            callback_results.lock().push((success, result));
        }),
    );
    execution.queue_initial_command(
        normal,
        TestSource::new("discarded", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Completed);
    assert_eq!(*frame_results.lock(), [(true, 42)]);
    assert_eq!(*depths.lock(), [0]);
    assert!(observed.invocations.lock().is_empty());
}

struct ReturningModifier;

impl CustomModifierExecutor<TestSource> for ReturningModifier {
    fn apply(
        &self,
        original_source: Arc<TestSource>,
        sources: Vec<Arc<TestSource>>,
        chain: &SteelContextChain<TestSource>,
        modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, TestSource>,
    ) {
        let Some(next_stage) = chain.next_stage() else {
            panic!("custom redirect should have a following stage");
        };
        control.queue_contexts(
            next_stage,
            original_source,
            sources,
            modifiers.with_return(),
        );
    }
}

#[test]
fn custom_modifier_can_continue_with_return_propagation() {
    let observed = Arc::new(Observed::default());
    let frame_results = Arc::new(SyncMutex::new(Vec::new()));
    let callback_results = Arc::clone(&frame_results);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("run").executes(|_| Ok(5)),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("returning").redirects_custom(root, ReturningModifier, false),
    );
    let chain = chain(&dispatcher, "returning run", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::new(move |success, result| {
            callback_results.lock().push((success, result));
        }),
    );

    assert_eq!(execution.run(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(true, 5)]);
    assert_eq!(*frame_results.lock(), [(true, 5)]);
}

struct NoopAction;

impl EntryAction<TestSource> for NoopAction {
    fn execute(self: Box<Self>, _context: &mut CommandExecutionContext<TestSource>, _frame: Frame) {
    }
}

struct OverflowExecutor;

impl CustomCommandExecutor<TestSource> for OverflowExecutor {
    fn run(
        &self,
        _source: Arc<TestSource>,
        _chain: &SteelContextChain<TestSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, TestSource>,
    ) {
        for _ in 0..4 {
            control.queue_next(NoopAction);
        }
    }
}

#[test]
fn queue_overflow_stops_work_queued_by_a_custom_executor() {
    let observed = Arc::new(Observed::default());
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("overflow").executes_custom(OverflowExecutor),
    );
    let chain = chain(&dispatcher, "overflow", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::with_queue_limit(10, 10, 1);
    execution.queue_initial_command(
        chain,
        TestSource::new("runtime", observed),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::QueueOverflow);
}
