use std::{cmp::Ordering, error::Error, fmt};

use steel_utils::Identifier;

use super::{PermissionKeyError, PermissionSegment};

/// Rule-side context in which a permission entry applies.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PermissionRuleContext {
    /// Applies in every runtime context.
    Global,
    /// Applies within one server domain.
    Domain(PermissionDomain),
    /// Applies within one loaded world.
    World(Identifier),
    /// Applies when a subsystem-provided key has one value.
    Custom {
        /// Context key owned by Steel or a future plugin.
        key: PermissionContextKey,
        /// Required active value.
        value: PermissionContextValue,
    },
    /// Applies when every nested context matches.
    All(PermissionRuleContexts),
}

/// Validated Steel domain name used by permission contexts.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionDomain(String);

impl PermissionDomain {
    /// Parses a domain name using the command-visible world namespace grammar.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain is not a valid identifier namespace.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionRuleContextError> {
        let value = value.into();
        if value.is_empty() || !Identifier::validate_namespace(&value) {
            return Err(PermissionRuleContextError::InvalidDomain(value));
        }
        Ok(Self(value))
    }

    /// Returns the validated domain name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PermissionDomain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validated value in Steel's unquoted permission-context expression syntax.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionContextValue(String);

impl PermissionContextValue {
    /// Parses one custom context value.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot round-trip through an expression.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionRuleContextError> {
        let value = value.into();
        if value.is_empty()
            || value
                .chars()
                .any(|character| character.is_whitespace() || "{},=".contains(character))
        {
            return Err(PermissionRuleContextError::InvalidValue(value));
        }
        Ok(Self(value))
    }

    /// Returns the validated context value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PermissionContextValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Canonically ordered AND-chain of permission rule contexts.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PermissionRuleContexts {
    contexts: Vec<PermissionRuleContext>,
}

impl PermissionRuleContexts {
    const fn new(contexts: Vec<PermissionRuleContext>) -> Self {
        Self { contexts }
    }

    fn into_vec(self) -> Vec<PermissionRuleContext> {
        self.contexts
    }

    /// Returns the canonical context sequence.
    pub fn iter(&self) -> impl Iterator<Item = &PermissionRuleContext> {
        self.contexts.iter()
    }
}

impl PermissionRuleContext {
    /// Returns the global context.
    #[must_use]
    pub const fn global() -> Self {
        Self::Global
    }

    /// Creates a domain-scoped rule context.
    pub fn domain(domain: impl Into<String>) -> Result<Self, PermissionRuleContextError> {
        PermissionDomain::parse(domain).map(Self::Domain)
    }

    /// Creates a loaded-world rule context.
    #[must_use]
    pub const fn world(world: Identifier) -> Self {
        Self::World(world)
    }

    /// Creates a custom rule context.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is empty.
    pub fn custom(
        key: PermissionContextKey,
        value: impl Into<String>,
    ) -> Result<Self, PermissionRuleContextError> {
        let value = PermissionContextValue::parse(value)?;
        Ok(Self::Custom { key, value })
    }

    /// Creates a canonical AND-chain of rule contexts.
    ///
    /// Global entries are omitted, nested chains are flattened, and duplicate
    /// values are idempotent. Conflicting values for one context key are rejected.
    ///
    /// # Errors
    ///
    /// Returns an error when a built-in or custom key receives multiple values.
    pub fn all(
        contexts: impl IntoIterator<Item = Self>,
    ) -> Result<Self, PermissionRuleContextError> {
        let mut flattened = Vec::new();
        for context in contexts {
            match context {
                Self::Global => {}
                Self::All(contexts) => {
                    for context in contexts.into_vec() {
                        push_unique_context(&mut flattened, context)?;
                    }
                }
                context => push_unique_context(&mut flattened, context)?,
            }
        }
        normalize_world_domain(&mut flattened)?;
        flattened.sort_by(compare_rule_contexts);

        Ok(match flattened.len() {
            0 => Self::Global,
            1 => match flattened.pop() {
                Some(context) => context,
                None => Self::Global,
            },
            _ => Self::All(PermissionRuleContexts::new(flattened)),
        })
    }

    pub(super) fn matches(&self, context: &PermissionContext) -> bool {
        match self {
            Self::Global => true,
            Self::Domain(domain) => context.domain.as_ref() == Some(domain),
            Self::World(world) => context.world.as_ref() == Some(world),
            Self::Custom { .. } => context.custom_contexts.contains(self),
            Self::All(contexts) => contexts
                .iter()
                .all(|constraint| constraint.matches(context)),
        }
    }

    pub(super) fn specificity(&self) -> usize {
        match self {
            Self::Global => 0,
            Self::Domain(_) | Self::Custom { .. } => 1,
            Self::World(_) => 2,
            Self::All(contexts) => contexts.iter().map(Self::specificity).sum(),
        }
    }
}

impl fmt::Display for PermissionRuleContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => formatter.write_str("global"),
            Self::Domain(domain) => write!(formatter, "domain {}", domain.as_str()),
            Self::World(world) => write!(formatter, "world {world}"),
            Self::Custom { key, value } => {
                write!(formatter, "{} {}", key.as_str(), value.as_str())
            }
            Self::All(contexts) => {
                for (index, context) in contexts.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(" + ")?;
                    }
                    write!(formatter, "{context}")?;
                }
                Ok(())
            }
        }
    }
}

fn normalize_world_domain(
    contexts: &mut Vec<PermissionRuleContext>,
) -> Result<(), PermissionRuleContextError> {
    let domain = contexts.iter().find_map(|context| match context {
        PermissionRuleContext::Domain(domain) => Some(domain.as_str()),
        _ => None,
    });
    let world_domain = contexts.iter().find_map(|context| match context {
        PermissionRuleContext::World(world) => Some(world.namespace.as_ref()),
        _ => None,
    });
    let (Some(domain), Some(world_domain)) = (domain, world_domain) else {
        return Ok(());
    };
    if domain != world_domain {
        return Err(PermissionRuleContextError::WorldDomainMismatch {
            domain: domain.to_owned(),
            world_domain: world_domain.to_owned(),
        });
    }
    contexts.retain(|context| !matches!(context, PermissionRuleContext::Domain(_)));
    Ok(())
}

fn push_unique_context(
    contexts: &mut Vec<PermissionRuleContext>,
    context: PermissionRuleContext,
) -> Result<(), PermissionRuleContextError> {
    for existing in contexts.iter() {
        match (&context, existing) {
            (PermissionRuleContext::Domain(value), PermissionRuleContext::Domain(current)) => {
                if value == current {
                    return Ok(());
                }
                return Err(PermissionRuleContextError::DuplicateDomain);
            }
            (PermissionRuleContext::World(value), PermissionRuleContext::World(current)) => {
                if value == current {
                    return Ok(());
                }
                return Err(PermissionRuleContextError::DuplicateWorld);
            }
            (
                PermissionRuleContext::Custom { key, value },
                PermissionRuleContext::Custom {
                    key: current_key,
                    value: current_value,
                },
            ) if key == current_key => {
                if value == current_value {
                    return Ok(());
                }
                return Err(PermissionRuleContextError::DuplicateCustomKey(key.clone()));
            }
            _ => {}
        }
    }
    if !contexts.contains(&context) {
        contexts.push(context);
    }
    Ok(())
}

fn compare_rule_contexts(left: &PermissionRuleContext, right: &PermissionRuleContext) -> Ordering {
    rule_context_rank(left)
        .cmp(&rule_context_rank(right))
        .then_with(|| match (left, right) {
            (PermissionRuleContext::Domain(left), PermissionRuleContext::Domain(right)) => {
                left.cmp(right)
            }
            (PermissionRuleContext::World(left), PermissionRuleContext::World(right)) => left
                .namespace
                .cmp(&right.namespace)
                .then_with(|| left.path.cmp(&right.path)),
            (
                PermissionRuleContext::Custom {
                    key: left_key,
                    value: left_value,
                },
                PermissionRuleContext::Custom {
                    key: right_key,
                    value: right_value,
                },
            ) => left_key
                .as_str()
                .cmp(right_key.as_str())
                .then_with(|| left_value.cmp(right_value)),
            _ => Ordering::Equal,
        })
}

const fn rule_context_rank(context: &PermissionRuleContext) -> u8 {
    match context {
        PermissionRuleContext::Domain(_) => 0,
        PermissionRuleContext::World(_) => 1,
        PermissionRuleContext::Custom { .. } => 2,
        PermissionRuleContext::Global => 3,
        PermissionRuleContext::All(_) => 4,
    }
}

/// Active runtime context used to evaluate permission rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionContext {
    domain: Option<PermissionDomain>,
    world: Option<Identifier>,
    custom_contexts: Vec<PermissionRuleContext>,
}

impl PermissionContext {
    /// Creates a context with no active scopes.
    #[must_use]
    pub fn global() -> Self {
        Self::default()
    }

    /// Creates a context for one domain.
    pub fn for_domain(domain: impl Into<String>) -> Result<Self, PermissionRuleContextError> {
        Ok(Self {
            domain: Some(PermissionDomain::parse(domain)?),
            world: None,
            custom_contexts: Vec::new(),
        })
    }

    /// Creates a context for a loaded world and its owning domain.
    #[must_use]
    pub fn for_world(world: Identifier) -> Self {
        Self {
            domain: Some(PermissionDomain(world.namespace.to_string())),
            world: Some(world),
            custom_contexts: Vec::new(),
        }
    }

    /// Builds the active context represented by one rule-side expression.
    ///
    /// World namespaces are Steel domain names, matching command-visible world identifiers.
    ///
    /// # Errors
    ///
    /// Returns an error if a custom context value conflicts with another value.
    pub fn from_rule_context(
        rule_context: &PermissionRuleContext,
    ) -> Result<Self, PermissionRuleContextError> {
        let mut context = Self::global();
        context.append_rule_context(rule_context)?;
        Ok(context)
    }

    fn append_rule_context(
        &mut self,
        rule_context: &PermissionRuleContext,
    ) -> Result<(), PermissionRuleContextError> {
        match rule_context {
            PermissionRuleContext::Global => {}
            PermissionRuleContext::Domain(domain) => {
                self.domain = Some(domain.clone());
            }
            PermissionRuleContext::World(world) => {
                self.domain = Some(PermissionDomain(world.namespace.to_string()));
                self.world = Some(world.clone());
            }
            PermissionRuleContext::Custom { key, value } => {
                self.add_custom_context(key.clone(), value.as_str())?;
            }
            PermissionRuleContext::All(contexts) => {
                for context in contexts.iter() {
                    self.append_rule_context(context)?;
                }
            }
        }
        Ok(())
    }

    /// Adds one custom active context.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty value or a conflicting value for the same key.
    pub fn with_custom_context(
        mut self,
        key: PermissionContextKey,
        value: impl Into<String>,
    ) -> Result<Self, PermissionRuleContextError> {
        self.add_custom_context(key, value)?;
        Ok(self)
    }

    /// Adds one custom active context in place.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty value or a conflicting value for the same key.
    pub fn add_custom_context(
        &mut self,
        key: PermissionContextKey,
        value: impl Into<String>,
    ) -> Result<(), PermissionRuleContextError> {
        let context = PermissionRuleContext::custom(key, value)?;
        let PermissionRuleContext::Custom { key, value } = &context else {
            return Ok(());
        };
        for existing in &self.custom_contexts {
            let PermissionRuleContext::Custom {
                key: existing_key,
                value: existing_value,
            } = existing
            else {
                continue;
            };
            if existing_key != key {
                continue;
            }
            if existing_value == value {
                return Ok(());
            }
            return Err(PermissionRuleContextError::DuplicateCustomKey(key.clone()));
        }
        self.custom_contexts.push(context);
        Ok(())
    }
}

/// One custom permission context key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionContextKey(String);

impl PermissionContextKey {
    /// Parses a local name or namespaced plugin context key.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is not a valid segment or identifier-like name.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionContextKeyError> {
        let value = value.into();
        if value.contains(':') {
            validate_namespaced_context_key(&value)?;
            return Ok(Self(value));
        }
        PermissionSegment::parse(value.clone())
            .map_err(PermissionContextKeyError::InvalidUnqualified)?;
        Ok(Self(value))
    }

    /// Returns the validated context key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_namespaced_context_key(value: &str) -> Result<(), PermissionContextKeyError> {
    let Some((namespace, path)) = value.split_once(':') else {
        return Err(PermissionContextKeyError::InvalidFormat);
    };
    if namespace.is_empty() {
        return Err(PermissionContextKeyError::EmptyNamespace);
    }
    if path.is_empty() {
        return Err(PermissionContextKeyError::EmptyPath);
    }
    if path.contains(':') {
        return Err(PermissionContextKeyError::InvalidFormat);
    }
    if namespace.split('.').any(str::is_empty) {
        return Err(PermissionContextKeyError::InvalidNamespace);
    }
    if path.split(['.', '/']).any(str::is_empty) {
        return Err(PermissionContextKeyError::InvalidPath);
    }
    if !Identifier::validate_namespace(namespace) {
        return Err(PermissionContextKeyError::InvalidNamespace);
    }
    if !Identifier::validate_path(path) {
        return Err(PermissionContextKeyError::InvalidPath);
    }
    Ok(())
}

/// Invalid rule-side or active permission context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionRuleContextError {
    /// A domain name is not a valid identifier namespace.
    InvalidDomain(String),
    /// A custom value cannot be represented by the expression syntax.
    InvalidValue(String),
    /// One chain binds two different domains.
    DuplicateDomain,
    /// One chain binds two different loaded worlds.
    DuplicateWorld,
    /// One custom key is bound to two different values.
    DuplicateCustomKey(PermissionContextKey),
    /// A world identifier and explicit domain name disagree.
    WorldDomainMismatch {
        /// Explicit domain constraint.
        domain: String,
        /// Domain implied by the world identifier.
        world_domain: String,
    },
}

impl fmt::Display for PermissionRuleContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDomain(domain) => {
                write!(formatter, "invalid permission context domain '{domain}'")
            }
            Self::InvalidValue(value) => {
                write!(formatter, "invalid permission context value '{value}'")
            }
            Self::DuplicateDomain => {
                formatter.write_str("domain permission context cannot have multiple values")
            }
            Self::DuplicateWorld => {
                formatter.write_str("world permission context cannot have multiple values")
            }
            Self::DuplicateCustomKey(key) => write!(
                formatter,
                "custom permission context key '{}' cannot have multiple values",
                key.as_str()
            ),
            Self::WorldDomainMismatch {
                domain,
                world_domain,
            } => write!(
                formatter,
                "permission context domain '{domain}' conflicts with world domain '{world_domain}'"
            ),
        }
    }
}

impl Error for PermissionRuleContextError {}

/// Invalid custom permission context key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionContextKeyError {
    /// A namespaced key does not use one `namespace:path` separator.
    InvalidFormat,
    /// The namespace before `:` is empty.
    EmptyNamespace,
    /// The path after `:` is empty.
    EmptyPath,
    /// The namespace is not a valid identifier namespace.
    InvalidNamespace,
    /// The path is not a valid identifier path.
    InvalidPath,
    /// An unqualified key is not a valid permission segment.
    InvalidUnqualified(PermissionKeyError),
}

impl fmt::Display for PermissionContextKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => {
                formatter.write_str("context key must be a name or namespaced id")
            }
            Self::EmptyNamespace => formatter.write_str("context key namespace is empty"),
            Self::EmptyPath => formatter.write_str("context key path is empty"),
            Self::InvalidNamespace => {
                formatter.write_str("context key namespace contains invalid characters")
            }
            Self::InvalidPath => {
                formatter.write_str("context key path contains invalid characters")
            }
            Self::InvalidUnqualified(source) => source.fmt(formatter),
        }
    }
}

impl Error for PermissionContextKeyError {}
