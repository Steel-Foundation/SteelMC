use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use steel_utils::Identifier;

use super::{PermissionGroupRule, PermissionGroupRuleError, PermissionRuleExpression};

/// One member referenced by a permission collection.
///
/// Collections use the same parsed rule type as group `allow`/`deny` entries, so
/// concrete permissions, wildcard patterns, and nested collection references all
/// follow the same syntax.
pub type PermissionCollectionMember = PermissionGroupRule;

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
        self.push(PermissionGroupRule::permission(permission));
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
            .map(|(id, members)| (id, PermissionCollection::new(members)))
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

    /// Returns the permission rules granted by `id`, following nested references.
    ///
    /// Unknown collections resolve to nothing.
    #[must_use]
    pub fn expand(&self, id: &Identifier) -> Vec<PermissionRuleExpression> {
        let mut permissions = Vec::new();
        let mut visited = BTreeSet::new();
        self.expand_into(id, &mut visited, &mut permissions);
        permissions
    }

    fn expand_into(
        &self,
        id: &Identifier,
        visited: &mut BTreeSet<Identifier>,
        permissions: &mut Vec<PermissionRuleExpression>,
    ) {
        if !visited.insert(id.clone()) {
            return;
        }
        let Some(collection) = self.collections.get(id) else {
            return;
        };
        for member in collection.members() {
            match member {
                PermissionGroupRule::Expression(expression) => {
                    if !permissions.contains(expression) {
                        permissions.push(expression.clone());
                    }
                }
                PermissionGroupRule::Collection(nested) => {
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
                    collection.push(PermissionGroupRule::parse(member)?);
                }
                collections.insert(Identifier::new(namespace.clone(), name.clone()), collection);
            }
        }
        Ok(Self { collections })
    }

    /// Merges generated vanilla collections, code-declared contributions, and config overrides.
    ///
    /// Declared contributions extend the built-in collection with the same id. A config
    /// collection replaces the built-in collection with the same id entirely.
    #[must_use]
    pub fn effective(declared: &Self, overrides: &Self) -> Self {
        let mut collections = Self::vanilla();
        collections.extend(declared);
        for (id, collection) in &overrides.collections {
            collections
                .collections
                .insert(id.clone(), collection.clone());
        }
        collections
    }

    /// Adds a member to the collection `id`, creating it when absent.
    pub fn push(&mut self, id: Identifier, member: PermissionCollectionMember) {
        self.collections.entry(id).or_default().push(member);
    }

    /// Adds a permission and its management authority to the collection `id`.
    ///
    /// Every permission contributed by code can then be administered through
    /// `/perms` without a separate authority declaration.
    pub fn push_managed_permission(&mut self, id: Identifier, permission: Identifier) {
        self.push_managed_member(id, PermissionGroupRule::permission(permission));
    }

    /// Adds a member and its matching management authority to the collection `id`.
    ///
    /// Collection references are pushed unchanged; rule expressions receive the
    /// authority derived from their key, preserving wildcard and context selectors.
    pub fn push_managed_member(&mut self, id: Identifier, member: PermissionCollectionMember) {
        let authority = match &member {
            PermissionGroupRule::Expression(expression) => Some(PermissionGroupRule::Expression(
                PermissionRuleExpression::new(
                    expression.key().manage_authority(),
                    expression.context().clone(),
                ),
            )),
            PermissionGroupRule::Collection(_) => None,
        };
        self.push(id.clone(), member);
        if let Some(authority) = authority {
            self.push(id, authority);
        }
    }

    /// Additively merges the members of `other` into this set.
    pub fn extend(&mut self, other: &Self) {
        for (id, collection) in &other.collections {
            let target = self.collections.entry(id.clone()).or_default();
            for member in collection.members() {
                target.push(member.clone());
            }
        }
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
    /// A collection member is not a valid group-style rule.
    #[error(transparent)]
    InvalidRule(#[from] PermissionGroupRuleError),
}
