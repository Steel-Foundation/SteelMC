use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use steel_utils::Identifier;

/// One member referenced by a permission collection.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PermissionCollectionMember {
    /// A concrete permission granted by the collection.
    Permission(Identifier),
    /// Another collection included by this collection.
    Collection(Identifier),
}

impl PermissionCollectionMember {
    /// Parses one collection member.
    ///
    /// A leading `#` marks a nested collection reference; any other value is a
    /// concrete permission.
    ///
    /// # Errors
    ///
    /// Returns an error when the referenced identifier is invalid.
    pub fn parse(value: &str) -> Result<Self, PermissionCollectionError> {
        if let Some(reference) = value.strip_prefix('#') {
            Identifier::from_str(reference)
                .map(Self::Collection)
                .map_err(
                    |reason| PermissionCollectionError::InvalidCollectionReference {
                        value: value.to_owned(),
                        reason,
                    },
                )
        } else {
            Identifier::from_str(value)
                .map(Self::Permission)
                .map_err(
                    |reason| PermissionCollectionError::InvalidPermissionReference {
                        value: value.to_owned(),
                        reason,
                    },
                )
        }
    }
}

impl fmt::Display for PermissionCollectionMember {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Permission(identifier) => write!(formatter, "{identifier}"),
            Self::Collection(identifier) => write!(formatter, "#{identifier}"),
        }
    }
}

/// One named permission collection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionCollection {
    members: Vec<PermissionCollectionMember>,
}

impl PermissionCollection {
    /// Creates a collection from members, ignoring duplicates.
    #[must_use]
    pub fn new(members: impl IntoIterator<Item = PermissionCollectionMember>) -> Self {
        let mut collection = Self::default();
        for member in members {
            collection.push(member);
        }
        collection
    }

    /// Returns the collection members in insertion order.
    #[must_use]
    pub fn members(&self) -> &[PermissionCollectionMember] {
        &self.members
    }

    /// Adds a member, ignoring duplicates.
    pub fn push(&mut self, member: PermissionCollectionMember) {
        if !self.members.contains(&member) {
            self.members.push(member);
        }
    }

    /// Adds a concrete permission member.
    pub fn push_permission(&mut self, permission: Identifier) {
        self.push(PermissionCollectionMember::Permission(permission));
    }
}

/// A set of named permission collections.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionCollections {
    collections: BTreeMap<Identifier, PermissionCollection>,
}

impl PermissionCollections {
    /// Creates an empty collection set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the generated vanilla collections.
    #[must_use]
    pub fn vanilla() -> Self {
        let collections = super::generated_collections::collections()
            .into_iter()
            .map(|(id, permissions)| {
                (
                    id,
                    PermissionCollection::new(
                        permissions
                            .into_iter()
                            .map(PermissionCollectionMember::Permission),
                    ),
                )
            })
            .collect();
        Self { collections }
    }

    /// Returns the collection registered under `id`.
    #[must_use]
    pub fn get(&self, id: &Identifier) -> Option<&PermissionCollection> {
        self.collections.get(id)
    }

    /// Returns whether a collection is registered under `id`.
    #[must_use]
    pub fn contains(&self, id: &Identifier) -> bool {
        self.collections.contains_key(id)
    }

    /// Returns every registered collection id in deterministic order.
    pub fn ids(&self) -> impl Iterator<Item = &Identifier> {
        self.collections.keys()
    }

    /// Returns the concrete permissions granted by `id`, following nested references.
    ///
    /// Unknown collections resolve to nothing.
    #[must_use]
    pub fn expand(&self, id: &Identifier) -> BTreeSet<Identifier> {
        let mut permissions = BTreeSet::new();
        let mut visited = BTreeSet::new();
        self.expand_into(id, &mut visited, &mut permissions);
        permissions
    }

    fn expand_into(
        &self,
        id: &Identifier,
        visited: &mut BTreeSet<Identifier>,
        permissions: &mut BTreeSet<Identifier>,
    ) {
        if !visited.insert(id.clone()) {
            return;
        }
        let Some(collection) = self.collections.get(id) else {
            return;
        };
        for member in collection.members() {
            match member {
                PermissionCollectionMember::Permission(permission) => {
                    permissions.insert(permission.clone());
                }
                PermissionCollectionMember::Collection(nested) => {
                    self.expand_into(nested, visited, permissions);
                }
            }
        }
    }

    /// Parses `collections.toml` configuration into collections.
    ///
    /// # Errors
    ///
    /// Returns an error when a namespace, collection name, or member is invalid.
    pub fn from_config(
        config: &PermissionCollectionsConfig,
    ) -> Result<Self, PermissionCollectionError> {
        let mut collections = BTreeMap::new();
        for (namespace, named) in &config.collections {
            if !Identifier::validate_namespace(namespace) {
                return Err(PermissionCollectionError::InvalidNamespace(
                    namespace.clone(),
                ));
            }
            for (name, members) in named {
                if !Identifier::validate_path(name) {
                    return Err(PermissionCollectionError::InvalidCollectionName {
                        namespace: namespace.clone(),
                        name: name.clone(),
                    });
                }
                let mut collection = PermissionCollection::default();
                for member in members {
                    collection.push(PermissionCollectionMember::parse(member)?);
                }
                collections.insert(Identifier::new(namespace.clone(), name.clone()), collection);
            }
        }
        Ok(Self { collections })
    }

    /// Merges generated vanilla collections, runtime command contributions, and config overrides.
    ///
    /// Runtime contributions extend the built-in collection with the same id. A config
    /// collection replaces the built-in collection with the same id entirely.
    #[must_use]
    pub fn effective(
        command_collections: &BTreeMap<Identifier, Vec<Identifier>>,
        overrides: &Self,
    ) -> Self {
        let mut collections = Self::vanilla();
        for (id, permissions) in command_collections {
            let collection = collections.collections.entry(id.clone()).or_default();
            for permission in permissions {
                collection.push_permission(permission.clone());
            }
        }
        for (id, collection) in &overrides.collections {
            collections
                .collections
                .insert(id.clone(), collection.clone());
        }
        collections
    }
}

/// Parsed `collections.toml` permission collection configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionCollectionsConfig {
    /// Collections keyed by namespace and then by collection name.
    pub collections: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

/// Invalid permission collection configuration or reference.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[expect(
    clippy::enum_variant_names,
    reason = "variants intentionally name what is invalid in a collection definition"
)]
pub enum PermissionCollectionError {
    /// A collection namespace contains unsupported characters.
    #[error("permission collection namespace '{0}' is invalid")]
    InvalidNamespace(String),
    /// A collection name contains unsupported characters.
    #[error("permission collection name '{name}' in namespace '{namespace}' is invalid")]
    InvalidCollectionName {
        /// Collection namespace.
        namespace: String,
        /// Invalid collection name.
        name: String,
    },
    /// A nested collection reference is invalid.
    #[error("invalid permission collection reference '{value}': {reason}")]
    InvalidCollectionReference {
        /// Invalid member text.
        value: String,
        /// Identifier parse failure reason.
        reason: &'static str,
    },
    /// A concrete permission reference is invalid.
    #[error("invalid permission reference '{value}': {reason}")]
    InvalidPermissionReference {
        /// Invalid member text.
        value: String,
        /// Identifier parse failure reason.
        reason: &'static str,
    },
}
