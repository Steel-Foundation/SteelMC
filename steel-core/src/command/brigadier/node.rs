//! Command graph nodes and registration errors.

use std::{fmt, sync::Arc};

use thiserror::Error;

use super::{BrigadierRuntime, CommandRuntime};

type RequirementPredicate<S> = Arc<dyn Fn(&S) -> bool + Send + Sync>;

/// Identifies a node in one command dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId {
    pub(super) dispatcher: u64,
    pub(super) index: usize,
}

impl NodeId {
    pub(super) const fn new(dispatcher: u64, index: usize) -> Self {
        Self { dispatcher, index }
    }
}

/// Selects either an existing dispatcher node or the root currently being registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CommandRedirectTarget {
    /// A dispatcher node that already exists.
    Node(NodeId),
    /// The root of the command tree containing this redirect.
    CommandRoot,
}

impl From<NodeId> for CommandRedirectTarget {
    fn from(value: NodeId) -> Self {
        Self::Node(value)
    }
}

/// The externally relevant category of a command node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    /// The dispatcher root.
    Root,
    /// A literal token.
    Literal,
    /// A parsed argument.
    Argument,
}

/// A source predicate attached to a command node.
pub(crate) struct CommandRequirement<S> {
    predicate: Option<RequirementPredicate<S>>,
    kind: Option<CommandRequirementKind>,
}

/// Why a command node has a source requirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandRequirementKind {
    /// Access depends on Steel's authorization state.
    Authorization,
    /// Access depends on execution context rather than permission.
    Context,
}

impl<S> CommandRequirement<S> {
    /// Creates a requirement that permits every source.
    pub(crate) const fn allow_all() -> Self {
        Self {
            predicate: None,
            kind: None,
        }
    }

    /// Creates a permission-backed requirement with stable identity.
    pub(crate) fn authorization(predicate: impl Fn(&S) -> bool + Send + Sync + 'static) -> Self {
        Self {
            predicate: Some(Arc::new(predicate)),
            kind: Some(CommandRequirementKind::Authorization),
        }
    }

    /// Creates a non-permission source requirement with stable identity.
    pub(crate) fn contextual(predicate: impl Fn(&S) -> bool + Send + Sync + 'static) -> Self {
        Self {
            predicate: Some(Arc::new(predicate)),
            kind: Some(CommandRequirementKind::Context),
        }
    }

    /// Returns whether `source` can use the node.
    pub(crate) fn allows(&self, source: &S) -> bool {
        self.predicate
            .as_ref()
            .is_none_or(|predicate| predicate(source))
    }

    /// Returns whether the client should treat this node as permission restricted.
    pub(crate) const fn is_authorization(&self) -> bool {
        matches!(self.kind, Some(CommandRequirementKind::Authorization))
    }

    pub(super) fn and(self, other: Self) -> Self
    where
        S: 'static,
    {
        let kind = match (self.kind, other.kind) {
            (Some(CommandRequirementKind::Authorization), _)
            | (_, Some(CommandRequirementKind::Authorization)) => {
                Some(CommandRequirementKind::Authorization)
            }
            (Some(CommandRequirementKind::Context), _)
            | (_, Some(CommandRequirementKind::Context)) => Some(CommandRequirementKind::Context),
            (None, None) => None,
        };
        let predicate = match (self.predicate, other.predicate) {
            (None, None) => None,
            (Some(predicate), None) | (None, Some(predicate)) => Some(predicate),
            (Some(first), Some(second)) => {
                let combined: RequirementPredicate<S> =
                    Arc::new(move |source| first(source) && second(source));
                Some(combined)
            }
        };
        Self { predicate, kind }
    }

    pub(super) fn is_compatible_with(&self, other: &Self) -> bool {
        self.kind == other.kind
            && match (&self.predicate, &other.predicate) {
                (None, None) => true,
                (Some(first), Some(second)) => Arc::ptr_eq(first, second),
                (None, Some(_)) | (Some(_), None) => false,
            }
    }
}

impl<S> Clone for CommandRequirement<S> {
    fn clone(&self) -> Self {
        Self {
            predicate: self.predicate.as_ref().map(Arc::clone),
            kind: self.kind,
        }
    }
}

impl<S> fmt::Debug for CommandRequirement<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandRequirement")
            .field(
                "predicate",
                &self.predicate.as_ref().map(|_| "<source predicate>"),
            )
            .field("kind", &self.kind)
            .finish()
    }
}

pub(super) struct CommandRedirect<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    pub(super) target: CommandRedirectTarget,
    pub(super) modifier: Option<Arc<R::Modifier>>,
    pub(super) forks: bool,
}

impl<S, R> CommandRedirect<S, R>
where
    R: CommandRuntime<S>,
{
    pub(super) const fn identity(target: CommandRedirectTarget) -> Self {
        Self {
            target,
            modifier: None,
            forks: false,
        }
    }

    pub(super) const fn with_modifier(
        target: CommandRedirectTarget,
        modifier: Arc<R::Modifier>,
        forks: bool,
    ) -> Self {
        Self {
            target,
            modifier: Some(modifier),
            forks,
        }
    }

    fn resolve_command_root(&mut self, command_root: NodeId) {
        if self.target == CommandRedirectTarget::CommandRoot {
            self.target = CommandRedirectTarget::Node(command_root);
        }
    }

    fn is_compatible_with(&self, other: &Self) -> bool {
        self.target == other.target
            && self.forks == other.forks
            && match (&self.modifier, &other.modifier) {
                (None, None) => true,
                (Some(first), Some(second)) => Arc::ptr_eq(first, second),
                (None, Some(_)) | (Some(_), None) => false,
            }
    }
}

impl<S, R> Clone for CommandRedirect<S, R>
where
    R: CommandRuntime<S>,
{
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            modifier: self.modifier.as_ref().map(Arc::clone),
            forks: self.forks,
        }
    }
}

#[derive(Clone)]
pub(super) enum CommandNodeData<A> {
    Root,
    Literal(Box<str>),
    Argument { name: Box<str>, argument_type: A },
}

impl<A> CommandNodeData<A> {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Root => "",
            Self::Literal(name) | Self::Argument { name, .. } => name,
        }
    }

    pub(super) const fn kind(&self) -> NodeKind {
        match self {
            Self::Root => NodeKind::Root,
            Self::Literal(_) => NodeKind::Literal,
            Self::Argument { .. } => NodeKind::Argument,
        }
    }
}

impl<A> CommandNodeData<A>
where
    A: PartialEq,
{
    fn collision_with(&self, other: &Self) -> Option<RegistrationErrorKind> {
        let name = other.name().into();
        match (self, other) {
            (Self::Literal(first), Self::Literal(second)) if first == second => None,
            (
                Self::Argument {
                    name: first_name,
                    argument_type: first_type,
                },
                Self::Argument {
                    name: second_name,
                    argument_type: second_type,
                },
            ) if first_name == second_name && first_type == second_type => None,
            (Self::Argument { name: first, .. }, Self::Argument { name: second, .. })
                if first == second =>
            {
                Some(RegistrationErrorKind::ArgumentTypeCollision { name })
            }
            _ => Some(RegistrationErrorKind::NodeKindCollision {
                name,
                existing: self.kind(),
                incoming: other.kind(),
            }),
        }
    }
}

/// One node stored in the dispatcher's arena.
pub(crate) struct CommandNode<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    pub(super) data: CommandNodeData<R::Argument>,
    pub(super) children: Vec<NodeId>,
    pub(super) executor: Option<Arc<R::Executor>>,
    pub(super) requirement: CommandRequirement<S>,
    pub(super) execution_requirement: CommandRequirement<S>,
    pub(super) redirect: Option<CommandRedirect<S, R>>,
}

impl<S, R> CommandNode<S, R>
where
    R: CommandRuntime<S>,
{
    pub(super) const fn root() -> Self {
        Self {
            data: CommandNodeData::Root,
            children: Vec::new(),
            executor: None,
            requirement: CommandRequirement::allow_all(),
            execution_requirement: CommandRequirement::allow_all(),
            redirect: None,
        }
    }

    /// Returns the node name.
    pub(crate) fn name(&self) -> &str {
        self.data.name()
    }

    /// Returns whether this node has a command callback.
    pub(crate) const fn is_executable(&self) -> bool {
        self.executor.is_some()
    }

    /// Returns whether this source may run the node's executor.
    pub(crate) fn can_execute(&self, source: &S) -> bool {
        self.executor.is_some()
            && self.requirement.allows(source)
            && self.execution_requirement.allows(source)
    }

    /// Returns this node's redirect target.
    pub(crate) fn redirect(&self) -> Option<NodeId> {
        self.redirect
            .as_ref()
            .map(|redirect| match redirect.target {
                CommandRedirectTarget::Node(target) => target,
                CommandRedirectTarget::CommandRoot => {
                    unreachable!("registered command redirects have concrete targets")
                }
            })
    }

    /// Returns whether this node's redirect forks its command source.
    pub(crate) fn is_forked_redirect(&self) -> bool {
        self.redirect
            .as_ref()
            .is_some_and(|redirect| redirect.forks)
    }

    /// Returns whether this node transforms sources while redirecting.
    pub(crate) fn has_redirect_modifier(&self) -> bool {
        self.redirect
            .as_ref()
            .is_some_and(|redirect| redirect.modifier.is_some())
    }

    /// Returns the externally visible node category.
    pub(crate) const fn kind(&self) -> NodeKind {
        self.data.kind()
    }

    /// Returns this node's argument parser when it is an argument node.
    pub(crate) const fn argument_type(&self) -> Option<&R::Argument> {
        match &self.data {
            CommandNodeData::Argument { argument_type, .. } => Some(argument_type),
            CommandNodeData::Root | CommandNodeData::Literal(_) => None,
        }
    }

    /// Returns whether this node is available to `source`.
    pub(crate) fn allows(&self, source: &S) -> bool {
        self.requirement.allows(source)
    }

    /// Returns whether this node is guarded by authorization.
    pub(crate) const fn is_restricted(&self) -> bool {
        self.requirement.is_authorization() || self.execution_requirement.is_authorization()
    }

    pub(super) fn validate_compatible(
        &self,
        incoming: &UnregisteredCommandNode<S, R>,
    ) -> Result<(), RegistrationError> {
        if let Some(kind) = self.data.collision_with(&incoming.data) {
            return Err(RegistrationError::new(kind));
        }
        if !self.requirement.is_compatible_with(&incoming.requirement) {
            return Err(RegistrationError::new(
                RegistrationErrorKind::RequirementCollision {
                    name: incoming.name().into(),
                },
            ));
        }
        if !self
            .execution_requirement
            .is_compatible_with(&incoming.execution_requirement)
        {
            return Err(RegistrationError::new(
                RegistrationErrorKind::RequirementCollision {
                    name: incoming.name().into(),
                },
            ));
        }
        if !redirects_are_compatible(self.redirect.as_ref(), incoming.redirect.as_ref()) {
            return Err(RegistrationError::new(
                RegistrationErrorKind::RedirectCollision {
                    name: incoming.name().into(),
                },
            ));
        }
        Ok(())
    }
}

pub(super) struct UnregisteredCommandNode<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    pub(super) data: CommandNodeData<R::Argument>,
    pub(super) children: Vec<Self>,
    pub(super) executor: Option<Arc<R::Executor>>,
    pub(super) requirement: CommandRequirement<S>,
    pub(super) execution_requirement: CommandRequirement<S>,
    pub(super) redirect: Option<CommandRedirect<S, R>>,
}

impl<S, R> UnregisteredCommandNode<S, R>
where
    R: CommandRuntime<S>,
{
    pub(super) fn name(&self) -> &str {
        self.data.name()
    }

    pub(super) const fn kind(&self) -> NodeKind {
        self.data.kind()
    }

    pub(super) fn resolve_command_root(&mut self, command_root: NodeId) {
        if let Some(redirect) = &mut self.redirect {
            redirect.resolve_command_root(command_root);
        }
        for child in &mut self.children {
            child.resolve_command_root(command_root);
        }
    }

    pub(super) fn merge(&mut self, mut incoming: Self) -> Result<(), RegistrationError> {
        self.validate_compatible(&incoming)?;
        if incoming.executor.is_some() {
            self.executor = incoming.executor.take();
        }
        for child in incoming.children {
            merge_or_push(&mut self.children, child)?;
        }
        Ok(())
    }

    fn validate_compatible(&self, incoming: &Self) -> Result<(), RegistrationError> {
        if let Some(kind) = self.data.collision_with(&incoming.data) {
            return Err(RegistrationError::new(kind));
        }
        if !self.requirement.is_compatible_with(&incoming.requirement) {
            return Err(RegistrationError::new(
                RegistrationErrorKind::RequirementCollision {
                    name: incoming.name().into(),
                },
            ));
        }
        if !self
            .execution_requirement
            .is_compatible_with(&incoming.execution_requirement)
        {
            return Err(RegistrationError::new(
                RegistrationErrorKind::RequirementCollision {
                    name: incoming.name().into(),
                },
            ));
        }
        if !redirects_are_compatible(self.redirect.as_ref(), incoming.redirect.as_ref()) {
            return Err(RegistrationError::new(
                RegistrationErrorKind::RedirectCollision {
                    name: incoming.name().into(),
                },
            ));
        }
        Ok(())
    }
}

fn redirects_are_compatible<S, R>(
    first: Option<&CommandRedirect<S, R>>,
    second: Option<&CommandRedirect<S, R>>,
) -> bool
where
    R: CommandRuntime<S>,
{
    match (first, second) {
        (None, None) => true,
        (Some(first), Some(second)) => first.is_compatible_with(second),
        (None, Some(_)) | (Some(_), None) => false,
    }
}

pub(super) fn merge_or_push<S, R>(
    nodes: &mut Vec<UnregisteredCommandNode<S, R>>,
    incoming: UnregisteredCommandNode<S, R>,
) -> Result<(), RegistrationError>
where
    R: CommandRuntime<S>,
{
    let Some(existing) = nodes
        .iter_mut()
        .find(|existing| existing.name() == incoming.name())
    else {
        nodes.push(incoming);
        return Ok(());
    };
    existing.merge(incoming)
}

/// A command registration failure.
#[derive(Debug, Error)]
#[error("{kind}")]
pub(crate) struct RegistrationError {
    kind: RegistrationErrorKind,
}

impl RegistrationError {
    pub(super) const fn new(kind: RegistrationErrorKind) -> Self {
        Self { kind }
    }

    /// Returns the specific registration failure.
    pub(crate) const fn kind(&self) -> &RegistrationErrorKind {
        &self.kind
    }
}

/// Identifies why command registration failed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum RegistrationErrorKind {
    /// Only literals may be registered directly under the root.
    #[error("only literal command nodes can be registered at the root")]
    ArgumentRoot,
    /// The two nodes sharing a name have different categories.
    #[error("command node '{name}' is already registered as {existing:?}, not {incoming:?}")]
    NodeKindCollision {
        name: Box<str>,
        existing: NodeKind,
        incoming: NodeKind,
    },
    /// Argument nodes sharing a name use different parsers.
    #[error("argument node '{name}' is already registered with a different parser")]
    ArgumentTypeCollision { name: Box<str> },
    /// Nodes sharing a name use predicates with different identities.
    #[error("command node '{name}' is already registered with a different requirement")]
    RequirementCollision { name: Box<str> },
    /// Nodes sharing a name have different redirects.
    #[error("command node '{name}' is already registered with a different redirect")]
    RedirectCollision { name: Box<str> },
    /// A redirected node also has children.
    #[error("redirected command node '{name}' cannot have children")]
    RedirectWithChildren { name: Box<str> },
    /// A redirect points outside its dispatcher.
    #[error("redirect target {target:?} does not belong to this dispatcher")]
    InvalidRedirectTarget { target: NodeId },
}
