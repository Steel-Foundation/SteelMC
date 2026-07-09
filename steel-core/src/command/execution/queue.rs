use std::{collections::VecDeque, sync::Arc};

use crate::command::brigadier::{CommandSyntaxError, ContextChainStage};

use super::{
    CommandResultCallback, ExecutionCommandSource, SteelContextChain, SteelExecutor, SteelModifier,
};

const MAX_COMMAND_QUEUE_DEPTH: usize = 10_000_000;

/// Flags accumulated while traversing a command context chain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ChainModifiers(u8);

impl ChainModifiers {
    const FORKED: u8 = 1;
    const RETURN: u8 = 2;

    pub(crate) const fn is_forked(self) -> bool {
        self.0 & Self::FORKED != 0
    }

    pub(crate) const fn is_return(self) -> bool {
        self.0 & Self::RETURN != 0
    }

    pub(crate) const fn with_forked(self) -> Self {
        Self(self.0 | Self::FORKED)
    }

    pub(crate) const fn with_return(self) -> Self {
        Self(self.0 | Self::RETURN)
    }
}

/// Why a command queue stopped running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExecutionStop {
    Completed,
    CommandLimit,
    QueueOverflow,
}

#[derive(Clone)]
pub(crate) struct Frame {
    depth: usize,
    return_value_consumer: CommandResultCallback,
    discard: FrameDiscard,
}

#[derive(Clone, Copy)]
enum FrameDiscard {
    All,
    AtOrAbove(usize),
}

impl Frame {
    pub(crate) const fn depth(&self) -> usize {
        self.depth
    }

    pub(crate) fn return_success(&self, value: i32) {
        self.return_value_consumer.on_result(true, value);
    }

    pub(crate) fn return_failure(&self) {
        self.return_value_consumer.on_result(false, 0);
    }
}

pub(crate) trait EntryAction<S>: 'static
where
    S: ExecutionCommandSource,
{
    fn execute(self: Box<Self>, context: &mut CommandExecutionContext<S>, frame: Frame);
}

struct CommandQueueEntry<S>
where
    S: ExecutionCommandSource,
{
    frame: Frame,
    action: Box<dyn EntryAction<S>>,
}

/// Vanilla-style, tick-local command action queue.
pub(crate) struct CommandExecutionContext<S>
where
    S: ExecutionCommandSource,
{
    command_limit: usize,
    fork_limit: usize,
    queue_limit: usize,
    command_quota: usize,
    queue_overflow: bool,
    command_queue: VecDeque<CommandQueueEntry<S>>,
    new_top_commands: Vec<CommandQueueEntry<S>>,
    current_frame_depth: usize,
}

impl<S> CommandExecutionContext<S>
where
    S: ExecutionCommandSource,
{
    pub(crate) fn new(command_limit: usize, fork_limit: usize) -> Self {
        let command_limit = command_limit.max(1);
        Self {
            command_limit,
            fork_limit,
            queue_limit: MAX_COMMAND_QUEUE_DEPTH,
            command_quota: command_limit,
            queue_overflow: false,
            command_queue: VecDeque::new(),
            new_top_commands: Vec::new(),
            current_frame_depth: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn with_queue_limit(
        command_limit: usize,
        fork_limit: usize,
        queue_limit: usize,
    ) -> Self {
        let mut context = Self::new(command_limit, fork_limit);
        context.queue_limit = queue_limit;
        context
    }

    pub(crate) fn queue_initial_command(
        &mut self,
        chain: SteelContextChain<S>,
        source: S,
        return_value_consumer: CommandResultCallback,
    ) {
        let source = Arc::new(source);
        let frame = self.create_top_frame(return_value_consumer);
        self.queue_entry(CommandQueueEntry {
            frame,
            action: Box::new(BuildContextsAction {
                chain,
                original_source: Arc::clone(&source),
                sources: vec![source],
                modifiers: ChainModifiers::default(),
            }),
        });
    }

    pub(crate) fn run(&mut self) -> ExecutionStop {
        self.push_new_commands();
        let stop = loop {
            if self.command_quota == 0 {
                log::info!(
                    "Command execution stopped due to limit (executed {} commands)",
                    self.command_limit
                );
                break ExecutionStop::CommandLimit;
            }

            let Some(entry) = self.command_queue.pop_front() else {
                break ExecutionStop::Completed;
            };
            self.current_frame_depth = entry.frame.depth;
            entry.action.execute(self, entry.frame);
            if self.queue_overflow {
                log::error!(
                    "Command execution stopped due to command queue overflow (max {})",
                    self.queue_limit
                );
                break ExecutionStop::QueueOverflow;
            }
            self.push_new_commands();
        };
        self.current_frame_depth = 0;
        stop
    }

    pub(crate) const fn fork_limit(&self) -> usize {
        self.fork_limit
    }

    pub(crate) const fn increment_cost(&mut self) {
        self.command_quota = self.command_quota.saturating_sub(1);
    }

    const fn create_top_frame(&self, return_value_consumer: CommandResultCallback) -> Frame {
        if self.current_frame_depth == 0 {
            return Frame {
                depth: 0,
                return_value_consumer,
                discard: FrameDiscard::All,
            };
        }

        let depth = self.current_frame_depth + 1;
        Frame {
            depth,
            return_value_consumer,
            discard: FrameDiscard::AtOrAbove(depth),
        }
    }

    fn queue_next(&mut self, frame: Frame, action: impl EntryAction<S>) {
        self.queue_entry(CommandQueueEntry {
            frame,
            action: Box::new(action),
        });
    }

    fn queue_boxed(&mut self, frame: Frame, action: Box<dyn EntryAction<S>>) {
        self.queue_entry(CommandQueueEntry { frame, action });
    }

    fn queue_entry(&mut self, entry: CommandQueueEntry<S>) {
        if self
            .new_top_commands
            .len()
            .saturating_add(self.command_queue.len())
            > self.queue_limit
        {
            self.queue_overflow = true;
            self.new_top_commands.clear();
            self.command_queue.clear();
        }
        if !self.queue_overflow {
            self.new_top_commands.push(entry);
        }
    }

    fn push_new_commands(&mut self) {
        while let Some(command) = self.new_top_commands.pop() {
            self.command_queue.push_front(command);
        }
    }

    fn discard(&mut self, frame: &Frame) {
        match frame.discard {
            FrameDiscard::All => self.command_queue.clear(),
            FrameDiscard::AtOrAbove(depth) => {
                while self
                    .command_queue
                    .front()
                    .is_some_and(|entry| entry.frame.depth >= depth)
                {
                    self.command_queue.pop_front();
                }
            }
        }
    }
}

/// Queue and frame operations available only to custom internal executors.
pub(crate) struct ExecutionControl<'context, S>
where
    S: ExecutionCommandSource,
{
    context: &'context mut CommandExecutionContext<S>,
    frame: Frame,
}

impl<'context, S> ExecutionControl<'context, S>
where
    S: ExecutionCommandSource,
{
    const fn new(context: &'context mut CommandExecutionContext<S>, frame: Frame) -> Self {
        Self { context, frame }
    }

    pub(crate) const fn current_frame(&self) -> &Frame {
        &self.frame
    }

    pub(crate) fn queue_next(&mut self, action: impl EntryAction<S>) {
        self.context.queue_next(self.frame.clone(), action);
    }

    pub(crate) fn queue_contexts(
        &mut self,
        chain: SteelContextChain<S>,
        original_source: Arc<S>,
        sources: Vec<Arc<S>>,
        modifiers: ChainModifiers,
    ) {
        self.context.queue_next(
            self.frame.clone(),
            BuildContextsAction {
                chain,
                original_source,
                sources,
                modifiers,
            },
        );
    }

    pub(crate) fn return_success(&mut self, result: i32) {
        self.frame.return_success(result);
        self.context.discard(&self.frame);
    }

    pub(crate) fn return_failure(&mut self) {
        self.frame.return_failure();
        self.context.discard(&self.frame);
    }
}

struct BuildContextsAction<S>
where
    S: ExecutionCommandSource,
{
    chain: SteelContextChain<S>,
    original_source: Arc<S>,
    sources: Vec<Arc<S>>,
    modifiers: ChainModifiers,
}

impl<S> EntryAction<S> for BuildContextsAction<S>
where
    S: ExecutionCommandSource,
{
    fn execute(self: Box<Self>, context: &mut CommandExecutionContext<S>, frame: Frame) {
        let Self {
            mut chain,
            original_source,
            mut sources,
            mut modifiers,
        } = *self;

        while chain.stage() == ContextChainStage::Modify {
            if chain.top_context().is_forked() {
                modifiers = modifiers.with_forked();
            }

            match chain.top_context().modifier() {
                Some(SteelModifier::Custom(modifier)) => {
                    let mut control = ExecutionControl::new(context, frame);
                    modifier.apply(original_source, sources, &chain, modifiers, &mut control);
                    return;
                }
                Some(SteelModifier::Standard(modifier)) => {
                    context.increment_cost();
                    let mut next_sources = Vec::new();
                    for source in sources {
                        let command_context = chain.top_context().copy_for(Arc::clone(&source));
                        let new_sources = match modifier(&command_context) {
                            Ok(sources) => sources,
                            Err(error) => {
                                if modifiers.is_forked() {
                                    continue;
                                }
                                source.handle_error(&error, false);
                                return;
                            }
                        };
                        if next_sources.len().saturating_add(new_sources.len())
                            >= context.fork_limit()
                        {
                            let error = CommandSyntaxError::dynamic(format!(
                                "Command fork limit reached ({})",
                                context.fork_limit()
                            ));
                            original_source.handle_error(&error, modifiers.is_forked());
                            return;
                        }
                        next_sources.extend(new_sources.into_iter().map(Arc::new));
                    }
                    sources = next_sources;
                }
                None => {}
            }

            let Some(next_stage) = chain.next_stage() else {
                unreachable!("a modifying command stage always has a following stage")
            };
            chain = next_stage;
        }

        if sources.is_empty() {
            if modifiers.is_return() {
                context.queue_next(frame, FallthroughAction);
            }
            return;
        }

        let Some(executor) = chain.top_context().executor() else {
            unreachable!("a context chain's final stage is always executable")
        };
        match executor {
            SteelExecutor::Custom(executor) => {
                for source in sources {
                    let mut control = ExecutionControl::new(context, frame.clone());
                    executor.run(source, &chain, modifiers, &mut control);
                }
            }
            SteelExecutor::Standard(_) => {
                if modifiers.is_return() {
                    let Some(source) = sources.into_iter().next() else {
                        unreachable!("empty source lists return before terminal scheduling")
                    };
                    let callback = CommandResultCallback::chain(
                        source.callback(),
                        frame.return_value_consumer.clone(),
                    );
                    let source = Arc::new(source.with_callback(callback));
                    schedule_executions(context, frame, chain, vec![source], modifiers);
                } else {
                    schedule_executions(context, frame, chain, sources, modifiers);
                }
            }
        }
    }
}

fn schedule_executions<S>(
    context: &mut CommandExecutionContext<S>,
    frame: Frame,
    chain: SteelContextChain<S>,
    sources: Vec<Arc<S>>,
    modifiers: ChainModifiers,
) where
    S: ExecutionCommandSource,
{
    match sources.len() {
        0 => {}
        1 | 2 => {
            for source in sources {
                context.queue_next(
                    frame.clone(),
                    ExecuteAction {
                        chain: chain.clone(),
                        source,
                        modifiers,
                    },
                );
            }
        }
        _ => context.queue_next(
            frame,
            ExecuteContinuation {
                chain,
                sources: sources.into(),
                modifiers,
            },
        ),
    }
}

struct ExecuteContinuation<S>
where
    S: ExecutionCommandSource,
{
    chain: SteelContextChain<S>,
    sources: VecDeque<Arc<S>>,
    modifiers: ChainModifiers,
}

impl<S> EntryAction<S> for ExecuteContinuation<S>
where
    S: ExecutionCommandSource,
{
    fn execute(mut self: Box<Self>, context: &mut CommandExecutionContext<S>, frame: Frame) {
        let Some(source) = self.sources.pop_front() else {
            return;
        };
        context.queue_next(
            frame.clone(),
            ExecuteAction {
                chain: self.chain.clone(),
                source,
                modifiers: self.modifiers,
            },
        );
        if !self.sources.is_empty() {
            context.queue_boxed(frame, self);
        }
    }
}

struct ExecuteAction<S>
where
    S: ExecutionCommandSource,
{
    chain: SteelContextChain<S>,
    source: Arc<S>,
    modifiers: ChainModifiers,
}

impl<S> EntryAction<S> for ExecuteAction<S>
where
    S: ExecutionCommandSource,
{
    fn execute(self: Box<Self>, context: &mut CommandExecutionContext<S>, _frame: Frame) {
        context.increment_cost();
        let command_context = self.chain.top_context().copy_for(Arc::clone(&self.source));
        let Some(SteelExecutor::Standard(executor)) = command_context.executor() else {
            unreachable!("only standard executors are scheduled as execute actions")
        };
        match executor(&command_context) {
            Ok(result) => command_context.source().callback().on_result(true, result),
            Err(error) => {
                command_context.source().callback().on_result(false, 0);
                if !self.modifiers.is_forked() {
                    self.source.handle_error(&error, false);
                }
            }
        }
    }
}

struct FallthroughAction;

impl<S> EntryAction<S> for FallthroughAction
where
    S: ExecutionCommandSource,
{
    fn execute(self: Box<Self>, context: &mut CommandExecutionContext<S>, frame: Frame) {
        frame.return_failure();
        context.discard(&frame);
    }
}
