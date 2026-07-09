//! Immutable Brigadier redirect and execution stages.

use std::sync::Arc;

use super::{BrigadierRuntime, CommandContext, CommandRuntime, CommandSyntaxError};

pub(crate) type CommandResultConsumer<S> = dyn Fn(&CommandContext<S, BrigadierRuntime>, bool, i32);

/// Whether the current context transforms sources or runs a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextChainStage {
    Modify,
    Execute,
}

/// A flattened sequence of redirect contexts followed by one executable context.
pub(crate) struct ContextChain<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    contexts: Arc<[Arc<CommandContext<S, R>>]>,
    position: usize,
}

impl<S, R> ContextChain<S, R>
where
    R: CommandRuntime<S>,
{
    pub(super) fn try_flatten(root: Arc<CommandContext<S, R>>) -> Option<Self> {
        let mut contexts = Vec::new();
        let mut current = root;
        loop {
            let child = current.child_arc().map(Arc::clone);
            contexts.push(current);
            let Some(child) = child else {
                break;
            };
            current = child;
        }

        if contexts
            .last()
            .is_none_or(|context| context.executor().is_none())
        {
            return None;
        }
        Some(Self {
            contexts: contexts.into(),
            position: 0,
        })
    }

    /// Returns the kind of work represented by the current stage.
    pub(crate) fn stage(&self) -> ContextChainStage {
        if self.position + 1 == self.contexts.len() {
            ContextChainStage::Execute
        } else {
            ContextChainStage::Modify
        }
    }

    /// Returns the parsed context at the current stage.
    pub(crate) fn top_context(&self) -> &CommandContext<S, R> {
        &self.contexts[self.position]
    }

    /// Advances to the next redirect or executable stage.
    pub(crate) fn next_stage(&self) -> Option<Self> {
        let position = self.position + 1;
        (position < self.contexts.len()).then(|| Self {
            contexts: Arc::clone(&self.contexts),
            position,
        })
    }
}

impl<S> ContextChain<S, BrigadierRuntime> {
    /// Applies the current stage's source modifier.
    pub(crate) fn run_modifier(
        &self,
        source: Arc<S>,
        consumer: &CommandResultConsumer<S>,
        forked: bool,
    ) -> Result<Vec<Arc<S>>, CommandSyntaxError> {
        let template = self.top_context();
        let Some(modifier) = template.modifier() else {
            return Ok(vec![source]);
        };
        let context = template.copy_for(source);
        match modifier(&context) {
            Ok(sources) => Ok(sources.into_iter().map(Arc::new).collect()),
            Err(error) => {
                consumer(&context, false, 0);
                if forked { Ok(Vec::new()) } else { Err(error) }
            }
        }
    }

    /// Runs the current stage's terminal command.
    pub(crate) fn run_executable(
        &self,
        source: Arc<S>,
        consumer: &CommandResultConsumer<S>,
        forked: bool,
    ) -> Result<i32, CommandSyntaxError> {
        let context = self.top_context().copy_for(source);
        let Some(executor) = context.executor() else {
            unreachable!("a context chain's final stage is always executable")
        };
        match executor(&context) {
            Ok(result) => {
                consumer(&context, true, result);
                Ok(if forked { 1 } else { result })
            }
            Err(error) => {
                consumer(&context, false, 0);
                if forked { Ok(0) } else { Err(error) }
            }
        }
    }

    /// Executes the complete chain with Brigadier's synchronous semantics.
    pub(crate) fn execute_all(
        &self,
        source: S,
        consumer: &CommandResultConsumer<S>,
    ) -> Result<i32, CommandSyntaxError> {
        let mut stage = self.clone();
        let mut forked = false;
        let mut sources = vec![Arc::new(source)];

        while stage.stage() == ContextChainStage::Modify {
            forked |= stage.top_context().is_forked();
            let mut next_sources = Vec::new();
            for source in sources {
                next_sources.extend(stage.run_modifier(source, consumer, forked)?);
            }
            if next_sources.is_empty() {
                return Ok(0);
            }
            sources = next_sources;
            let Some(next_stage) = stage.next_stage() else {
                unreachable!("a modifying context chain stage always has a following stage")
            };
            stage = next_stage;
        }

        let mut result = 0_i32;
        for source in sources {
            result = result.wrapping_add(stage.run_executable(source, consumer, forked)?);
        }
        Ok(result)
    }
}

impl<S, R> Clone for ContextChain<S, R>
where
    R: CommandRuntime<S>,
{
    fn clone(&self) -> Self {
        Self {
            contexts: Arc::clone(&self.contexts),
            position: self.position,
        }
    }
}
