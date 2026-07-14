use std::collections::BTreeMap;

use uuid::Uuid;

use super::{PermissionMetadataSet, PermissionSet};

/// Persisted permission state for one player or other internal subject.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionSubjectState {
    groups: Vec<String>,
    overrides: PermissionSet,
    metadata_overrides: PermissionMetadataSet,
}

impl PermissionSubjectState {
    /// Creates a persisted subject snapshot.
    #[must_use]
    pub const fn new(groups: Vec<String>, overrides: PermissionSet) -> Self {
        Self {
            groups,
            overrides,
            metadata_overrides: PermissionMetadataSet::new(),
        }
    }

    /// Creates a persisted subject snapshot with metadata overrides.
    #[must_use]
    pub const fn new_with_metadata(
        groups: Vec<String>,
        overrides: PermissionSet,
        metadata_overrides: PermissionMetadataSet,
    ) -> Self {
        Self {
            groups,
            overrides,
            metadata_overrides,
        }
    }

    /// Returns assigned group names in persisted order.
    #[must_use]
    pub fn groups(&self) -> &[String] {
        &self.groups
    }

    /// Returns direct permission overrides.
    #[must_use]
    pub const fn overrides(&self) -> &PermissionSet {
        &self.overrides
    }

    /// Returns direct metadata overrides.
    #[must_use]
    pub const fn metadata_overrides(&self) -> &PermissionMetadataSet {
        &self.metadata_overrides
    }

    /// Returns whether this subject has no persisted permission state.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
            && self.overrides.entries().is_empty()
            && self.metadata_overrides.is_empty()
    }

    /// Splits the snapshot into assigned groups and overrides.
    #[must_use]
    pub fn into_parts(self) -> (Vec<String>, PermissionSet, PermissionMetadataSet) {
        (self.groups, self.overrides, self.metadata_overrides)
    }
}

/// In-memory index of persisted player permission snapshots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionSubjectIndex {
    states: BTreeMap<Uuid, PermissionSubjectState>,
}

impl PermissionSubjectIndex {
    /// Creates an empty subject index.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            states: BTreeMap::new(),
        }
    }

    /// Returns one player's persisted permission state.
    #[must_use]
    pub fn get(&self, uuid: Uuid) -> Option<&PermissionSubjectState> {
        self.states.get(&uuid)
    }

    /// Inserts or replaces one player's persisted permission state.
    pub fn set(&mut self, uuid: Uuid, state: PermissionSubjectState) {
        self.states.insert(uuid, state);
    }

    /// Removes one player's persisted permission state.
    pub fn remove(&mut self, uuid: Uuid) -> Option<PermissionSubjectState> {
        self.states.remove(&uuid)
    }

    /// Returns all entries sorted by UUID.
    pub fn entries(&self) -> impl Iterator<Item = (Uuid, &PermissionSubjectState)> {
        self.states.iter().map(|(uuid, state)| (*uuid, state))
    }

    /// Returns the number of persisted subjects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Returns whether no subjects have persisted permission state.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}
