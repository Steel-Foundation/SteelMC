use super::{
    Identifier, PermissionContext, PermissionExpr, PermissionMetadataSet, PermissionMetadataValue,
    PermissionSet, PermissionState, Player,
};

#[derive(Clone, Debug, Default)]
pub(super) struct PlayerPermissionState {
    pub(super) groups: Vec<String>,
    pub(super) overrides: PermissionSet,
    pub(super) metadata_overrides: PermissionMetadataSet,
    pub(super) effective: PermissionSet,
    pub(super) effective_metadata: PermissionMetadataSet,
    pub(super) version: u64,
}

impl PlayerPermissionState {
    pub(super) fn replace(
        &mut self,
        groups: Vec<String>,
        overrides: PermissionSet,
        metadata_overrides: PermissionMetadataSet,
        effective: PermissionSet,
        effective_metadata: PermissionMetadataSet,
    ) -> u64 {
        let version = self.version.wrapping_add(1);
        *self = Self {
            groups,
            overrides,
            metadata_overrides,
            effective,
            effective_metadata,
            version,
        };
        version
    }
}

impl Player {
    /// Replaces assigned groups, direct overrides, metadata, and effective state.
    pub fn set_permission_state(
        &self,
        groups: Vec<String>,
        overrides: PermissionSet,
        metadata_overrides: PermissionMetadataSet,
        effective: PermissionSet,
        effective_metadata: PermissionMetadataSet,
    ) -> u64 {
        self.permissions.lock().replace(
            groups,
            overrides,
            metadata_overrides,
            effective,
            effective_metadata,
        )
    }

    /// Returns a snapshot of effective permissions.
    #[must_use]
    pub fn permissions(&self) -> PermissionSet {
        self.permissions.lock().effective.clone()
    }

    /// Returns assigned permission groups.
    #[must_use]
    pub fn permission_groups(&self) -> Vec<String> {
        self.permissions.lock().groups.clone()
    }

    /// Returns whether the latest published subject state assigns the operator group.
    #[must_use]
    pub(crate) fn is_operator(&self) -> bool {
        self.server
            .upgrade()
            .is_some_and(|server| server.is_operator(self.gameprofile.id))
    }

    /// Returns direct permission overrides.
    #[must_use]
    pub fn permission_overrides(&self) -> PermissionSet {
        self.permissions.lock().overrides.clone()
    }

    /// Returns direct permission metadata overrides.
    #[must_use]
    pub fn permission_metadata_overrides(&self) -> PermissionMetadataSet {
        self.permissions.lock().metadata_overrides.clone()
    }

    /// Returns the current permission snapshot version.
    #[must_use]
    pub fn permission_state_version(&self) -> u64 {
        self.permissions.lock().version
    }

    /// Returns whether the player satisfies an expression in their current world.
    #[must_use]
    pub fn has_permission(&self, permission: &PermissionExpr) -> bool {
        let world = self.get_world();
        let context = PermissionContext::for_world(world.key.clone());
        self.has_permission_in(permission, &context)
    }

    /// Returns whether the player satisfies an expression in an explicit context.
    #[must_use]
    pub fn has_permission_in(
        &self,
        permission: &PermissionExpr,
        context: &PermissionContext,
    ) -> bool {
        self.permissions
            .lock()
            .effective
            .allows_in(permission, context)
    }

    /// Resolves an expression in the player's current world.
    #[must_use]
    pub fn permission_state(&self, permission: &PermissionExpr) -> Option<PermissionState> {
        let world = self.get_world();
        let context = PermissionContext::for_world(world.key.clone());
        self.permission_state_in(permission, &context)
    }

    /// Resolves an expression in an explicit context.
    #[must_use]
    pub fn permission_state_in(
        &self,
        permission: &PermissionExpr,
        context: &PermissionContext,
    ) -> Option<PermissionState> {
        self.permissions
            .lock()
            .effective
            .resolve_in(permission, context)
    }

    /// Resolves one permission metadata value in the player's current world.
    #[must_use]
    pub fn permission_metadata(&self, key: &Identifier) -> Option<PermissionMetadataValue> {
        let world = self.get_world();
        let context = PermissionContext::for_world(world.key.clone());
        self.permission_metadata_in(key, &context)
    }

    /// Resolves one permission metadata value in an explicit context.
    #[must_use]
    pub fn permission_metadata_in(
        &self,
        key: &Identifier,
        context: &PermissionContext,
    ) -> Option<PermissionMetadataValue> {
        self.permissions
            .lock()
            .effective_metadata
            .resolve_in(key, context)
            .cloned()
    }
}
