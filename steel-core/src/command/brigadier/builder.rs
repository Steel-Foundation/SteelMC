//! Command node builders.

use std::sync::Arc;

use super::{
    ArgumentType, BrigadierRuntime, CommandContext, CommandRequirement, CommandRuntime,
    CommandSyntaxError, NodeId, RegistrationError, RegistrationErrorKind,
    node::{
        CommandNodeData, CommandRedirect, CommandRedirectTarget, UnregisteredCommandNode,
        merge_or_push,
    },
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
    execution_requirement: CommandRequirement<S>,
    redirect: Option<CommandRedirect<S, R>>,
}

/// Requirements for traversing a scoped route and executing its current node.
pub(crate) struct CommandRequirementRoute<S> {
    traversal: CommandRequirement<S>,
    execution: CommandRequirement<S>,
}

impl<S> CommandRequirementRoute<S> {
    /// Creates the requirements for one resolved route through a command tree.
    pub(crate) const fn new(
        traversal: CommandRequirement<S>,
        execution: CommandRequirement<S>,
    ) -> Self {
        Self {
            traversal,
            execution,
        }
    }
}

impl<S> Clone for CommandRequirementRoute<S> {
    fn clone(&self) -> Self {
        Self {
            traversal: self.traversal.clone(),
            execution: self.execution.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequirementRouteKey {
    governing_scope: Option<usize>,
    descendant_scopes: Vec<usize>,
}

#[derive(Clone, Copy)]
struct ActiveRequirementScope<'path> {
    index: usize,
    remaining: &'path [Box<str>],
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
            execution_requirement: self.execution_requirement.clone(),
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
            execution_requirement: CommandRequirement::allow_all(),
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
            execution_requirement: CommandRequirement::allow_all(),
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

    /// Adds a requirement that applies only when this node's executor is selected.
    #[must_use]
    pub(crate) fn also_requires_execution(mut self, requirement: CommandRequirement<S>) -> Self
    where
        S: 'static,
    {
        self.execution_requirement = self.execution_requirement.and(requirement);
        self
    }

    /// Returns the number of occurrences of one literal path below this node.
    ///
    /// Argument nodes do not consume path segments.
    pub(crate) fn literal_path_match_count(&self, path: &[Box<str>]) -> usize {
        let Some((name, remaining)) = path.split_first() else {
            return 0;
        };
        let mut matches = 0;
        for child in &self.children {
            let Some(literal) = child.literal_name() else {
                matches += child.literal_path_match_count(path);
                continue;
            };
            if literal != name.as_ref() {
                continue;
            }
            if remaining.is_empty() {
                matches += 1;
            } else {
                matches += child.literal_path_match_count(remaining);
            }
        }
        matches
    }

    /// Applies independently scoped requirements using literal-only paths.
    ///
    /// A descendant scope may traverse its ancestors, but it cannot execute an
    /// ancestor unless the route's governing requirement also permits it.
    pub(crate) fn apply_scoped_requirements(
        &mut self,
        scope_paths: &[Vec<Box<str>>],
        mut requirements_for: impl FnMut(Option<usize>, &[usize]) -> CommandRequirementRoute<S>,
    ) where
        S: 'static,
    {
        let active = scope_paths
            .iter()
            .enumerate()
            .map(|(index, path)| ActiveRequirementScope {
                index,
                remaining: path,
            })
            .collect::<Vec<_>>();
        let mut cache = Vec::new();
        self.apply_scoped_requirements_inner(
            None,
            &active,
            None,
            &mut cache,
            &mut requirements_for,
        );
    }

    fn apply_scoped_requirements_inner<F>(
        &mut self,
        inherited_scope: Option<usize>,
        active: &[ActiveRequirementScope<'_>],
        parent_route: Option<&RequirementRouteKey>,
        cache: &mut Vec<(RequirementRouteKey, CommandRequirementRoute<S>)>,
        requirements_for: &mut F,
    ) where
        S: 'static,
        F: FnMut(Option<usize>, &[usize]) -> CommandRequirementRoute<S>,
    {
        let governing_scope = active
            .iter()
            .find(|scope| scope.remaining.is_empty())
            .map_or(inherited_scope, |scope| Some(scope.index));
        let descendant_scopes = active
            .iter()
            .filter(|scope| !scope.remaining.is_empty())
            .map(|scope| scope.index)
            .collect::<Vec<_>>();
        let route = RequirementRouteKey {
            governing_scope,
            descendant_scopes,
        };
        let requirements = cached_route_requirements(cache, &route, requirements_for);

        if parent_route != Some(&route) {
            self.requirement = self.requirement.clone().and(requirements.traversal);
        }
        if !route.descendant_scopes.is_empty() && self.executor.is_some() {
            self.execution_requirement = self
                .execution_requirement
                .clone()
                .and(requirements.execution);
        }

        for child in &mut self.children {
            let child_active = if let Some(literal) = child.literal_name() {
                active
                    .iter()
                    .filter_map(|scope| {
                        let (name, remaining) = scope.remaining.split_first()?;
                        (name.as_ref() == literal).then_some(ActiveRequirementScope {
                            index: scope.index,
                            remaining,
                        })
                    })
                    .collect::<Vec<_>>()
            } else {
                active
                    .iter()
                    .filter(|scope| {
                        !scope.remaining.is_empty()
                            && child.literal_path_match_count(scope.remaining) > 0
                    })
                    .copied()
                    .collect::<Vec<_>>()
            };
            child.apply_scoped_requirements_inner(
                governing_scope,
                &child_active,
                Some(&route),
                cache,
                requirements_for,
            );
        }
    }

    /// Redirects parsing to an existing node without transforming the source.
    #[must_use]
    pub(crate) fn redirects(mut self, target: impl Into<CommandRedirectTarget>) -> Self {
        self.redirect = Some(CommandRedirect::identity(target.into()));
        self
    }

    /// Redirects with an opaque runtime modifier payload.
    #[must_use]
    pub(crate) fn redirects_with_modifier(
        mut self,
        target: impl Into<CommandRedirectTarget>,
        modifier: Arc<R::Modifier>,
        forks: bool,
    ) -> Self {
        self.redirect = Some(CommandRedirect::with_modifier(
            target.into(),
            modifier,
            forks,
        ));
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
            execution_requirement: self.execution_requirement,
            redirect: self.redirect,
        })
    }
}

fn cached_route_requirements<S, F>(
    cache: &mut Vec<(RequirementRouteKey, CommandRequirementRoute<S>)>,
    route: &RequirementRouteKey,
    requirements_for: &mut F,
) -> CommandRequirementRoute<S>
where
    F: FnMut(Option<usize>, &[usize]) -> CommandRequirementRoute<S>,
{
    if let Some((_, requirements)) = cache.iter().find(|(cached, _)| cached == route) {
        return requirements.clone();
    }
    let requirements = requirements_for(route.governing_scope, &route.descendant_scopes);
    cache.push((route.clone(), requirements.clone()));
    requirements
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
