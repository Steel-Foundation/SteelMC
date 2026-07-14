use std::{collections::BTreeSet, error::Error, fmt};

use steel_utils::Identifier;

use super::{
    PermissionContextKey, PermissionContextKeyError, PermissionKey, PermissionKeyError,
    PermissionRuleContext, PermissionRuleContextError,
};

/// A permission key and its optional rule-side context selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionRuleExpression {
    key: PermissionKey,
    context: PermissionRuleContext,
}

impl PermissionRuleExpression {
    /// Creates a permission rule expression from validated parts.
    #[must_use]
    pub const fn new(key: PermissionKey, context: PermissionRuleContext) -> Self {
        Self { key, context }
    }

    /// Parses `permission` or `permission{context=value,...}` syntax.
    ///
    /// # Errors
    ///
    /// Returns an error when the permission key or context selector is invalid.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionRuleExpressionError> {
        let value = value.into();
        let Some(context_start) = value.find('{') else {
            let key = PermissionKey::parse(value.as_str()).map_err(|source| {
                PermissionRuleExpressionError::InvalidPermissionKey { value, source }
            })?;
            return Ok(Self::new(key, PermissionRuleContext::Global));
        };

        if !value.ends_with('}') {
            return Err(PermissionRuleExpressionError::UnclosedContext);
        }

        let key_value = &value[..context_start];
        let key = PermissionKey::parse(key_value).map_err(|source| {
            PermissionRuleExpressionError::InvalidPermissionKey {
                value: key_value.to_owned(),
                source,
            }
        })?;
        let context_value = &value[context_start + 1..value.len() - 1];
        let context = parse_context(context_value)
            .map_err(PermissionRuleExpressionError::from_context_error)?;
        Ok(Self::new(key, context))
    }

    /// Returns the permission key.
    #[must_use]
    pub const fn key(&self) -> &PermissionKey {
        &self.key
    }

    /// Returns the rule-side context selector.
    #[must_use]
    pub const fn context(&self) -> &PermissionRuleContext {
        &self.context
    }

    /// Splits the expression into its key and context.
    #[must_use]
    pub fn into_parts(self) -> (PermissionKey, PermissionRuleContext) {
        (self.key, self.context)
    }
}

impl fmt::Display for PermissionRuleExpression {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key.as_str())?;
        write_context(formatter, &self.context)
    }
}

pub(super) fn parse_context(
    value: &str,
) -> Result<PermissionRuleContext, PermissionExpressionContextError> {
    if value.is_empty() {
        return Err(PermissionExpressionContextError::EmptyContext);
    }

    let mut contexts = Vec::new();
    let mut seen_keys = BTreeSet::new();
    for entry in value.split(',') {
        let Some((key, context_value)) = entry.split_once('=') else {
            return Err(PermissionExpressionContextError::InvalidContextEntry(
                entry.to_owned(),
            ));
        };
        if key.is_empty() || context_value.is_empty() {
            return Err(PermissionExpressionContextError::InvalidContextEntry(
                entry.to_owned(),
            ));
        }
        if context_value
            .chars()
            .any(|character| character.is_whitespace() || "{},=".contains(character))
        {
            return Err(PermissionExpressionContextError::InvalidContextValue {
                key: key.to_owned(),
                value: context_value.to_owned(),
            });
        }
        if !seen_keys.insert(key) {
            return Err(PermissionExpressionContextError::DuplicateContextKey(
                key.to_owned(),
            ));
        }

        let context = match key {
            "domain" => parse_domain_context(context_value)?,
            "world" => parse_world_context(context_value)?,
            custom_key => parse_custom_context(custom_key, context_value)?,
        };
        contexts.push(context);
    }

    PermissionRuleContext::all(contexts)
        .map_err(PermissionExpressionContextError::InvalidRuleContext)
}

fn parse_domain_context(
    value: &str,
) -> Result<PermissionRuleContext, PermissionExpressionContextError> {
    if value.is_empty() || !Identifier::validate_namespace(value) {
        return Err(PermissionExpressionContextError::InvalidDomain(
            value.to_owned(),
        ));
    }
    PermissionRuleContext::domain(value)
        .map_err(PermissionExpressionContextError::InvalidRuleContext)
}

fn parse_world_context(
    value: &str,
) -> Result<PermissionRuleContext, PermissionExpressionContextError> {
    let Some((domain, name)) = value.split_once(':') else {
        return Err(PermissionExpressionContextError::InvalidWorld(
            value.to_owned(),
        ));
    };
    if domain.is_empty()
        || name.is_empty()
        || name.contains([':', '/'])
        || !Identifier::validate_namespace(domain)
        || !Identifier::validate_path(name)
    {
        return Err(PermissionExpressionContextError::InvalidWorld(
            value.to_owned(),
        ));
    }
    Ok(PermissionRuleContext::world(Identifier::new(
        domain.to_owned(),
        name.to_owned(),
    )))
}

fn parse_custom_context(
    key: &str,
    value: &str,
) -> Result<PermissionRuleContext, PermissionExpressionContextError> {
    let key = PermissionContextKey::parse(key).map_err(|source| {
        PermissionExpressionContextError::InvalidContextKey {
            key: key.to_owned(),
            source,
        }
    })?;
    PermissionRuleContext::custom(key, value)
        .map_err(PermissionExpressionContextError::InvalidRuleContext)
}

pub(super) fn write_context(
    formatter: &mut fmt::Formatter<'_>,
    context: &PermissionRuleContext,
) -> fmt::Result {
    if matches!(context, PermissionRuleContext::Global) {
        return Ok(());
    }

    formatter.write_str("{")?;
    write_context_entries(formatter, context, &mut true)?;
    formatter.write_str("}")
}

fn write_context_entries(
    formatter: &mut fmt::Formatter<'_>,
    context: &PermissionRuleContext,
    first: &mut bool,
) -> fmt::Result {
    match context {
        PermissionRuleContext::Global => Ok(()),
        PermissionRuleContext::Domain(domain) => {
            write_context_entry(formatter, first, "domain", domain)
        }
        PermissionRuleContext::World(world) => {
            write_context_entry(formatter, first, "world", world)
        }
        PermissionRuleContext::Custom { key, value } => {
            write_context_entry(formatter, first, key.as_str(), value)
        }
        PermissionRuleContext::All(contexts) => {
            for context in contexts.iter() {
                write_context_entries(formatter, context, first)?;
            }
            Ok(())
        }
    }
}

fn write_context_entry(
    formatter: &mut fmt::Formatter<'_>,
    first: &mut bool,
    key: &str,
    value: impl fmt::Display,
) -> fmt::Result {
    if *first {
        *first = false;
    } else {
        formatter.write_str(",")?;
    }
    write!(formatter, "{key}={value}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PermissionExpressionContextError {
    EmptyContext,
    InvalidContextEntry(String),
    InvalidContextValue {
        key: String,
        value: String,
    },
    DuplicateContextKey(String),
    InvalidDomain(String),
    InvalidWorld(String),
    InvalidContextKey {
        key: String,
        source: PermissionContextKeyError,
    },
    InvalidRuleContext(PermissionRuleContextError),
}

/// Invalid permission rule expression syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionRuleExpressionError {
    /// The permission key is invalid.
    InvalidPermissionKey {
        /// Invalid permission key text.
        value: String,
        /// Parse error.
        source: PermissionKeyError,
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
        /// Parse error.
        source: PermissionContextKeyError,
    },
    /// The combined rule-side context is invalid.
    InvalidRuleContext(PermissionRuleContextError),
}

impl PermissionRuleExpressionError {
    pub(super) fn from_context_error(error: PermissionExpressionContextError) -> Self {
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

impl fmt::Display for PermissionRuleExpressionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPermissionKey { value, source } => {
                write!(formatter, "invalid permission key '{value}': {source}")
            }
            Self::UnclosedContext => {
                formatter.write_str("permission context selector is not closed")
            }
            Self::EmptyContext => formatter.write_str("permission context selector is empty"),
            Self::InvalidContextEntry(entry) => {
                write!(formatter, "invalid permission context entry '{entry}'")
            }
            Self::InvalidContextValue { key, value } => write!(
                formatter,
                "invalid permission context value '{value}' for '{key}'"
            ),
            Self::DuplicateContextKey(key) => {
                write!(
                    formatter,
                    "permission context key '{key}' appears more than once"
                )
            }
            Self::InvalidDomain(domain) => {
                write!(formatter, "invalid domain context '{domain}'")
            }
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

impl Error for PermissionRuleExpressionError {}
