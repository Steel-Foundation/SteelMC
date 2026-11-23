use steel_utils::Identifier;

use super::{PermissionKey, PermissionKeyError};

/// One permission declared by code before startup.
///
/// A declaration always makes the key discoverable by `/perms`. Setting a collection
/// additionally makes the permission a managed member of that collection, so groups
/// referencing the collection can grant it. `grant_permission_to` adds the key to an
/// existing collection; `grant_collection_to` references the declared collection
/// from another collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionDeclaration {
    key: PermissionKey,
    collection: Option<Identifier>,
    permission_grants: Vec<Identifier>,
    collection_grants: Vec<Identifier>,
}

impl PermissionDeclaration {
    /// Validates and creates a declaration for one permission key.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is not a valid permission key.
    pub fn new(key: impl Into<String>) -> Result<Self, PermissionDeclarationError> {
        let value = key.into();
        let key = PermissionKey::parse(value.clone())
            .map_err(|source| PermissionDeclarationError::InvalidKey { value, source })?;
        Ok(Self {
            key,
            collection: None,
            permission_grants: Vec::new(),
            collection_grants: Vec::new(),
        })
    }

    /// Makes this permission a managed member of `collection`.
    ///
    /// The collection is created when it does not exist yet. Its matching
    /// `steel:permission.manage.*` authority is added alongside the permission.
    #[must_use]
    pub fn in_collection(mut self, collection: Identifier) -> Self {
        self.collection = Some(collection);
        self
    }

    /// Adds this permission directly to `collection`.
    ///
    /// Use this to contribute one permission to an existing collection, including
    /// a generated vanilla collection such as `minecraft:moderators`.
    #[must_use]
    pub fn grant_permission_to(mut self, collection: Identifier) -> Self {
        self.permission_grants.push(collection);
        self
    }

    /// References the collection declared by [`Self::in_collection`] from `collection`.
    ///
    /// Every member of the declared collection is granted through the reference.
    /// This requires [`Self::in_collection`] to have been set.
    #[must_use]
    pub fn grant_collection_to(mut self, collection: Identifier) -> Self {
        self.collection_grants.push(collection);
        self
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        PermissionKey,
        Option<Identifier>,
        Vec<Identifier>,
        Vec<Identifier>,
    ) {
        (
            self.key,
            self.collection,
            self.permission_grants,
            self.collection_grants,
        )
    }
}

/// A permission declaration failed validation.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PermissionDeclarationError {
    /// The declared permission key is invalid.
    #[error("invalid declared permission '{value}': {source}")]
    InvalidKey {
        /// Original key text.
        value: String,
        /// Validation failure.
        source: PermissionKeyError,
    },
}
