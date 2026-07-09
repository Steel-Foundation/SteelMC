use std::sync::Arc;

use steel_registry::{timeline::TimelineRef, world_clock::WorldClockRef};
use steel_utils::Identifier;

use crate::command::brigadier::{
    CommandContext, CommandNodeBuilder, CommandRuntime, CommandSyntaxError, ContextChain, NodeId,
};

use super::{
    ChainModifiers, ExecutionCommandSource, ExecutionControl, SteelArgumentType,
    argument::SteelArgumentValue,
};

/// Runtime model interpreted by Steel's tick-owned command scheduler.
pub(crate) struct SteelCommandRuntime;

pub(crate) type SteelCommandContext<S> = CommandContext<S, SteelCommandRuntime>;
pub(crate) type SteelContextChain<S> = ContextChain<S, SteelCommandRuntime>;

type StandardExecutor<S> =
    dyn Fn(&SteelCommandContext<S>) -> Result<i32, CommandSyntaxError> + Send + Sync;
type StandardModifier<S> =
    dyn Fn(&SteelCommandContext<S>) -> Result<Vec<S>, CommandSyntaxError> + Send + Sync;

/// A terminal executor stored in a Steel command graph.
pub(crate) enum SteelExecutor<S>
where
    S: ExecutionCommandSource,
{
    Standard(Box<StandardExecutor<S>>),
    Custom(Arc<dyn CustomCommandExecutor<S>>),
}

/// A redirect modifier stored in a Steel command graph.
pub(crate) enum SteelModifier<S>
where
    S: ExecutionCommandSource,
{
    Standard(Box<StandardModifier<S>>),
    Custom(Arc<dyn CustomModifierExecutor<S>>),
}

/// Special terminal behavior that controls command frames or queues more work.
pub(crate) trait CustomCommandExecutor<S>: Send + Sync
where
    S: ExecutionCommandSource,
{
    fn run(
        &self,
        source: Arc<S>,
        chain: &SteelContextChain<S>,
        modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, S>,
    );
}

/// Special redirect behavior that controls command frames or queues more work.
pub(crate) trait CustomModifierExecutor<S>: Send + Sync
where
    S: ExecutionCommandSource,
{
    fn apply(
        &self,
        original_source: Arc<S>,
        sources: Vec<Arc<S>>,
        chain: &SteelContextChain<S>,
        modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, S>,
    );
}

impl<S> CommandRuntime<S> for SteelCommandRuntime
where
    S: ExecutionCommandSource,
{
    type Argument = SteelArgumentType;
    type ArgumentValue = SteelArgumentValue;
    type Executor = SteelExecutor<S>;
    type Modifier = SteelModifier<S>;
}

/// Creates a literal backed by Steel's runtime model.
pub(crate) fn literal<S>(name: impl Into<Box<str>>) -> CommandNodeBuilder<S, SteelCommandRuntime>
where
    S: ExecutionCommandSource,
{
    CommandNodeBuilder::literal(name)
}

/// Creates an argument backed by Steel's runtime model.
pub(crate) fn argument<S>(
    name: impl Into<Box<str>>,
    argument_type: impl Into<SteelArgumentType>,
) -> CommandNodeBuilder<S, SteelCommandRuntime>
where
    S: ExecutionCommandSource,
{
    CommandNodeBuilder::argument(name, argument_type.into())
}

impl<S> CommandNodeBuilder<S, SteelCommandRuntime>
where
    S: ExecutionCommandSource,
{
    /// Attaches an ordinary synchronous executor.
    #[must_use]
    pub(crate) fn executes(
        self,
        executor: impl Fn(&SteelCommandContext<S>) -> Result<i32, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.executes_with_executor(Arc::new(SteelExecutor::Standard(Box::new(executor))))
    }

    /// Attaches an internal executor with frame and queue control.
    #[must_use]
    pub(crate) fn executes_custom(self, executor: impl CustomCommandExecutor<S> + 'static) -> Self {
        self.executes_with_executor(Arc::new(SteelExecutor::Custom(Arc::new(executor))))
    }

    /// Redirects parsing and transforms the source once before continuing.
    #[must_use]
    pub(crate) fn redirects_with(
        self,
        target: NodeId,
        modifier: impl Fn(&SteelCommandContext<S>) -> Result<S, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let modifier = SteelModifier::Standard(Box::new(move |context| {
            modifier(context).map(|source| vec![source])
        }));
        self.redirects_with_modifier(target, Arc::new(modifier), false)
    }

    /// Redirects parsing and expands one source into zero or more sources.
    #[must_use]
    pub(crate) fn forks(
        self,
        target: NodeId,
        modifier: impl Fn(&SteelCommandContext<S>) -> Result<Vec<S>, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.redirects_with_modifier(
            target,
            Arc::new(SteelModifier::Standard(Box::new(modifier))),
            true,
        )
    }

    /// Redirects with an internal modifier that controls frames or queued work.
    #[must_use]
    pub(crate) fn redirects_custom(
        self,
        target: NodeId,
        modifier: impl CustomModifierExecutor<S> + 'static,
        forks: bool,
    ) -> Self {
        self.redirects_with_modifier(
            target,
            Arc::new(SteelModifier::Custom(Arc::new(modifier))),
            forks,
        )
    }
}

impl<S> SteelCommandContext<S>
where
    S: ExecutionCommandSource,
{
    /// Returns a parsed Minecraft time argument in ticks.
    pub(crate) fn time(&self, name: &str) -> Option<i32> {
        match self.argument(name) {
            Some(SteelArgumentValue::Time(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    pub(crate) fn identifier(&self, name: &str) -> Option<&Identifier> {
        match self.argument(name) {
            Some(SteelArgumentValue::Identifier(value)) => Some(value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    pub(crate) fn world_clock(&self, name: &str) -> Option<WorldClockRef> {
        match self.argument(name) {
            Some(SteelArgumentValue::WorldClock(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    pub(crate) fn timeline(&self, name: &str) -> Option<TimelineRef> {
        match self.argument(name) {
            Some(SteelArgumentValue::Timeline(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_),
            )
            | None => None,
        }
    }
}
