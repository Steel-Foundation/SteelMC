use std::{error::Error, fmt, mem};

use serde::{Deserialize, Serialize};
use steel_utils::Identifier;

use super::{
    PermissionContext, PermissionContextKeyError, PermissionResolutionSource,
    PermissionRuleContext, PermissionRuleContextError,
    rule_expression::{PermissionExpressionContextError, parse_context, write_context},
};

/// A typed value attached to a permission subject or group.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PermissionMetadataValue {
    /// Boolean metadata.
    Bool(bool),
    /// Signed integer metadata.
    Integer(i64),
    /// Text metadata.
    String(String),
}

impl PermissionMetadataValue {
    /// Returns the boolean value when the stored type matches.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Integer(_) | Self::String(_) => None,
        }
    }

    /// Returns the integer value when the stored type matches.
    #[must_use]
    pub const fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            Self::Bool(_) | Self::String(_) => None,
        }
    }

    /// Returns the text value when the stored type matches.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            Self::Bool(_) | Self::Integer(_) => None,
        }
    }
}

impl fmt::Display for PermissionMetadataValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => value.fmt(formatter),
            Self::Integer(value) => value.fmt(formatter),
            Self::String(value) => formatter.write_str(value),
        }
    }
}

/// A namespaced metadata key and its optional rule-side context selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionMetadataExpression {
    key: Identifier,
    context: PermissionRuleContext,
}

impl PermissionMetadataExpression {
    /// Creates an expression from validated parts.
    #[must_use]
    pub const fn new(key: Identifier, context: PermissionRuleContext) -> Self {
        Self { key, context }
    }

    /// Parses `namespace:path` or `namespace:path{context=value,...}`.
    ///
    /// # Errors
    ///
    /// Returns an error when the metadata key or context selector is invalid.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionMetadataExpressionError> {
        let value = value.into();
        let Some(context_start) = value.find('{') else {
            let key = parse_permission_metadata_key(value.as_str()).map_err(|source| {
                PermissionMetadataExpressionError::InvalidMetadataKey { value, source }
            })?;
            return Ok(Self::new(key, PermissionRuleContext::Global));
        };

        if !value.ends_with('}') {
            return Err(PermissionMetadataExpressionError::UnclosedContext);
        }

        let key_value = &value[..context_start];
        let key = parse_permission_metadata_key(key_value).map_err(|source| {
            PermissionMetadataExpressionError::InvalidMetadataKey {
                value: key_value.to_owned(),
                source,
            }
        })?;
        let context_value = &value[context_start + 1..value.len() - 1];
        let context = parse_context(context_value)
            .map_err(PermissionMetadataExpressionError::from_context_error)?;
        Ok(Self::new(key, context))
    }

    /// Returns the metadata key.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    /// Returns the rule-side context selector.
    #[must_use]
    pub const fn context(&self) -> &PermissionRuleContext {
        &self.context
    }

    /// Splits the expression into its key and context.
    #[must_use]
    pub fn into_parts(self) -> (Identifier, PermissionRuleContext) {
        (self.key, self.context)
    }
}

impl fmt::Display for PermissionMetadataExpression {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.key.fmt(formatter)?;
        write_context(formatter, &self.context)
    }
}

/// Parses a namespaced permission metadata key.
///
/// # Errors
///
/// Returns an error unless the key is a non-empty `namespace:path` identifier.
pub fn parse_permission_metadata_key(
    value: impl Into<String>,
) -> Result<Identifier, PermissionMetadataKeyError> {
    let value = value.into();
    let Some((namespace, path)) = value.split_once(':') else {
        return Err(PermissionMetadataKeyError::InvalidFormat);
    };
    if namespace.is_empty() {
        return Err(PermissionMetadataKeyError::EmptyNamespace);
    }
    if path.is_empty() {
        return Err(PermissionMetadataKeyError::EmptyPath);
    }
    if path.contains(':') {
        return Err(PermissionMetadataKeyError::InvalidFormat);
    }
    if namespace.split('.').any(str::is_empty) {
        return Err(PermissionMetadataKeyError::InvalidNamespace);
    }
    if path.split(['.', '/']).any(str::is_empty) {
        return Err(PermissionMetadataKeyError::InvalidPath);
    }
    if !Identifier::validate_namespace(namespace) {
        return Err(PermissionMetadataKeyError::InvalidNamespace);
    }
    if !Identifier::validate_path(path) {
        return Err(PermissionMetadataKeyError::InvalidPath);
    }
    Ok(Identifier::new(namespace.to_owned(), path.to_owned()))
}

/// Invalid permission metadata identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMetadataKeyError {
    /// The value does not use exactly one `namespace:path` separator.
    InvalidFormat,
    /// The namespace is empty.
    EmptyNamespace,
    /// The path is empty.
    EmptyPath,
    /// The namespace contains invalid characters or empty segments.
    InvalidNamespace,
    /// The path contains invalid characters or empty segments.
    InvalidPath,
}

impl fmt::Display for PermissionMetadataKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => {
                formatter.write_str("permission metadata key must be a namespaced id")
            }
            Self::EmptyNamespace => {
                formatter.write_str("permission metadata key namespace is empty")
            }
            Self::EmptyPath => formatter.write_str("permission metadata key path is empty"),
            Self::InvalidNamespace => {
                formatter.write_str("permission metadata key namespace contains invalid characters")
            }
            Self::InvalidPath => {
                formatter.write_str("permission metadata key path contains invalid characters")
            }
        }
    }
}

impl Error for PermissionMetadataKeyError {}

/// Invalid permission metadata expression syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionMetadataExpressionError {
    /// The metadata key is invalid.
    InvalidMetadataKey {
        /// Invalid key text.
        value: String,
        /// Key parse error.
        source: PermissionMetadataKeyError,
    },
    /// A context selector starts with `{` but does not end with `}`.
    UnclosedContext,
    /// The context selector contains no entries.
    EmptyContext,
    /// A context entry is not `key=value`.
    InvalidContextEntry(String),
    /// A context entry contains an unsupported value.
    InvalidContextValue {
        /// Context key.
        key: String,
        /// Invalid context value.
        value: String,
    },
    /// The same context key appears more than once.
    DuplicateContextKey(String),
    /// The built-in domain value is invalid.
    InvalidDomain(String),
    /// The built-in loaded-world value is invalid.
    InvalidWorld(String),
    /// A custom context key is invalid.
    InvalidContextKey {
        /// Invalid context key text.
        key: String,
        /// Context key parse error.
        source: PermissionContextKeyError,
    },
    /// The combined rule-side context is invalid.
    InvalidRuleContext(PermissionRuleContextError),
}

impl PermissionMetadataExpressionError {
    fn from_context_error(error: PermissionExpressionContextError) -> Self {
        match error {
            PermissionExpressionContextError::EmptyContext => Self::EmptyContext,
            PermissionExpressionContextError::InvalidContextEntry(entry) => {
                Self::InvalidContextEntry(entry)
            }
            PermissionExpressionContextError::InvalidContextValue { key, value } => {
                Self::InvalidContextValue { key, value }
            }
            PermissionExpressionContextError::DuplicateContextKey(key) => {
                Self::DuplicateContextKey(key)
            }
            PermissionExpressionContextError::InvalidDomain(domain) => Self::InvalidDomain(domain),
            PermissionExpressionContextError::InvalidWorld(world) => Self::InvalidWorld(world),
            PermissionExpressionContextError::InvalidContextKey { key, source } => {
                Self::InvalidContextKey { key, source }
            }
            PermissionExpressionContextError::InvalidRuleContext(source) => {
                Self::InvalidRuleContext(source)
            }
        }
    }
}

impl fmt::Display for PermissionMetadataExpressionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMetadataKey { value, source } => {
                write!(
                    formatter,
                    "invalid permission metadata key '{value}': {source}"
                )
            }
            Self::UnclosedContext => {
                formatter.write_str("permission metadata context selector is not closed")
            }
            Self::EmptyContext => {
                formatter.write_str("permission metadata context selector is empty")
            }
            Self::InvalidContextEntry(entry) => {
                write!(
                    formatter,
                    "invalid permission metadata context entry '{entry}'"
                )
            }
            Self::InvalidContextValue { key, value } => write!(
                formatter,
                "invalid permission metadata context value '{value}' for '{key}'"
            ),
            Self::DuplicateContextKey(key) => write!(
                formatter,
                "permission metadata context key '{key}' appears more than once"
            ),
            Self::InvalidDomain(domain) => write!(formatter, "invalid domain context '{domain}'"),
            Self::InvalidWorld(world) => write!(formatter, "invalid world context '{world}'"),
            Self::InvalidContextKey { key, source } => {
                write!(
                    formatter,
                    "invalid permission context key '{key}': {source}"
                )
            }
            Self::InvalidRuleContext(source) => source.fmt(formatter),
        }
    }
}

impl Error for PermissionMetadataExpressionError {}

/// One permission metadata rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionMetadataEntry {
    key: Identifier,
    context: PermissionRuleContext,
    value: PermissionMetadataValue,
}

impl PermissionMetadataEntry {
    /// Creates a global metadata rule.
    #[must_use]
    pub const fn new(key: Identifier, value: PermissionMetadataValue) -> Self {
        Self {
            key,
            context: PermissionRuleContext::Global,
            value,
        }
    }

    /// Creates a contextual metadata rule.
    #[must_use]
    pub const fn new_with_context(
        key: Identifier,
        context: PermissionRuleContext,
        value: PermissionMetadataValue,
    ) -> Self {
        Self {
            key,
            context,
            value,
        }
    }

    /// Returns the metadata key.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    /// Returns the rule-side context selector.
    #[must_use]
    pub const fn context(&self) -> &PermissionRuleContext {
        &self.context
    }

    /// Returns the configured value.
    #[must_use]
    pub const fn value(&self) -> &PermissionMetadataValue {
        &self.value
    }
}

/// A flat effective permission metadata set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionMetadataSet {
    entries: Vec<PermissionMetadataEntry>,
    sources: Vec<PermissionResolutionSource>,
}

impl PermissionMetadataSet {
    /// Creates an empty metadata set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            sources: Vec::new(),
        }
    }

    /// Creates a subject metadata set from entries.
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = PermissionMetadataEntry>) -> Self {
        let entries = entries.into_iter().collect::<Vec<_>>();
        let sources = vec![PermissionResolutionSource::Subject; entries.len()];
        Self { entries, sources }
    }

    /// Returns all entries in insertion order.
    #[must_use]
    pub fn entries(&self) -> &[PermissionMetadataEntry] {
        &self.entries
    }

    /// Returns whether the set contains no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Adds one subject metadata rule.
    pub fn push(&mut self, entry: PermissionMetadataEntry) {
        self.push_with_source(entry, PermissionResolutionSource::Subject);
    }

    /// Sets one exact global value, replacing any previous exact value.
    pub fn set(&mut self, key: Identifier, value: PermissionMetadataValue) {
        self.set_in(key, PermissionRuleContext::Global, value);
    }

    /// Sets one exact contextual value, replacing any previous exact value.
    pub fn set_in(
        &mut self,
        key: Identifier,
        context: PermissionRuleContext,
        value: PermissionMetadataValue,
    ) {
        self.retain_entries(|entry| entry.key != key || entry.context != context);
        self.push(PermissionMetadataEntry::new_with_context(
            key, context, value,
        ));
    }

    /// Removes one exact global value.
    pub fn unset(&mut self, key: &Identifier) -> bool {
        self.unset_in(key, &PermissionRuleContext::Global)
    }

    /// Removes one exact contextual value.
    pub fn unset_in(&mut self, key: &Identifier, context: &PermissionRuleContext) -> bool {
        let old_len = self.entries.len();
        self.retain_entries(|entry| entry.key() != key || entry.context() != context);
        self.entries.len() != old_len
    }

    /// Resolves one value in the global context.
    #[must_use]
    pub fn resolve(&self, key: &Identifier) -> Option<&PermissionMetadataValue> {
        self.resolve_in(key, &PermissionContext::global())
    }

    /// Resolves one value in an active permission context.
    ///
    /// More-specific contexts win. Ties prefer subject metadata, then group
    /// priority, then the final insertion order for deterministic resolution.
    #[must_use]
    pub fn resolve_in(
        &self,
        key: &Identifier,
        context: &PermissionContext,
    ) -> Option<&PermissionMetadataValue> {
        self.best_candidate(key, context)
            .map(|candidate| self.entries[candidate.entry_index].value())
    }

    /// Resolves one global value and returns the winning rule.
    #[must_use]
    pub fn resolve_detailed(&self, key: &Identifier) -> Option<PermissionMetadataResolution> {
        self.resolve_in_detailed(key, &PermissionContext::global())
    }

    /// Resolves one contextual value and returns the winning rule.
    #[must_use]
    pub fn resolve_in_detailed(
        &self,
        key: &Identifier,
        context: &PermissionContext,
    ) -> Option<PermissionMetadataResolution> {
        self.best_candidate(key, context)
            .map(|candidate| self.resolution(candidate))
    }

    pub(super) fn push_group(
        &mut self,
        entry: PermissionMetadataEntry,
        group: &str,
        group_priority: i32,
    ) {
        self.push_with_source(
            entry,
            PermissionResolutionSource::Group {
                name: group.to_owned(),
                priority: group_priority,
            },
        );
    }

    fn push_with_source(
        &mut self,
        entry: PermissionMetadataEntry,
        source: PermissionResolutionSource,
    ) {
        self.entries.push(entry);
        self.sources.push(source);
    }

    fn retain_entries(&mut self, mut keep: impl FnMut(&PermissionMetadataEntry) -> bool) {
        let entries = mem::take(&mut self.entries);
        let sources = mem::take(&mut self.sources);
        for (entry, source) in entries.into_iter().zip(sources) {
            if keep(&entry) {
                self.entries.push(entry);
                self.sources.push(source);
            }
        }
    }

    fn best_candidate(
        &self,
        key: &Identifier,
        context: &PermissionContext,
    ) -> Option<PermissionMetadataCandidate> {
        let mut best: Option<PermissionMetadataCandidate> = None;
        for (index, (entry, source)) in self.entries.iter().zip(&self.sources).enumerate() {
            if entry.key() != key || !entry.context().matches(context) {
                continue;
            }
            let candidate = PermissionMetadataCandidate {
                entry_index: index,
                context_specificity: entry.context().specificity(),
                source: source.clone(),
            };
            if best
                .as_ref()
                .is_none_or(|current| candidate.order() > current.order())
            {
                best = Some(candidate);
            }
        }
        best
    }

    fn resolution(&self, candidate: PermissionMetadataCandidate) -> PermissionMetadataResolution {
        PermissionMetadataResolution {
            entry: self.entries[candidate.entry_index].clone(),
            source: candidate.source,
            context_specificity: candidate.context_specificity,
            insertion_index: candidate.entry_index,
        }
    }
}

/// Detailed winning permission metadata rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionMetadataResolution {
    entry: PermissionMetadataEntry,
    source: PermissionResolutionSource,
    context_specificity: usize,
    insertion_index: usize,
}

impl PermissionMetadataResolution {
    /// Returns the complete winning rule.
    #[must_use]
    pub const fn entry(&self) -> &PermissionMetadataEntry {
        &self.entry
    }

    /// Returns where the winning rule came from.
    #[must_use]
    pub const fn source(&self) -> &PermissionResolutionSource {
        &self.source
    }

    /// Returns the configured value.
    #[must_use]
    pub const fn value(&self) -> &PermissionMetadataValue {
        self.entry.value()
    }

    /// Returns the winning metadata key.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        self.entry.key()
    }

    /// Returns the winning context constraint.
    #[must_use]
    pub const fn context(&self) -> &PermissionRuleContext {
        self.entry.context()
    }

    /// Returns the context-specificity rank used during resolution.
    #[must_use]
    pub const fn context_specificity(&self) -> usize {
        self.context_specificity
    }

    /// Returns the insertion index used as the final deterministic tie-breaker.
    #[must_use]
    pub const fn insertion_index(&self) -> usize {
        self.insertion_index
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PermissionMetadataCandidate {
    entry_index: usize,
    context_specificity: usize,
    source: PermissionResolutionSource,
}

impl PermissionMetadataCandidate {
    const fn order(&self) -> (usize, usize, i32, usize) {
        (
            self.context_specificity,
            self.source.rank(),
            self.source.tie_priority(),
            self.entry_index,
        )
    }
}
