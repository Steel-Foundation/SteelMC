use std::fmt;

use steel_utils::Identifier;

/// One permission pattern: an optional namespace and a dot-separated path whose
/// final segment may be a `*` wildcard.
///
/// Concrete permissions are [`Identifier`]s. A [`PermissionKey`] is the pattern
/// form used in `allow`/`deny` rules and permission queries. A missing namespace
/// matches every namespace.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionKey {
    namespace: Option<String>,
    segments: Vec<KeySegment>,
    value: String,
}

/// One parsed path segment of a [`PermissionKey`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum KeySegment {
    Literal(String),
    Wildcard,
}

impl PermissionKey {
    /// Parses `namespace:path` or `path`, where the final path segment may be `*`.
    ///
    /// # Errors
    ///
    /// Returns an error for empty namespaces, paths, or segments and misplaced wildcards.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionKeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PermissionKeyError::Empty);
        }

        let (namespace, path) = match value.split_once(':') {
            Some((namespace, path)) => {
                if namespace.is_empty() {
                    return Err(PermissionKeyError::EmptyNamespace);
                }
                if !Identifier::validate_namespace(namespace) {
                    return Err(PermissionKeyError::InvalidNamespace);
                }
                (Some(namespace.to_owned()), path)
            }
            None => (None, value.as_str()),
        };
        if path.is_empty() {
            return Err(PermissionKeyError::EmptyPath);
        }

        let parts = path.split('.').collect::<Vec<_>>();
        let mut segments = Vec::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
            if part.is_empty() {
                return Err(PermissionKeyError::EmptySegment);
            }
            if *part == "*" {
                if index + 1 != parts.len() {
                    return Err(PermissionKeyError::WildcardNotFinal);
                }
                segments.push(KeySegment::Wildcard);
                continue;
            }
            if part.contains('*') {
                return Err(PermissionKeyError::InvalidWildcardSegment);
            }
            validate_permission_segment(part)?;
            segments.push(KeySegment::Literal((*part).to_owned()));
        }

        Ok(Self {
            namespace,
            segments,
            value,
        })
    }

    /// Builds the exact pattern matching one concrete permission.
    #[must_use]
    pub fn from_identifier(identifier: &Identifier) -> Self {
        let segments = identifier
            .path
            .split('.')
            .map(|part| KeySegment::Literal(part.to_owned()))
            .collect();
        Self {
            namespace: Some(identifier.namespace.to_string()),
            segments,
            value: identifier.to_string(),
        }
    }

    /// Returns the canonical textual key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the pattern's namespace, or `None` when it matches every namespace.
    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Returns the authority pattern that grants management of this expression.
    ///
    /// The managed namespace and path are appended to the Steel management prefix
    /// as path segments, so `minecraft:command.give` is managed by
    /// `steel:permission.manage.minecraft.command.give`. A namespace-less expression
    /// appends only its path.
    #[must_use]
    pub fn manage_authority(&self) -> Self {
        let mut segments = Vec::with_capacity(self.segments.len() + 3);
        segments.push(KeySegment::Literal("permission".to_owned()));
        segments.push(KeySegment::Literal("manage".to_owned()));
        let mut value = String::from("steel:permission.manage");
        if let Some(namespace) = &self.namespace {
            value.push('.');
            value.push_str(namespace);
            segments.push(KeySegment::Literal(namespace.clone()));
        }
        for segment in &self.segments {
            value.push('.');
            match segment {
                KeySegment::Literal(literal) => {
                    value.push_str(literal);
                    segments.push(KeySegment::Literal(literal.clone()));
                }
                KeySegment::Wildcard => {
                    value.push('*');
                    segments.push(KeySegment::Wildcard);
                }
            }
        }
        Self {
            namespace: Some(Identifier::STEEL_NAMESPACE.to_owned()),
            segments,
            value,
        }
    }

    /// Returns whether this pattern matches one concrete permission.
    #[must_use]
    pub fn matches_identifier(&self, identifier: &Identifier) -> bool {
        if let Some(namespace) = &self.namespace
            && identifier.namespace != namespace.as_str()
        {
            return false;
        }
        let parts = identifier.path.split('.').collect::<Vec<_>>();
        self.matches_path_parts(&parts)
    }

    /// Returns whether this pattern matches another permission pattern.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        if let Some(namespace) = &self.namespace
            && other.namespace.as_deref() != Some(namespace.as_str())
        {
            return false;
        }
        match self.segments.split_last() {
            Some((KeySegment::Wildcard, literals)) => {
                other.segments.len() > literals.len()
                    && literal_segments_match(literals, &other.segments[..literals.len()])
            }
            _ => self.segments == other.segments,
        }
    }

    fn matches_path_parts(&self, parts: &[&str]) -> bool {
        match self.segments.split_last() {
            Some((KeySegment::Wildcard, literals)) => {
                parts.len() > literals.len() && literal_parts_match(literals, parts)
            }
            _ => parts.len() == self.segments.len() && literal_parts_match(&self.segments, parts),
        }
    }

    pub(super) fn specificity(&self) -> usize {
        let literals = self
            .segments
            .iter()
            .filter(|segment| matches!(segment, KeySegment::Literal(_)))
            .count();
        literals + usize::from(self.namespace.is_some())
    }
}

impl fmt::Display for PermissionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

fn literal_parts_match(literals: &[KeySegment], parts: &[&str]) -> bool {
    literals
        .iter()
        .zip(parts)
        .all(|(literal, part)| matches!(literal, KeySegment::Literal(value) if value == part))
}

fn literal_segments_match(literals: &[KeySegment], segments: &[KeySegment]) -> bool {
    literals
        .iter()
        .zip(segments)
        .all(|(literal, segment)| match (literal, segment) {
            (KeySegment::Literal(left), KeySegment::Literal(right)) => left == right,
            _ => false,
        })
}

/// Returns whether `parent` is an ancestor path of `key` in the same namespace.
pub(super) fn identifier_scopes(parent: &Identifier, key: &Identifier) -> bool {
    if parent.namespace != key.namespace {
        return false;
    }
    if parent.path == key.path {
        return true;
    }
    key.path
        .strip_prefix(parent.path.as_ref())
        .is_some_and(|remaining| remaining.starts_with('.'))
}

/// One validated non-wildcard permission key segment.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionSegment(String);

impl PermissionSegment {
    /// Parses one permission key segment.
    ///
    /// # Errors
    ///
    /// Returns an error when the segment is empty, contains a wildcard, or
    /// contains characters outside lowercase ASCII letters, digits, `_`, and `-`.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionKeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PermissionKeyError::EmptySegment);
        }
        if value.contains('*') {
            return Err(PermissionKeyError::InvalidWildcardSegment);
        }
        validate_permission_segment(&value)?;
        Ok(Self(value))
    }

    /// Returns the validated segment text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_permission_segment(segment: &str) -> Result<(), PermissionKeyError> {
    if segment.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
    }) {
        Ok(())
    } else {
        Err(PermissionKeyError::InvalidSegment)
    }
}

/// Why a permission key failed validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PermissionKeyError {
    /// The complete key is empty.
    #[error("permission key is empty")]
    Empty,
    /// The namespace before `:` is empty.
    #[error("permission namespace is empty")]
    EmptyNamespace,
    /// The namespace contains unsupported characters.
    #[error("permission namespace is invalid")]
    InvalidNamespace,
    /// The path after `:` is empty.
    #[error("permission path is empty")]
    EmptyPath,
    /// A dotted segment is empty.
    #[error("permission key contains an empty segment")]
    EmptySegment,
    /// A wildcard appears before the final segment.
    #[error("permission wildcard must be the final segment")]
    WildcardNotFinal,
    /// A wildcard is embedded within a non-wildcard segment.
    #[error("permission wildcard must occupy the full segment")]
    InvalidWildcardSegment,
    /// A segment contains unsupported characters.
    #[error("permission segment must contain only lowercase letters, numbers, '_' or '-'")]
    InvalidSegment,
}
