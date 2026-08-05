use std::ops::{BitAnd, BitOr};

use super::PermissionKey;

/// Boolean permission expression evaluated with tri-state allow/deny/unset semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionExpr {
    /// Resolves one permission key.
    Key(PermissionKey),
    /// A child permission that may also inherit a broader parent grant.
    ScopedKey {
        /// Broad permission accepted as a fallback grant.
        parent: PermissionKey,
        /// Specific child permission being checked.
        key: PermissionKey,
    },
    /// Requires every nested expression to allow.
    All(Vec<Self>),
    /// Requires at least one nested expression to allow.
    Any(Vec<Self>),
}

impl PermissionExpr {
    /// Creates a single-key expression.
    #[must_use]
    pub const fn key(key: PermissionKey) -> Self {
        Self::Key(key)
    }

    /// Creates a child expression with a broad parent fallback.
    #[must_use]
    pub const fn scoped_key(parent: PermissionKey, key: PermissionKey) -> Self {
        Self::ScopedKey { parent, key }
    }
}

impl BitAnd for PermissionExpr {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::All(mut left), Self::All(mut right)) => {
                left.append(&mut right);
                Self::All(left)
            }
            (Self::All(mut left), right) => {
                left.push(right);
                Self::All(left)
            }
            (left, Self::All(mut right)) => {
                right.insert(0, left);
                Self::All(right)
            }
            (left, right) => Self::All(vec![left, right]),
        }
    }
}

impl BitOr for PermissionExpr {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Any(mut left), Self::Any(mut right)) => {
                left.append(&mut right);
                Self::Any(left)
            }
            (Self::Any(mut left), right) => {
                left.push(right);
                Self::Any(left)
            }
            (left, Self::Any(mut right)) => {
                right.insert(0, left);
                Self::Any(right)
            }
            (left, right) => Self::Any(vec![left, right]),
        }
    }
}
