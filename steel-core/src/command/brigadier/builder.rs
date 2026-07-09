//! Command node builders.

use std::sync::Arc;

use super::{
    ArgumentType, BrigadierRuntime, CommandContext, CommandRequirement, CommandRuntime,
    CommandSyntaxError, NodeId, RegistrationError, RegistrationErrorKind,
    node::{CommandNodeData, CommandRedirect, UnregisteredCommandNode, merge_or_push},
    runtime::{BrigadierExecutor, BrigadierModifier},
};

/// Builds one literal or argument command node and its descendants.
pub(crate) struct CommandNodeBuilder<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    data: CommandNodeData<R::Argument>,
    children: Vec<Self>,
    executor: Option<Arc<R::Executor>>,
    requirement: CommandRequirement<S>,
    redirect: Option<CommandRedirect<S, R>>,
}

impl<S, R> Clone for CommandNodeBuilder<S, R>
where
    R: CommandRuntime<S>,
    R::Argument: Clone,
{
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            children: self.children.clone(),
            executor: self.executor.as_ref().map(Arc::clone),
            requirement: self.requirement.clone(),
            redirect: self.redirect.clone(),
        }
    }
}

/// Creates a literal using the standard synchronous Brigadier runtime.
pub(crate) fn literal<S>(name: impl Into<Box<str>>) -> CommandNodeBuilder<S> {
    CommandNodeBuilder::literal(name)
}

/// Creates an argument using the standard synchronous Brigadier runtime.
pub(crate) fn argument<S>(
    name: impl Into<Box<str>>,
    argument_type: ArgumentType,
) -> CommandNodeBuilder<S> {
    CommandNodeBuilder::argument(name, argument_type)
}

impl<S, R> CommandNodeBuilder<S, R>
where
    R: CommandRuntime<S>,
{
    /// Creates a literal for this runtime model.
    pub(crate) fn literal(name: impl Into<Box<str>>) -> Self {
        Self {
            data: CommandNodeData::Literal(name.into()),
            children: Vec::new(),
            executor: None,
            requirement: CommandRequirement::allow_all(),
            redirect: None,
        }
    }

    /// Creates an argument for this runtime model.
    pub(crate) fn argument(name: impl Into<Box<str>>, argument_type: R::Argument) -> Self {
        Self {
            data: CommandNodeData::Argument {
                name: name.into(),
                argument_type,
            },
            children: Vec::new(),
            executor: None,
            requirement: CommandRequirement::allow_all(),
            redirect: None,
        }
    }

    /// Returns this node's literal name, or `None` for an argument node.
    pub(crate) fn literal_name(&self) -> Option<&str> {
        match &self.data {
            CommandNodeData::Literal(name) => Some(name),
            CommandNodeData::Root | CommandNodeData::Argument { .. } => None,
        }
    }

    /// Replaces this node's literal name, returning `None` for an argument node.
    pub(crate) fn with_literal_name(mut self, name: impl Into<Box<str>>) -> Option<Self> {
        let CommandNodeData::Literal(literal) = &mut self.data else {
            return None;
        };
        *literal = name.into();
        Some(self)
    }

    /// Adds a child while preserving registration order.
    #[must_use]
    pub(crate) fn then(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// Attaches an executor payload without interpreting it.
    #[must_use]
    pub(crate) fn executes_with_executor(mut self, executor: Arc<R::Executor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Replaces the allow-all requirement with `requirement`.
    #[must_use]
    pub(crate) fn requires(mut self, requirement: CommandRequirement<S>) -> Self {
        self.requirement = requirement;
        self
    }

    /// Adds a requirement while preserving any predicate already on this node.
    #[must_use]
    pub(crate) fn also_requires(mut self, requirement: CommandRequirement<S>) -> Self
    where
        S: 'static,
    {
        self.requirement = self.requirement.and(requirement);
        self
    }

    /// Redirects parsing to an existing node without transforming the source.
    #[must_use]
    pub(crate) fn redirects(mut self, target: NodeId) -> Self {
        self.redirect = Some(CommandRedirect::identity(target));
        self
    }

    /// Redirects with an opaque runtime modifier payload.
    #[must_use]
    pub(crate) fn redirects_with_modifier(
        mut self,
        target: NodeId,
        modifier: Arc<R::Modifier>,
        forks: bool,
    ) -> Self {
        self.redirect = Some(CommandRedirect::with_modifier(target, modifier, forks));
        self
    }

    pub(super) fn normalize(self) -> Result<UnregisteredCommandNode<S, R>, RegistrationError> {
        let mut children = Vec::new();
        for child in self.children {
            merge_or_push(&mut children, child.normalize()?)?;
        }
        if self.redirect.is_some() && !children.is_empty() {
            return Err(RegistrationError::new(
                RegistrationErrorKind::RedirectWithChildren {
                    name: self.data.name().into(),
                },
            ));
        }

        Ok(UnregisteredCommandNode {
            data: self.data,
            children,
            executor: self.executor,
            requirement: self.requirement,
            redirect: self.redirect,
        })
    }
}

impl<S> CommandNodeBuilder<S, BrigadierRuntime> {
    /// Attaches a standard synchronous command callback.
    #[must_use]
    pub(crate) fn executes(
        self,
        executor: impl Fn(&CommandContext<S>) -> Result<i32, CommandSyntaxError> + Send + Sync + 'static,
    ) -> Self {
        let executor: Arc<BrigadierExecutor<S>> = Arc::new(executor);
        self.executes_with_executor(executor)
    }

    /// Redirects parsing and transforms the source once before continuing.
    #[must_use]
    pub(crate) fn redirects_with(
        self,
        target: NodeId,
        modifier: impl Fn(&CommandContext<S>) -> Result<S, CommandSyntaxError> + Send + Sync + 'static,
    ) -> Self {
        let modifier: Arc<BrigadierModifier<S>> =
            Arc::new(move |context| modifier(context).map(|source| vec![source]));
        self.redirects_with_modifier(target, modifier, false)
    }

    /// Redirects parsing and expands one source into zero or more sources.
    #[must_use]
    pub(crate) fn forks(
        self,
        target: NodeId,
        modifier: impl Fn(&CommandContext<S>) -> Result<Vec<S>, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let modifier: Arc<BrigadierModifier<S>> = Arc::new(modifier);
        self.redirects_with_modifier(target, modifier, true)
    }
}
