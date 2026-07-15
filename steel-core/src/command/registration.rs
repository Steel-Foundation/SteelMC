//! Stable command identities and collision-aware dispatcher construction.

use std::{collections::BTreeSet, iter::once};

use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::Identifier;
use thiserror::Error;

use super::{
    brigadier::{
        CommandDispatcher, CommandNodeBuilder, CommandRequirement, CommandRequirementRoute, NodeId,
        RegistrationError,
    },
    execution::{CommandPermissionSource, SteelCommandRuntime},
};
use crate::permission::{
    PermissionExpr, PermissionKey, PermissionKeyError, PermissionSegment, PermissionState,
};

type CommandFactory<S> =
    dyn FnOnce(NodeId) -> CommandNodeBuilder<S, SteelCommandRuntime> + Send + 'static;

pub(crate) const ENTITY_SELECTOR_PERMISSION_KEY: &str = "minecraft.selector";
pub(crate) const ENTITY_SELECTOR_ADVANCED_PERMISSION_KEY: &str = "minecraft.selector.advanced";

pub(crate) fn entity_selector_permission_expr() -> Result<PermissionExpr, PermissionKeyError> {
    PermissionKey::parse(ENTITY_SELECTOR_PERMISSION_KEY).map(PermissionExpr::key)
}

pub(crate) fn entity_selector_advanced_permission_expr()
-> Result<PermissionExpr, PermissionKeyError> {
    PermissionKey::parse(ENTITY_SELECTOR_ADVANCED_PERMISSION_KEY).map(PermissionExpr::key)
}

/// One complete command tree and its stable owner identity.
pub(crate) struct CommandRegistration<S>
where
    S: CommandPermissionSource,
{
    id: Identifier,
    aliases: Vec<Box<str>>,
    permission: Option<PermissionExpr>,
    subcommand_permissions: Vec<Vec<Box<str>>>,
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
        factory: impl FnOnce(NodeId) -> CommandNodeBuilder<S, SteelCommandRuntime> + Send + 'static,
    ) -> Self {
        Self {
            id,
            aliases: Vec::new(),
            permission: None,
            subcommand_permissions: Vec::new(),
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

    /// Allows a literal path through a permission derived from the command ID.
    ///
    /// The root permission remains a fallback grant. For example,
    /// `minecraft.command.tick.freeze` permits only `/tick freeze`, while
    /// `minecraft.command.tick` permits every tick subcommand.
    #[must_use]
    pub(crate) fn subcommand_permission<I, T>(mut self, path: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Box<str>>,
    {
        self.subcommand_permissions
            .push(path.into_iter().map(Into::into).collect());
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
        if self.permission.is_some() && !self.subcommand_permissions.is_empty() {
            return Err(
                CommandRegistrationError::SubcommandPermissionsRequireDerivedRoot {
                    id: self.id.clone(),
                },
            );
        }
        let mut permission_paths = FxHashSet::default();
        for path in &self.subcommand_permissions {
            if path.is_empty() {
                return Err(CommandRegistrationError::EmptySubcommandPermissionPath {
                    id: self.id.clone(),
                });
            }
            for segment in path {
                PermissionSegment::parse(segment.to_string()).map_err(|source| {
                    CommandRegistrationError::InvalidSubcommandPermissionPath {
                        id: self.id.clone(),
                        path: display_permission_path(path),
                        source,
                    }
                })?;
            }
            let path = display_permission_path(path);
            if !permission_paths.insert(path.clone()) {
                return Err(
                    CommandRegistrationError::DuplicateSubcommandPermissionPath {
                        id: self.id.clone(),
                        path,
                    },
                );
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
    declared_permissions: BTreeSet<PermissionKey>,
}

/// Built dispatcher and its discovery-only permission declarations.
pub(crate) struct RegisteredCommandDispatcher<S>
where
    S: CommandPermissionSource,
{
    pub(crate) dispatcher: CommandDispatcher<S, SteelCommandRuntime>,
    pub(crate) permissions: Vec<PermissionKey>,
}

impl<S> CommandDispatcherBuilder<S>
where
    S: CommandPermissionSource,
{
    pub(crate) fn new() -> Self {
        Self {
            registrations: Vec::new(),
            ids: FxHashSet::default(),
            declared_permissions: BTreeSet::new(),
        }
    }

    /// Declares a non-command permission for discovery and autocomplete.
    pub(crate) fn declare_permission(
        &mut self,
        permission: impl Into<String>,
    ) -> Result<(), CommandRegistrationError> {
        let value = permission.into();
        let permission = PermissionKey::parse(value.clone()).map_err(|source| {
            CommandRegistrationError::InvalidInternalPermission { value, source }
        })?;
        self.declared_permissions.insert(permission);
        Ok(())
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

    pub(crate) fn extend(&mut self, other: Self) -> Result<(), CommandRegistrationError> {
        let Self {
            registrations,
            declared_permissions,
            ..
        } = other;
        self.declared_permissions.extend(declared_permissions);
        for registration in registrations {
            self.register(registration)?;
        }
        Ok(())
    }

    /// Builds the complete graph without exposing a partially registered dispatcher.
    #[cfg(test)]
    pub(crate) fn build(
        self,
    ) -> Result<CommandDispatcher<S, SteelCommandRuntime>, CommandRegistrationError> {
        self.build_with_permissions()
            .map(|registered| registered.dispatcher)
    }

    /// Builds the graph and retains permission keys for command autocomplete.
    pub(crate) fn build_with_permissions(
        self,
    ) -> Result<RegisteredCommandDispatcher<S>, CommandRegistrationError> {
        let mut dispatcher = CommandDispatcher::new();
        let dispatcher_root = dispatcher.root();
        let mut resolved = Vec::with_capacity(self.registrations.len());
        let mut permissions = self.declared_permissions;

        for registration in self.registrations {
            let CommandRegistration {
                id,
                aliases,
                permission,
                subcommand_permissions,
                default_access,
                factory,
            } = registration;
            let (root_permission, derived_root) = if let Some(permission) = permission {
                (permission, None)
            } else {
                let key = derived_command_permission_key(&id)?;
                (PermissionExpr::key(key.clone()), Some(key))
            };
            collect_permission_keys(&root_permission, &mut permissions);
            let root = apply_registration_requirements(
                factory(dispatcher_root),
                &id,
                root_permission,
                derived_root.as_ref(),
                &subcommand_permissions,
                default_access,
                &mut permissions,
            )?;
            let Some(root_name) = root.literal_name() else {
                return Err(CommandRegistrationError::RootMustBeLiteral { id });
            };
            if root_name != id.path {
                return Err(CommandRegistrationError::RootDoesNotMatchId {
                    id,
                    root: root_name.into(),
                });
            }
            resolved.push(ResolvedCommand { id, aliases, root });
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

        Ok(RegisteredCommandDispatcher {
            dispatcher,
            permissions: permissions.into_iter().collect(),
        })
    }
}

fn apply_registration_requirements<S>(
    mut root: CommandNodeBuilder<S, SteelCommandRuntime>,
    id: &Identifier,
    root_permission: PermissionExpr,
    derived_root: Option<&PermissionKey>,
    subcommand_permissions: &[Vec<Box<str>>],
    default_access: bool,
    permissions: &mut BTreeSet<PermissionKey>,
) -> Result<CommandNodeBuilder<S, SteelCommandRuntime>, CommandRegistrationError>
where
    S: CommandPermissionSource,
{
    if subcommand_permissions.is_empty() {
        return Ok(root.also_requires(root_permission_requirement(
            root_permission,
            Vec::new(),
            default_access,
        )));
    }
    let Some(derived_root) = derived_root else {
        return Err(
            CommandRegistrationError::SubcommandPermissionsRequireDerivedRoot { id: id.clone() },
        );
    };

    let mut scoped_permissions = Vec::with_capacity(subcommand_permissions.len());
    for path in subcommand_permissions {
        let permission = derived_subcommand_permission(id, derived_root, path)?;
        permissions.insert(permission.clone());
        match root.literal_path_match_count(path) {
            1 => scoped_permissions.push(permission),
            0 => {
                return Err(CommandRegistrationError::MissingSubcommandPermissionPath {
                    id: id.clone(),
                    path: display_permission_path(path),
                });
            }
            matches => {
                return Err(
                    CommandRegistrationError::AmbiguousSubcommandPermissionPath {
                        id: id.clone(),
                        path: display_permission_path(path),
                        matches,
                    },
                );
            }
        }
    }
    root.apply_scoped_requirements(
        subcommand_permissions,
        |governing_scope, descendant_scopes| {
            let descendants = descendant_scopes
                .iter()
                .map(|index| scoped_permissions[*index].clone())
                .collect::<Vec<_>>();
            let traversal = if let Some(index) = governing_scope {
                scoped_permission_requirement(derived_root, &scoped_permissions[index], descendants)
            } else {
                root_permission_requirement(root_permission.clone(), descendants, default_access)
            };
            let execution = if let Some(index) = governing_scope {
                scoped_permission_requirement(derived_root, &scoped_permissions[index], Vec::new())
            } else {
                root_permission_requirement(root_permission.clone(), Vec::new(), default_access)
            };
            CommandRequirementRoute::new(traversal, execution)
        },
    );
    Ok(root)
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

fn derived_command_permission_key(
    id: &Identifier,
) -> Result<PermissionKey, CommandRegistrationError> {
    PermissionKey::parse(format!("{}.command.{}", id.namespace, id.path)).map_err(|source| {
        CommandRegistrationError::InvalidDerivedPermission {
            id: id.clone(),
            source,
        }
    })
}

fn derived_subcommand_permission(
    id: &Identifier,
    root: &PermissionKey,
    path: &[Box<str>],
) -> Result<PermissionKey, CommandRegistrationError> {
    let mut permission = root.clone();
    for segment in path {
        let segment = PermissionSegment::parse(segment.to_string()).map_err(|source| {
            CommandRegistrationError::InvalidSubcommandPermissionPath {
                id: id.clone(),
                path: display_permission_path(path),
                source,
            }
        })?;
        permission = permission.child(&segment).map_err(|source| {
            CommandRegistrationError::InvalidSubcommandPermissionPath {
                id: id.clone(),
                path: display_permission_path(path),
                source,
            }
        })?;
    }
    Ok(permission)
}

fn root_permission_requirement<S>(
    root: PermissionExpr,
    alternatives: Vec<PermissionKey>,
    default_access: bool,
) -> CommandRequirement<S>
where
    S: CommandPermissionSource,
{
    if default_access {
        let alternatives = if alternatives.is_empty() {
            None
        } else {
            Some(PermissionExpr::Any(
                alternatives.into_iter().map(PermissionExpr::key).collect(),
            ))
        };
        return CommandRequirement::contextual(move |source: &S| {
            source.permission_state(&root) != Some(PermissionState::Deny)
                || alternatives.as_ref().is_some_and(|alternatives| {
                    source.permission_state(alternatives) == Some(PermissionState::Allow)
                })
        });
    }

    let permission = alternatives
        .into_iter()
        .fold(root, |permission, alternative| {
            permission | PermissionExpr::key(alternative)
        });
    permission_requirement(permission)
}

fn permission_requirement<S>(permission: PermissionExpr) -> CommandRequirement<S>
where
    S: CommandPermissionSource,
{
    CommandRequirement::authorization(move |source: &S| {
        source.permission_state(&permission) == Some(PermissionState::Allow)
    })
}

fn scoped_permission_requirement<S>(
    root: &PermissionKey,
    scoped: &PermissionKey,
    alternatives: Vec<PermissionKey>,
) -> CommandRequirement<S>
where
    S: CommandPermissionSource,
{
    let permission = alternatives.into_iter().fold(
        PermissionExpr::scoped_key(root.clone(), scoped.clone()),
        |permission, alternative| permission | PermissionExpr::key(alternative),
    );
    permission_requirement(permission)
}

fn collect_permission_keys(expression: &PermissionExpr, keys: &mut BTreeSet<PermissionKey>) {
    match expression {
        PermissionExpr::Key(key) => {
            keys.insert(key.clone());
        }
        PermissionExpr::ScopedKey { parent, key } => {
            keys.insert(parent.clone());
            keys.insert(key.clone());
        }
        PermissionExpr::All(expressions) | PermissionExpr::Any(expressions) => {
            for expression in expressions {
                collect_permission_keys(expression, keys);
            }
        }
    }
}

fn display_permission_path(path: &[Box<str>]) -> String {
    path.iter()
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>()
        .join(".")
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
    #[error("invalid internal permission declaration '{value}': {source}")]
    InvalidInternalPermission {
        value: String,
        #[source]
        source: PermissionKeyError,
    },
    #[error("command '{id}' has an invalid explicit permission: {source}")]
    InvalidExplicitPermission {
        id: Identifier,
        #[source]
        source: PermissionKeyError,
    },
    #[error("command '{id}' cannot combine explicit and derived subcommand permissions")]
    SubcommandPermissionsRequireDerivedRoot { id: Identifier },
    #[error("command '{id}' has an empty subcommand permission path")]
    EmptySubcommandPermissionPath { id: Identifier },
    #[error("command '{id}' has invalid subcommand permission path '{path}': {source}")]
    InvalidSubcommandPermissionPath {
        id: Identifier,
        path: String,
        #[source]
        source: PermissionKeyError,
    },
    #[error("command '{id}' declares subcommand permission path '{path}' more than once")]
    DuplicateSubcommandPermissionPath { id: Identifier, path: String },
    #[error("command '{id}' has no literal at subcommand permission path '{path}'")]
    MissingSubcommandPermissionPath { id: Identifier, path: String },
    #[error("command '{id}' has {matches} literals at subcommand permission path '{path}'")]
    AmbiguousSubcommandPermissionPath {
        id: Identifier,
        path: String,
        matches: usize,
    },
    #[error("a validated command root unexpectedly became an argument")]
    UnexpectedArgumentRoot,
    #[error(transparent)]
    InvalidGraph(#[from] RegistrationError),
}

#[cfg(test)]
mod tests;
