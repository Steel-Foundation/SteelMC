use super::{
    Arc, FnServerJob, OP_GROUP, PermissionGroupManager, PermissionGroupManagerError,
    PermissionGroupUpdateError, PermissionGroupsConfig, PermissionSet, PermissionSubjectState,
    Player, PlayerPermissionUpdateError, Server, ServerJobContext, Uuid,
};

pub(super) fn validate_player_permission_group_update<E>(
    manager: &PermissionGroupManager,
    previous_groups: &[String],
    updated_groups: &[String],
) -> Result<(), PlayerPermissionUpdateError<E>> {
    for group in updated_groups {
        let already_assigned = previous_groups.iter().any(|current| current == group);
        if !already_assigned && !manager.contains_group(group) {
            return Err(PlayerPermissionUpdateError::UnknownGroup(group.clone()));
        }
    }
    Ok(())
}

impl Server {
    pub(super) fn apply_cached_or_default_permission_state(&self, player: &Player) -> u64 {
        let state = self
            .player_permission_states
            .read()
            .get(player.gameprofile.id)
            .cloned()
            .unwrap_or_default();
        self.apply_player_permission_state(player, state)
    }

    fn apply_player_permission_state(&self, player: &Player, state: PermissionSubjectState) -> u64 {
        let (groups, overrides, metadata_overrides) = state.into_parts();
        for group in &groups {
            if !self.permission_groups.contains_group(group) {
                log::warn!(
                    "Player {} has unknown permission group {group}",
                    player.gameprofile.name
                );
            }
        }
        let effective = self
            .permission_groups
            .effective_permissions(&groups, &overrides);
        let effective_metadata = self
            .permission_groups
            .effective_metadata(&groups, &metadata_overrides);
        player.set_permission_state(
            groups,
            overrides,
            metadata_overrides,
            effective,
            effective_metadata,
        )
    }

    /// Returns one player's cached persisted permission state.
    #[must_use]
    pub fn player_permission_state(&self, uuid: Uuid) -> Option<PermissionSubjectState> {
        self.player_permission_states.read().get(uuid).cloned()
    }

    /// Returns whether the latest published subject state assigns the operator group.
    #[must_use]
    pub(crate) fn is_operator(&self, uuid: Uuid) -> bool {
        self.player_permission_states
            .read()
            .get(uuid)
            .is_some_and(|state| state.groups().iter().any(|group| group == OP_GROUP))
    }

    /// Captures effective command permissions from the latest published subject and group state.
    #[must_use]
    pub(crate) fn command_permission_snapshot(&self, uuid: Uuid) -> PermissionSet {
        let subject = self.player_permission_state(uuid).unwrap_or_default();
        self.permission_groups
            .effective_permissions(subject.groups(), subject.overrides())
    }

    /// Atomically edits one player's persisted permission state.
    ///
    /// Persistence completes before the cache is published. An online player is
    /// refreshed from the latest cached snapshot at the server job tick stage.
    ///
    /// # Errors
    ///
    /// Returns an edit error, an unknown newly assigned group, or a storage error.
    pub async fn try_update_player_permissions<T, E>(
        self: &Arc<Self>,
        uuid: Uuid,
        update: impl FnOnce(PermissionSubjectState) -> Result<(PermissionSubjectState, T), E> + Send,
    ) -> Result<(PermissionSubjectState, T), PlayerPermissionUpdateError<E>>
    where
        T: Send,
        E: Send,
    {
        let _guard = self.player_permission_updates.lock().await;
        let mut states = self.player_permission_states.read().clone();
        let current = states.get(uuid).cloned().unwrap_or_default();
        let previous_groups = current.groups().to_vec();
        let (updated, result) = update(current).map_err(PlayerPermissionUpdateError::Edit)?;
        validate_player_permission_group_update(
            &self.permission_groups,
            &previous_groups,
            updated.groups(),
        )?;

        if updated.is_empty() {
            states.remove(uuid);
        } else {
            states.set(uuid, updated.clone());
        }
        self.player_data_storage
            .save_permission_subjects(&states)
            .await?;

        *self.player_permission_states.write() = states;
        self.queue_player_permission_refresh(uuid);
        Ok((updated, result))
    }

    /// Replaces the complete permission group config and refreshes online players.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails.
    pub async fn replace_permission_groups(
        self: &Arc<Self>,
        config: PermissionGroupsConfig,
    ) -> Result<(), PermissionGroupManagerError> {
        self.permission_groups.replace_config(config).await?;
        self.queue_online_permission_group_refresh();
        Ok(())
    }

    /// Edits the latest permission group config and refreshes online players.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails.
    pub async fn update_permission_groups(
        self: &Arc<Self>,
        update: impl FnOnce(&mut PermissionGroupsConfig) + Send,
    ) -> Result<(), PermissionGroupManagerError> {
        self.permission_groups.update_config(update).await?;
        self.queue_online_permission_group_refresh();
        Ok(())
    }

    /// Applies a fallible permission group edit and refreshes online players.
    ///
    /// # Errors
    ///
    /// Returns the caller edit error or a validation/persistence error.
    pub async fn try_update_permission_groups<T, E>(
        self: &Arc<Self>,
        update: impl FnOnce(&mut PermissionGroupsConfig) -> Result<T, E> + Send,
    ) -> Result<T, PermissionGroupUpdateError<E>>
    where
        T: Send,
        E: Send,
    {
        let result = self.permission_groups.try_update_config(update).await?;
        self.queue_online_permission_group_refresh();
        Ok(result)
    }

    fn queue_player_permission_refresh(self: &Arc<Self>, uuid: Uuid) {
        self.jobs
            .spawn(FnServerJob::new(move |context: &mut ServerJobContext| {
                if let Some(server) = context.server() {
                    server.refresh_player_permission_state(uuid);
                }
            }));
    }

    pub(crate) fn refresh_player_permission_state(self: &Arc<Self>, uuid: Uuid) {
        let Some(player) = self.online_players.get_by_uuid(&uuid) else {
            return;
        };
        let state = self.player_permission_state(uuid).unwrap_or_default();
        self.apply_player_permission_state(&player, state);
        self.resend_player_permission_context(&player);
    }

    fn queue_online_permission_group_refresh(self: &Arc<Self>) {
        self.jobs
            .spawn(FnServerJob::new(|context: &mut ServerJobContext| {
                if let Some(server) = context.server() {
                    server.refresh_online_permission_groups();
                }
            }));
    }

    fn refresh_online_permission_groups(self: &Arc<Self>) {
        for player in self.get_players() {
            let state = self
                .player_permission_state(player.gameprofile.id)
                .unwrap_or_default();
            self.apply_player_permission_state(&player, state);
            self.resend_player_permission_context(&player);
        }
    }
}
