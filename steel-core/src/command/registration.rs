//! Stable command identities and collision-aware dispatcher construction.

use std::iter::once;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::Identifier;
use thiserror::Error;

use super::{
    brigadier::{
        CommandDispatcher, CommandNodeBuilder, CommandRequirement, NodeId, RegistrationError,
    },
    execution::{CommandPermissionSource, SteelCommandRuntime},
};
use crate::permission::{PermissionExpr, PermissionKey, PermissionKeyError, PermissionState};

type CommandFactory<S> = dyn FnOnce(NodeId) -> CommandNodeBuilder<S, SteelCommandRuntime> + 'static;

/// One complete command tree and its stable owner identity.
pub(crate) struct CommandRegistration<S>
where
    S: CommandPermissionSource,
{
    id: Identifier,
    aliases: Vec<Box<str>>,
    permission: Option<PermissionExpr>,
    default_access: bool,
    factory: Box<CommandFactory<S>>,
}

impl<S> CommandRegistration<S>
where
    S: CommandPermissionSource,
{
    /// Declares a command whose factory receives the target dispatcher's root.
    pub(crate) fn new(
        id: Identifier,
        factory: impl FnOnce(NodeId) -> CommandNodeBuilder<S, SteelCommandRuntime> + 'static,
    ) -> Self {
        Self {
            id,
            aliases: Vec::new(),
            permission: None,
            default_access: false,
            factory: Box::new(factory),
        }
    }

    /// Adds a fixed unqualified alias owned by this command.
    #[must_use]
    pub(crate) fn alias(mut self, alias: impl Into<Box<str>>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Allows an unset root permission while still respecting an explicit deny.
    #[must_use]
    pub(crate) const fn default_access(mut self) -> Self {
        self.default_access = true;
        self
    }

    /// Replaces the permission expression derived from this command's ID.
    #[must_use]
    pub(crate) fn permission(mut self, permission: PermissionExpr) -> Self {
        self.permission = Some(permission);
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
    S: CommandPermissionSource,
{
    registrations: Vec<CommandRegistration<S>>,
    ids: FxHashSet<Identifier>,
}

impl<S> CommandDispatcherBuilder<S>
where
    S: CommandPermissionSource,
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
            let permission = match registration.permission {
                Some(permission) => permission,
                None => derived_command_permission(&registration.id)?,
            };
            let default_access = registration.default_access;
            let requirement = CommandRequirement::authorization(move |source: &S| {
                match source.permission_state(&permission) {
                    Some(PermissionState::Allow) => true,
                    Some(PermissionState::Deny) => false,
                    None => default_access,
                }
            });
            let root = (registration.factory)(dispatcher_root).also_requires(requirement);
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
    S: CommandPermissionSource,
{
    fn default() -> Self {
        Self::new()
    }
}

struct ResolvedCommand<S>
where
    S: CommandPermissionSource,
{
    id: Identifier,
    aliases: Vec<Box<str>>,
    root: CommandNodeBuilder<S, SteelCommandRuntime>,
}

impl<S> ResolvedCommand<S>
where
    S: CommandPermissionSource,
{
    fn roots(&self) -> impl Iterator<Item = &str> {
        once(self.id.path.as_ref()).chain(self.aliases.iter().map(AsRef::as_ref))
    }
}

fn derived_command_permission(id: &Identifier) -> Result<PermissionExpr, CommandRegistrationError> {
    let permission = PermissionKey::parse(format!("{}.command.{}", id.namespace, id.path))
        .map_err(
            |source| CommandRegistrationError::InvalidDerivedPermission {
                id: id.clone(),
                source,
            },
        )?;
    Ok(PermissionExpr::key(permission))
}

fn register_renamed_root<S>(
    dispatcher: &mut CommandDispatcher<S, SteelCommandRuntime>,
    root: &CommandNodeBuilder<S, SteelCommandRuntime>,
    name: impl Into<Box<str>>,
) -> Result<(), CommandRegistrationError>
where
    S: CommandPermissionSource,
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
    #[error("command '{id}' cannot derive a permission from its id: {source}")]
    InvalidDerivedPermission {
        id: Identifier,
        #[source]
        source: PermissionKeyError,
    },
    #[error("a validated command root unexpectedly became an argument")]
    UnexpectedArgumentRoot,
    #[error(transparent)]
    InvalidGraph(#[from] RegistrationError),
}

#[cfg(test)]
mod tests;
