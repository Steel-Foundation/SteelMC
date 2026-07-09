//! Command node builders.

use std::sync::Arc;

use super::{
    ArgumentType, CommandContext, CommandRequirement, CommandSyntaxError, NodeId,
    RegistrationError, RegistrationErrorKind,
    node::{
        Command, CommandNodeData, CommandRedirect, RedirectModifier, UnregisteredCommandNode,
        merge_or_push,
    },
};

/// Builds one literal or argument command node and its descendants.
pub(crate) struct CommandNodeBuilder<S> {
    data: CommandNodeData,
    children: Vec<Self>,
    command: Option<Command<S>>,
    requirement: CommandRequirement<S>,
    redirect: Option<CommandRedirect<S>>,
}

/// Creates a literal command node builder.
pub(crate) fn literal<S>(name: impl Into<Box<str>>) -> CommandNodeBuilder<S> {
    CommandNodeBuilder {
        data: CommandNodeData::Literal(name.into()),
        children: Vec::new(),
        command: None,
        requirement: CommandRequirement::allow_all(),
        redirect: None,
    }
}

/// Creates an argument command node builder.
pub(crate) fn argument<S>(
    name: impl Into<Box<str>>,
    argument_type: ArgumentType,
) -> CommandNodeBuilder<S> {
    CommandNodeBuilder {
        data: CommandNodeData::Argument {
            name: name.into(),
            argument_type,
        },
        children: Vec::new(),
        command: None,
        requirement: CommandRequirement::allow_all(),
        redirect: None,
    }
}

impl<S> CommandNodeBuilder<S> {
    /// Adds a child while preserving registration order.
    #[must_use]
    pub(crate) fn then(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// Attaches a synchronous command callback.
    #[must_use]
    pub(crate) fn executes(
        mut self,
        command: impl Fn(&CommandContext<S>) -> Result<i32, CommandSyntaxError> + Send + Sync + 'static,
    ) -> Self {
        self.command = Some(Arc::new(command));
        self
    }

    /// Replaces the allow-all requirement with `requirement`.
    #[must_use]
    pub(crate) fn requires(mut self, requirement: CommandRequirement<S>) -> Self {
        self.requirement = requirement;
        self
    }

    /// Redirects parsing to an existing node in the same dispatcher.
    #[must_use]
    pub(crate) fn redirects(mut self, target: NodeId) -> Self {
        self.redirect = Some(CommandRedirect::identity(target));
        self
    }

    /// Redirects parsing and transforms the source once before continuing.
    #[must_use]
    pub(crate) fn redirects_with(
        mut self,
        target: NodeId,
        modifier: impl Fn(&CommandContext<S>) -> Result<S, CommandSyntaxError> + Send + Sync + 'static,
    ) -> Self {
        let modifier: RedirectModifier<S> =
            Arc::new(move |context| modifier(context).map(|source| vec![source]));
        self.redirect = Some(CommandRedirect::single(target, modifier));
        self
    }

    /// Redirects parsing and expands one source into zero or more sources.
    #[must_use]
    pub(crate) fn forks(
        mut self,
        target: NodeId,
        modifier: impl Fn(&CommandContext<S>) -> Result<Vec<S>, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let modifier: RedirectModifier<S> = Arc::new(modifier);
        self.redirect = Some(CommandRedirect::forked(target, modifier));
        self
    }

    pub(super) fn normalize(self) -> Result<UnregisteredCommandNode<S>, RegistrationError> {
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
            command: self.command,
            requirement: self.requirement,
            redirect: self.redirect,
        })
    }
}
