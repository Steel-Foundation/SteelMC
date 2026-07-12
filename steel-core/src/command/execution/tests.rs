use std::sync::Arc;

use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use crate::command::PendingCommandExecutionQueue;
use crate::command::brigadier::{
    ArgumentType, CommandDispatcher, CommandNodeBuilder, CommandSyntaxError, NodeId,
};
use crate::command::sender::CommandSenderKey;
use crate::permission::{PermissionExpr, PermissionState};

use super::{
    ChainModifiers, CommandArgumentSource, CommandExecutionContext, CommandPermissionSource,
    CommandResultCallback, CommandResultSuspension, CommandResultSuspensionPoll, CommandSuspension,
    CommandSuspensionOrder, CommandSuspensionPoll, CustomCommandExecutor, CustomModifierExecutor,
    EntryAction, ExecutionCommandSource, ExecutionControl, ExecutionStop, Frame,
    SteelCommandRuntime, SteelContextChain, argument, literal,
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

impl CommandArgumentSource for TestSource {}

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
        control.discard_frame();
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
    let discarded_observed = Arc::clone(&observed);
    register(
        &mut dispatcher,
        literal::<TestSource>("discarded").executes(move |context| {
            discarded_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(1)
        }),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("returning").redirects_custom(root, ReturningModifier, false),
    );
    let returning_chain = chain(&dispatcher, "returning run", Arc::clone(&observed));
    let discarded_chain = chain(&dispatcher, "discarded", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        returning_chain,
        TestSource::new("runtime", Arc::clone(&observed)),
        CommandResultCallback::new(move |success, result| {
            callback_results.lock().push((success, result));
        }),
    );
    execution.queue_initial_command(
        discarded_chain,
        TestSource::new("discarded", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(true, 5)]);
    assert_eq!(*frame_results.lock(), [(true, 5)]);
    assert!(observed.invocations.lock().is_empty());
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

struct CompleteSuspensionAction {
    invocation: &'static str,
    result: i32,
    observed: Arc<Observed>,
}

impl EntryAction<TestSource> for CompleteSuspensionAction {
    fn execute(self: Box<Self>, _context: &mut CommandExecutionContext<TestSource>, frame: Frame) {
        self.observed.invocations.lock().push(self.invocation);
        frame.return_success(self.result);
    }
}

struct TestSuspension {
    pending_polls: usize,
    invocation: &'static str,
    result: i32,
    observed: Arc<Observed>,
    cancellations: Arc<SyncMutex<usize>>,
}

struct TestResultSuspension {
    pending_polls: usize,
    result: Option<Result<i32, CommandSyntaxError>>,
    cancellations: Arc<SyncMutex<usize>>,
}

struct GlobalResultSuspension(TestResultSuspension);

impl CommandResultSuspension for GlobalResultSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        self.0.poll()
    }

    fn cancel(&mut self) {
        self.0.cancel();
    }
}

impl CommandResultSuspension for TestResultSuspension {
    fn poll(&mut self) -> CommandResultSuspensionPoll {
        if self.pending_polls > 0 {
            self.pending_polls -= 1;
            return CommandResultSuspensionPoll::Pending;
        }

        let Some(result) = self.result.take() else {
            panic!("a completed result suspension must not be polled again");
        };
        CommandResultSuspensionPoll::Ready(result)
    }

    fn cancel(&mut self) {
        *self.cancellations.lock() += 1;
    }
}

#[test]
fn suspended_normal_executor_reports_its_delayed_result() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let command_observed = Arc::clone(&observed);
    let command_cancellations = Arc::clone(&cancellations);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_suspended(move |context| {
            command_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(TestResultSuspension {
                pending_polls: 1,
                result: Some(Ok(42)),
                cancellations: Arc::clone(&command_cancellations),
            })
        }),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "wait", Arc::clone(&observed)),
        TestSource::new("waiting", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    assert_eq!(*observed.invocations.lock(), ["waiting"]);
    assert!(observed.results.lock().is_empty());

    assert_eq!(execution.poll_suspension(), ExecutionStop::Suspended);
    assert_eq!(execution.poll_suspension(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(true, 42)]);
    assert!(observed.errors.lock().is_empty());
    assert_eq!(*cancellations.lock(), 0);
}

#[test]
fn suspended_normal_executor_is_retained_at_the_sequence_limit() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let command_cancellations = Arc::clone(&cancellations);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_suspended(move |_| {
            Ok(TestResultSuspension {
                pending_polls: 0,
                result: Some(Ok(42)),
                cancellations: Arc::clone(&command_cancellations),
            })
        }),
    );
    let mut execution = CommandExecutionContext::new(1, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "wait", Arc::clone(&observed)),
        TestSource::new("waiting", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    assert!(observed.results.lock().is_empty());

    assert_eq!(execution.poll_suspension(), ExecutionStop::CommandLimit);
    assert_eq!(*observed.results.lock(), [(true, 42)]);
    assert_eq!(*cancellations.lock(), 0);
}

#[test]
fn suspended_normal_executor_reports_delayed_errors_like_a_standard_executor() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let command_cancellations = Arc::clone(&cancellations);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_suspended(move |_| {
            Ok(TestResultSuspension {
                pending_polls: 0,
                result: Some(Err(CommandSyntaxError::dynamic("delayed failure"))),
                cancellations: Arc::clone(&command_cancellations),
            })
        }),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "wait", Arc::clone(&observed)),
        TestSource::new("waiting", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    assert_eq!(execution.poll_suspension(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(false, 0)]);
    assert_eq!(
        *observed.errors.lock(),
        [("delayed failure".to_owned(), false)]
    );
    assert_eq!(*cancellations.lock(), 0);
}

#[test]
fn suspended_normal_executor_reports_startup_errors_without_suspending() {
    let observed = Arc::new(Observed::default());
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_suspended(
            |_| -> Result<TestResultSuspension, CommandSyntaxError> {
                Err(CommandSyntaxError::dynamic("startup failure"))
            },
        ),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "wait", Arc::clone(&observed)),
        TestSource::new("waiting", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(false, 0)]);
    assert_eq!(
        *observed.errors.lock(),
        [("startup failure".to_owned(), false)]
    );
}

#[test]
fn suspended_normal_executor_resumes_each_forked_source_and_suppresses_errors() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let command_observed = Arc::clone(&observed);
    let command_cancellations = Arc::clone(&cancellations);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_suspended(move |context| {
            command_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(TestResultSuspension {
                pending_polls: 0,
                result: Some(Err(CommandSyntaxError::dynamic("forked failure"))),
                cancellations: Arc::clone(&command_cancellations),
            })
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
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "fork wait", Arc::clone(&observed)),
        TestSource::new("original", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    assert_eq!(*observed.invocations.lock(), ["first"]);
    assert_eq!(execution.poll_suspension(), ExecutionStop::Suspended);
    assert_eq!(*observed.invocations.lock(), ["first", "second"]);
    assert_eq!(execution.poll_suspension(), ExecutionStop::Completed);

    assert_eq!(*observed.results.lock(), [(false, 0), (false, 0)]);
    assert!(observed.errors.lock().is_empty());
    assert_eq!(*cancellations.lock(), 0);
}

#[test]
fn suspended_normal_executor_preserves_forked_return_semantics() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let frame_results = Arc::new(SyncMutex::new(Vec::new()));
    let callback_results = Arc::clone(&frame_results);
    let command_observed = Arc::clone(&observed);
    let command_cancellations = Arc::clone(&cancellations);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_suspended(move |context| {
            command_observed
                .invocations
                .lock()
                .push(context.source().name);
            Ok(TestResultSuspension {
                pending_polls: 0,
                result: Some(Ok(7)),
                cancellations: Arc::clone(&command_cancellations),
            })
        }),
    );
    let root = dispatcher.root();
    register(
        &mut dispatcher,
        literal::<TestSource>("returning").redirects_custom(root, ReturningModifier, false),
    );
    register(
        &mut dispatcher,
        literal::<TestSource>("fork").forks(root, |context| {
            Ok(vec![
                context.source().with_name("first"),
                context.source().with_name("second"),
            ])
        }),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "fork returning wait", Arc::clone(&observed)),
        TestSource::new("original", Arc::clone(&observed)),
        CommandResultCallback::new(move |success, result| {
            callback_results.lock().push((success, result));
        }),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    assert_eq!(*observed.invocations.lock(), ["first"]);
    assert_eq!(execution.poll_suspension(), ExecutionStop::Completed);
    assert_eq!(*observed.results.lock(), [(true, 7)]);
    assert_eq!(*frame_results.lock(), [(true, 7)]);
    assert_eq!(*cancellations.lock(), 0);
}

#[test]
fn cancelling_suspended_normal_execution_cancels_its_work() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let command_cancellations = Arc::clone(&cancellations);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_suspended(move |_| {
            Ok(TestResultSuspension {
                pending_polls: usize::MAX,
                result: Some(Ok(1)),
                cancellations: Arc::clone(&command_cancellations),
            })
        }),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "wait", Arc::clone(&observed)),
        TestSource::new("waiting", observed),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    execution.cancel();

    assert_eq!(*cancellations.lock(), 1);
    assert_eq!(execution.run(), ExecutionStop::Completed);
}

impl CommandSuspension<TestSource> for TestSuspension {
    fn poll(&mut self) -> CommandSuspensionPoll<TestSource> {
        if self.pending_polls > 0 {
            self.pending_polls -= 1;
            return CommandSuspensionPoll::Pending;
        }

        CommandSuspensionPoll::resume(CompleteSuspensionAction {
            invocation: self.invocation,
            result: self.result,
            observed: Arc::clone(&self.observed),
        })
    }

    fn cancel(&mut self) {
        *self.cancellations.lock() += 1;
    }
}

struct SuspendingExecutor {
    pending_polls: usize,
    invocation: &'static str,
    result: i32,
    observed: Arc<Observed>,
    cancellations: Arc<SyncMutex<usize>>,
}

impl CustomCommandExecutor<TestSource> for SuspendingExecutor {
    fn run(
        &self,
        _source: Arc<TestSource>,
        _chain: &SteelContextChain<TestSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, TestSource>,
    ) {
        control.suspend(TestSuspension {
            pending_polls: self.pending_polls,
            invocation: self.invocation,
            result: self.result,
            observed: Arc::clone(&self.observed),
            cancellations: Arc::clone(&self.cancellations),
        });
    }
}

#[test]
fn suspended_execution_resumes_in_queue_order_with_the_original_frame() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let frame_results = Arc::new(SyncMutex::new(Vec::new()));
    let callback_results = Arc::clone(&frame_results);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_custom(SuspendingExecutor {
            pending_polls: 1,
            invocation: "resumed",
            result: 42,
            observed: Arc::clone(&observed),
            cancellations: Arc::clone(&cancellations),
        }),
    );
    let normal_observed = Arc::clone(&observed);
    register(
        &mut dispatcher,
        literal::<TestSource>("normal").executes(move |_| {
            normal_observed.invocations.lock().push("normal");
            Ok(1)
        }),
    );
    let waiting = chain(&dispatcher, "wait", Arc::clone(&observed));
    let normal = chain(&dispatcher, "normal", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        waiting,
        TestSource::new("waiting", Arc::clone(&observed)),
        CommandResultCallback::new(move |success, result| {
            callback_results.lock().push((success, result));
        }),
    );
    execution.queue_initial_command(
        normal,
        TestSource::new("normal", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    assert!(observed.invocations.lock().is_empty());
    assert!(frame_results.lock().is_empty());

    assert_eq!(execution.poll_suspension(), ExecutionStop::Suspended);
    assert!(observed.invocations.lock().is_empty());

    assert_eq!(execution.poll_suspension(), ExecutionStop::Completed);
    assert_eq!(*observed.invocations.lock(), ["resumed", "normal"]);
    assert_eq!(*frame_results.lock(), [(true, 42)]);
    assert_eq!(*cancellations.lock(), 0);
}

struct QueueTwoSuspensions {
    observed: Arc<Observed>,
    first_cancellations: Arc<SyncMutex<usize>>,
    second_cancellations: Arc<SyncMutex<usize>>,
}

impl CustomCommandExecutor<TestSource> for QueueTwoSuspensions {
    fn run(
        &self,
        _source: Arc<TestSource>,
        _chain: &SteelContextChain<TestSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, TestSource>,
    ) {
        control.suspend(TestSuspension {
            pending_polls: usize::MAX,
            invocation: "first",
            result: 1,
            observed: Arc::clone(&self.observed),
            cancellations: Arc::clone(&self.first_cancellations),
        });
        control.suspend(TestSuspension {
            pending_polls: usize::MAX,
            invocation: "second",
            result: 2,
            observed: Arc::clone(&self.observed),
            cancellations: Arc::clone(&self.second_cancellations),
        });
    }
}

#[test]
fn cancelling_execution_cancels_active_and_queued_suspensions() {
    let observed = Arc::new(Observed::default());
    let first_cancellations = Arc::new(SyncMutex::new(0));
    let second_cancellations = Arc::new(SyncMutex::new(0));
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_custom(QueueTwoSuspensions {
            observed: Arc::clone(&observed),
            first_cancellations: Arc::clone(&first_cancellations),
            second_cancellations: Arc::clone(&second_cancellations),
        }),
    );
    let waiting = chain(&dispatcher, "wait", Arc::clone(&observed));
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        waiting,
        TestSource::new("waiting", observed),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    execution.cancel();

    assert_eq!(*first_cancellations.lock(), 1);
    assert_eq!(*second_cancellations.lock(), 1);
    assert_eq!(execution.run(), ExecutionStop::Completed);
}

struct QueueReadyThenPendingSuspension {
    observed: Arc<Observed>,
    first_cancellations: Arc<SyncMutex<usize>>,
    second_cancellations: Arc<SyncMutex<usize>>,
}

struct ReturnFrameAction {
    result: i32,
}

impl EntryAction<TestSource> for ReturnFrameAction {
    fn execute(self: Box<Self>, context: &mut CommandExecutionContext<TestSource>, frame: Frame) {
        ExecutionControl::new(context, frame).return_success(self.result);
    }
}

struct ReturningSuspension {
    result: i32,
    cancellations: Arc<SyncMutex<usize>>,
}

impl CommandSuspension<TestSource> for ReturningSuspension {
    fn poll(&mut self) -> CommandSuspensionPoll<TestSource> {
        CommandSuspensionPoll::resume(ReturnFrameAction {
            result: self.result,
        })
    }

    fn cancel(&mut self) {
        *self.cancellations.lock() += 1;
    }
}

impl CustomCommandExecutor<TestSource> for QueueReadyThenPendingSuspension {
    fn run(
        &self,
        _source: Arc<TestSource>,
        _chain: &SteelContextChain<TestSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, TestSource>,
    ) {
        control.suspend(ReturningSuspension {
            result: 1,
            cancellations: Arc::clone(&self.first_cancellations),
        });
        control.suspend(TestSuspension {
            pending_polls: usize::MAX,
            invocation: "second",
            result: 2,
            observed: Arc::clone(&self.observed),
            cancellations: Arc::clone(&self.second_cancellations),
        });
    }
}

#[test]
fn frame_return_cancels_discarded_suspension_work() {
    let observed = Arc::new(Observed::default());
    let first_cancellations = Arc::new(SyncMutex::new(0));
    let second_cancellations = Arc::new(SyncMutex::new(0));
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_custom(QueueReadyThenPendingSuspension {
            observed: Arc::clone(&observed),
            first_cancellations: Arc::clone(&first_cancellations),
            second_cancellations: Arc::clone(&second_cancellations),
        }),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "wait", Arc::clone(&observed)),
        TestSource::new("waiting", observed),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::Suspended);
    assert_eq!(execution.poll_suspension(), ExecutionStop::Completed);

    assert_eq!(*first_cancellations.lock(), 0);
    assert_eq!(*second_cancellations.lock(), 1);
}

struct SuspensionOverflowExecutor {
    observed: Arc<Observed>,
    cancellations: Arc<SyncMutex<usize>>,
}

impl CustomCommandExecutor<TestSource> for SuspensionOverflowExecutor {
    fn run(
        &self,
        _source: Arc<TestSource>,
        _chain: &SteelContextChain<TestSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, TestSource>,
    ) {
        control.suspend(TestSuspension {
            pending_polls: usize::MAX,
            invocation: "never",
            result: 1,
            observed: Arc::clone(&self.observed),
            cancellations: Arc::clone(&self.cancellations),
        });
        for _ in 0..3 {
            control.queue_next(NoopAction);
        }
    }
}

#[test]
fn queue_overflow_cancels_queued_suspension_work() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("overflow").executes_custom(SuspensionOverflowExecutor {
            observed: Arc::clone(&observed),
            cancellations: Arc::clone(&cancellations),
        }),
    );
    let mut execution = CommandExecutionContext::with_queue_limit(10, 10, 1);
    execution.queue_initial_command(
        chain(&dispatcher, "overflow", Arc::clone(&observed)),
        TestSource::new("overflow", observed),
        CommandResultCallback::empty(),
    );

    assert_eq!(execution.run(), ExecutionStop::QueueOverflow);
    assert_eq!(*cancellations.lock(), 1);
}

#[test]
fn pending_execution_queue_polls_once_per_tick_in_fifo_order() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("first").executes_custom(SuspendingExecutor {
            pending_polls: 0,
            invocation: "first",
            result: 1,
            observed: Arc::clone(&observed),
            cancellations: Arc::clone(&cancellations),
        }),
    );
    register(
        &mut dispatcher,
        literal::<TestSource>("second").executes_custom(SuspendingExecutor {
            pending_polls: 1,
            invocation: "second",
            result: 2,
            observed: Arc::clone(&observed),
            cancellations: Arc::clone(&cancellations),
        }),
    );

    let mut first = CommandExecutionContext::new(10, 10);
    first.queue_initial_command(
        chain(&dispatcher, "first", Arc::clone(&observed)),
        TestSource::new("first", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );
    assert_eq!(first.run(), ExecutionStop::Suspended);

    let mut second = CommandExecutionContext::new(10, 10);
    second.queue_initial_command(
        chain(&dispatcher, "second", Arc::clone(&observed)),
        TestSource::new("second", Arc::clone(&observed)),
        CommandResultCallback::empty(),
    );
    assert_eq!(second.run(), ExecutionStop::Suspended);

    let mut queue = PendingCommandExecutionQueue::new();
    assert!(queue.push_suspended(CommandSenderKey::Console, first));
    assert!(queue.push_suspended(CommandSenderKey::Rcon, second));
    assert!(queue.blocks(CommandSenderKey::Console));
    assert!(queue.blocks(CommandSenderKey::Rcon));

    let first_tick = queue.tick(2);
    assert_eq!(first_tick.polled, 2);
    assert_eq!(first_tick.finished, 1);
    assert_eq!(first_tick.pending, 1);
    assert_eq!(*observed.invocations.lock(), ["first"]);
    assert!(!queue.blocks(CommandSenderKey::Console));
    assert!(queue.blocks(CommandSenderKey::Rcon));

    let second_tick = queue.tick(2);
    assert_eq!(second_tick.polled, 1);
    assert_eq!(second_tick.finished, 1);
    assert_eq!(second_tick.pending, 0);
    assert_eq!(*observed.invocations.lock(), ["first", "second"]);
    assert_eq!(*cancellations.lock(), 0);
    assert!(!queue.blocks(CommandSenderKey::Rcon));
}

#[test]
fn global_suspension_blocks_every_command_source_until_completion() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let command_cancellations = Arc::clone(&cancellations);
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("global").executes_suspended(move |_| {
            Ok(GlobalResultSuspension(TestResultSuspension {
                pending_polls: 1,
                result: Some(Ok(1)),
                cancellations: Arc::clone(&command_cancellations),
            }))
        }),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "global", Arc::clone(&observed)),
        TestSource::new("global", observed),
        CommandResultCallback::empty(),
    );
    assert_eq!(execution.run(), ExecutionStop::Suspended);

    let mut queue = PendingCommandExecutionQueue::new();
    assert!(queue.push_suspended(CommandSenderKey::Console, execution));
    assert!(queue.blocks(CommandSenderKey::Console));
    assert!(queue.blocks(CommandSenderKey::Rcon));

    assert_eq!(queue.tick(1).pending, 1);
    assert!(queue.blocks(CommandSenderKey::Rcon));
    assert_eq!(queue.tick(1).pending, 0);
    assert!(!queue.blocks(CommandSenderKey::Console));
    assert!(!queue.blocks(CommandSenderKey::Rcon));
    assert_eq!(*cancellations.lock(), 0);
}

#[test]
fn pending_execution_queue_rejects_active_contexts() {
    let mut queue = PendingCommandExecutionQueue::<TestSource>::new();
    let execution = CommandExecutionContext::new(10, 10);

    assert!(!queue.push_suspended(CommandSenderKey::Console, execution));
    assert_eq!(queue.len(), 0);
}

#[test]
fn pending_execution_queue_cancels_retained_work() {
    let observed = Arc::new(Observed::default());
    let cancellations = Arc::new(SyncMutex::new(0));
    let mut dispatcher = TestDispatcher::new();
    register(
        &mut dispatcher,
        literal::<TestSource>("wait").executes_custom(SuspendingExecutor {
            pending_polls: usize::MAX,
            invocation: "resumed",
            result: 1,
            observed: Arc::clone(&observed),
            cancellations: Arc::clone(&cancellations),
        }),
    );
    let mut execution = CommandExecutionContext::new(10, 10);
    execution.queue_initial_command(
        chain(&dispatcher, "wait", Arc::clone(&observed)),
        TestSource::new("waiting", observed),
        CommandResultCallback::empty(),
    );
    assert_eq!(execution.run(), ExecutionStop::Suspended);

    let mut queue = PendingCommandExecutionQueue::new();
    assert!(queue.push_suspended(CommandSenderKey::Console, execution));
    queue.cancel_all();

    assert_eq!(queue.len(), 0);
    assert!(!queue.blocks(CommandSenderKey::Console));
    assert_eq!(*cancellations.lock(), 1);
}
