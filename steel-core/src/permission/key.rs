use std::{error::Error, fmt};

/// One validated dotted permission key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionKey(String);

impl PermissionKey {
    /// Parses a dotted permission key with an optional trailing wildcard.
    ///
    /// # Errors
    ///
    /// Returns an error for empty or invalid segments and misplaced wildcards.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionKeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PermissionKeyError::Empty);
        }

        let segments = value.split('.').collect::<Vec<_>>();
        for (index, segment) in segments.iter().enumerate() {
            if segment.is_empty() {
                return Err(PermissionKeyError::EmptySegment);
            }
            if *segment == "*" {
                if index + 1 != segments.len() {
                    return Err(PermissionKeyError::WildcardNotFinal);
                }
                continue;
            }
            if segment.contains('*') {
                return Err(PermissionKeyError::InvalidWildcardSegment);
            }
            validate_permission_segment(segment)?;
        }

        Ok(Self(value))
    }

    /// Builds a permission key from validated non-wildcard segments.
    ///
    /// # Errors
    ///
    /// Returns an error when no segments are supplied.
    pub fn from_segments(
        segments: impl IntoIterator<Item = PermissionSegment>,
    ) -> Result<Self, PermissionKeyError> {
        let mut value = String::new();
        for segment in segments {
            if !value.is_empty() {
                value.push('.');
            }
            value.push_str(segment.as_str());
        }
        if value.is_empty() {
            return Err(PermissionKeyError::Empty);
        }
        Ok(Self(value))
    }

    /// Returns the validated textual key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Appends one non-wildcard child segment.
    ///
    /// # Errors
    ///
    /// Returns an error when this key ends in a wildcard.
    pub fn child(&self, segment: &PermissionSegment) -> Result<Self, PermissionKeyError> {
        Self::parse(format!("{}.{}", self.0, segment.as_str()))
    }

    /// Returns whether this key pattern matches `other`.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        if self.0 == "*" {
            return true;
        }
        let Some(prefix) = self.0.strip_suffix(".*") else {
            return self == other;
        };
        other
            .0
            .strip_prefix(prefix)
            .is_some_and(|remaining| remaining.starts_with('.'))
    }

    pub(super) fn specificity(&self) -> usize {
        if self.0 == "*" {
            return 0;
        }
        self.0
            .strip_suffix(".*")
            .map_or(self.0.as_str(), |prefix| prefix)
            .split('.')
            .count()
    }

    pub(super) fn scopes(&self, key: &Self) -> bool {
        if self == key {
            return true;
        }
        key.0
            .strip_prefix(self.as_str())
            .is_some_and(|remaining| remaining.starts_with('.'))
    }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionKeyError {
    /// The complete key is empty.
    Empty,
    /// A dotted segment is empty.
    EmptySegment,
    /// A wildcard appears before the final segment.
    WildcardNotFinal,
    /// A wildcard is embedded within a non-wildcard segment.
    InvalidWildcardSegment,
    /// A segment contains unsupported characters.
    InvalidSegment,
}

impl fmt::Display for PermissionKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("permission key is empty"),
            Self::EmptySegment => formatter.write_str("permission key contains an empty segment"),
            Self::WildcardNotFinal => {
                formatter.write_str("permission wildcard must be the final segment")
            }
            Self::InvalidWildcardSegment => {
                formatter.write_str("permission wildcard must occupy the full segment")
            }
            Self::InvalidSegment => formatter.write_str(
                "permission segment must contain only lowercase letters, numbers, '_' or '-'",
            ),
        }
    }
}

impl Error for PermissionKeyError {}
