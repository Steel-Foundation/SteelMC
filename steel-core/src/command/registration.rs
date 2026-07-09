//! Stable command identities and collision-aware dispatcher construction.

use std::iter::once;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::Identifier;
use thiserror::Error;

use super::{
    brigadier::{CommandDispatcher, CommandNodeBuilder, NodeId, RegistrationError},
    execution::{ExecutionCommandSource, SteelCommandRuntime},
};

type CommandFactory<S> = dyn FnOnce(NodeId) -> CommandNodeBuilder<S, SteelCommandRuntime> + 'static;

/// One complete command tree and its stable owner identity.
pub(crate) struct CommandRegistration<S>
where
    S: ExecutionCommandSource,
{
    id: Identifier,
    aliases: Vec<Box<str>>,
    factory: Box<CommandFactory<S>>,
}

impl<S> CommandRegistration<S>
where
    S: ExecutionCommandSource,
{
    /// Declares a command whose factory receives the target dispatcher's root.
    pub(crate) fn new(
        id: Identifier,
        factory: impl FnOnce(NodeId) -> CommandNodeBuilder<S, SteelCommandRuntime> + 'static,
    ) -> Self {
        Self {
            id,
            aliases: Vec::new(),
            factory: Box::new(factory),
        }
    }

    /// Adds a fixed unqualified alias owned by this command.
    #[must_use]
    pub(crate) fn alias(mut self, alias: impl Into<Box<str>>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    fn validate(&self) -> Result<(), CommandRegistrationError> {
        if self.id.namespace.is_empty()
            || self.id.path.is_empty()
            || !Identifier::validate(&self.id.namespace, &self.id.path)
        {
            return Err(CommandRegistrationError::InvalidCommandId(self.id.clone()));
        }

        let mut roots = FxHashSet::default();
        roots.insert(self.id.path.as_ref());
        for alias in &self.aliases {
            validate_alias(alias)?;
            if !roots.insert(alias.as_ref()) {
                return Err(CommandRegistrationError::DuplicateOwnedRoot {
                    id: self.id.clone(),
                    root: alias.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Collects declarations before atomically constructing a dispatcher.
pub(crate) struct CommandDispatcherBuilder<S>
where
    S: ExecutionCommandSource,
{
    registrations: Vec<CommandRegistration<S>>,
    ids: FxHashSet<Identifier>,
}

impl<S> CommandDispatcherBuilder<S>
where
    S: ExecutionCommandSource,
{
    pub(crate) fn new() -> Self {
        Self {
            registrations: Vec::new(),
            ids: FxHashSet::default(),
        }
    }

    /// Adds one declaration. Earlier declarations win unqualified collisions.
    pub(crate) fn register(
        &mut self,
        registration: CommandRegistration<S>,
    ) -> Result<(), CommandRegistrationError> {
        registration.validate()?;
        if !self.ids.insert(registration.id.clone()) {
            return Err(CommandRegistrationError::DuplicateCommandId(
                registration.id,
            ));
        }
        self.registrations.push(registration);
        Ok(())
    }

    /// Builds the complete graph without exposing a partially registered dispatcher.
    pub(crate) fn build(
        self,
    ) -> Result<CommandDispatcher<S, SteelCommandRuntime>, CommandRegistrationError> {
        let mut dispatcher = CommandDispatcher::new();
        let dispatcher_root = dispatcher.root();
        let mut resolved = Vec::with_capacity(self.registrations.len());

        for registration in self.registrations {
            let root = (registration.factory)(dispatcher_root);
            let Some(root_name) = root.literal_name() else {
                return Err(CommandRegistrationError::RootMustBeLiteral {
                    id: registration.id,
                });
            };
            if root_name != registration.id.path {
                return Err(CommandRegistrationError::RootDoesNotMatchId {
                    id: registration.id,
                    root: root_name.into(),
                });
            }
            resolved.push(ResolvedCommand {
                id: registration.id,
                aliases: registration.aliases,
                root,
            });
        }

        let mut claim_counts = FxHashMap::<Box<str>, usize>::default();
        for command in &resolved {
            for root in command.roots() {
                *claim_counts.entry(root.into()).or_default() += 1;
            }
        }

        let mut claimed_roots = FxHashSet::<Box<str>>::default();
        for command in &resolved {
            for root in command.roots() {
                if !claimed_roots.insert(root.into()) {
                    continue;
                }
                register_renamed_root(&mut dispatcher, &command.root, root)?;
            }
        }

        for command in &resolved {
            let collided = command
                .roots()
                .any(|root| claim_counts.get(root).is_some_and(|count| *count > 1));
            if collided {
                register_renamed_root(&mut dispatcher, &command.root, command.id.to_string())?;
            }
        }

        Ok(dispatcher)
    }
}

impl<S> Default for CommandDispatcherBuilder<S>
where
    S: ExecutionCommandSource,
{
    fn default() -> Self {
        Self::new()
    }
}

struct ResolvedCommand<S>
where
    S: ExecutionCommandSource,
{
    id: Identifier,
    aliases: Vec<Box<str>>,
    root: CommandNodeBuilder<S, SteelCommandRuntime>,
}

impl<S> ResolvedCommand<S>
where
    S: ExecutionCommandSource,
{
    fn roots(&self) -> impl Iterator<Item = &str> {
        once(self.id.path.as_ref()).chain(self.aliases.iter().map(AsRef::as_ref))
    }
}

fn register_renamed_root<S>(
    dispatcher: &mut CommandDispatcher<S, SteelCommandRuntime>,
    root: &CommandNodeBuilder<S, SteelCommandRuntime>,
    name: impl Into<Box<str>>,
) -> Result<(), CommandRegistrationError>
where
    S: ExecutionCommandSource,
{
    let renamed = root
        .clone()
        .with_literal_name(name)
        .ok_or(CommandRegistrationError::UnexpectedArgumentRoot)?;
    dispatcher.register(renamed)?;
    Ok(())
}

fn validate_alias(alias: &str) -> Result<(), CommandRegistrationError> {
    if alias.is_empty() {
        return Err(CommandRegistrationError::EmptyAlias);
    }
    if alias.chars().any(char::is_whitespace) {
        return Err(CommandRegistrationError::AliasContainsWhitespace(
            alias.into(),
        ));
    }
    if alias.contains(':') {
        return Err(CommandRegistrationError::NamespacedAlias(alias.into()));
    }
    Ok(())
}

/// A command declaration or its resulting Brigadier graph was invalid.
#[derive(Debug, Error)]
pub(crate) enum CommandRegistrationError {
    #[error("invalid command id '{0}'")]
    InvalidCommandId(Identifier),
    #[error("command id '{0}' is already registered")]
    DuplicateCommandId(Identifier),
    #[error("command '{id}' claims root '{root}' more than once")]
    DuplicateOwnedRoot { id: Identifier, root: Box<str> },
    #[error("command '{id}' must produce a literal root")]
    RootMustBeLiteral { id: Identifier },
    #[error("command '{id}' produced root '{root}' instead of its id path")]
    RootDoesNotMatchId { id: Identifier, root: Box<str> },
    #[error("command alias cannot be empty")]
    EmptyAlias,
    #[error("command alias '{0}' cannot contain whitespace")]
    AliasContainsWhitespace(Box<str>),
    #[error("command alias '{0}' cannot be namespaced")]
    NamespacedAlias(Box<str>),
    #[error("a validated command root unexpectedly became an argument")]
    UnexpectedArgumentRoot,
    #[error(transparent)]
    InvalidGraph(#[from] RegistrationError),
}

#[cfg(test)]
mod tests;
