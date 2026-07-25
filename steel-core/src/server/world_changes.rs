use super::{
    Arc, BlockPos, DomainSwitchRequest, EndGatewayTeleportJob, EndPortalTeleportJob, Entity,
    GlobalPlayerData, NetherPortalTeleportJob, NetworkConnection, PendingWorldChangeToken,
    PersistentPlayerData, Player, PortalKind, ResetReason, RespawnData, Server, SharedEntity,
    World, WorldChangeRequest, can_teleport_between_worlds, change_entity_world,
    clear_pending_world_change, is_allowed_to_enter_portal, is_end_dimension_type,
    is_nether_dimension_type, mem, nether_portal, portal_entity_still_valid,
    world_spawn_transition,
};

impl Server {
    pub(super) fn process_world_changes(self: &Arc<Self>, tick_count: u64, runs_normally: bool) {
        let mut changes = mem::take(&mut *self.pending_world_changes.lock());
        for world in self.worlds.values() {
            changes.extend(world.drain_world_changes());
        }

        for (entity, request) in changes {
            if entity.is_removed() {
                continue;
            }
            match request {
                WorldChangeRequest::Computed(transition) => {
                    change_entity_world(entity, &transition);
                }
                WorldChangeRequest::WorldSpawn { target_world } => {
                    let transition = world_spawn_transition(target_world);
                    change_entity_world(entity, &transition);
                }
                WorldChangeRequest::Portal {
                    portal: PortalKind::Nether,
                    source_world,
                    portal_pos,
                    pending_token,
                } => {
                    self.queue_nether_portal_change(
                        entity,
                        source_world,
                        portal_pos,
                        pending_token,
                        tick_count,
                        runs_normally,
                    );
                }
                WorldChangeRequest::Portal {
                    portal: PortalKind::End,
                    source_world,
                    portal_pos: _,
                    pending_token,
                } => {
                    self.queue_end_portal_change(
                        entity,
                        source_world,
                        pending_token,
                        tick_count,
                        runs_normally,
                    );
                }
                WorldChangeRequest::Portal {
                    portal: PortalKind::EndGateway,
                    source_world,
                    portal_pos,
                    pending_token,
                } => {
                    self.queue_end_gateway_change(
                        entity,
                        source_world,
                        portal_pos,
                        pending_token,
                        tick_count,
                        runs_normally,
                    );
                }
            }
        }
    }

    fn queue_nether_portal_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        portal_pos: BlockPos,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        if !portal_entity_still_valid(&entity, &source_world, pending_token) {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        let Some(target_world) = self.worlds.resolve_nether_portal_target(&source_world) else {
            log::warn!(
                "No Nether portal target world loaded for source world {}",
                source_world.key
            );
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        let to_nether = is_nether_dimension_type(&target_world);
        let approximate_exit_pos = nether_portal::approximate_exit_position(
            &source_world,
            &target_world,
            entity.position(),
        );
        self.jobs.poll_now_or_spawn(
            Arc::downgrade(self),
            tick_count,
            runs_normally,
            NetherPortalTeleportJob::new(
                entity,
                source_world,
                target_world,
                portal_pos,
                approximate_exit_pos,
                to_nether,
                pending_token,
            ),
        );
    }

    fn queue_end_portal_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        if !portal_entity_still_valid(&entity, &source_world, pending_token) {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        if !is_end_dimension_type(&source_world) {
            self.queue_end_entry_portal_change(
                entity,
                source_world,
                pending_token,
                tick_count,
                runs_normally,
            );
            return;
        }

        if entity.as_player().is_some() {
            self.queue_end_portal_player_return_change(
                entity,
                source_world,
                pending_token,
                tick_count,
                runs_normally,
            );
            return;
        }

        self.queue_end_portal_entity_return_change(
            entity,
            source_world,
            pending_token,
            tick_count,
            runs_normally,
        );
    }

    fn queue_end_entry_portal_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        let Some(target_world) = self.worlds.resolve_end_entry_portal_target(&source_world) else {
            log::warn!(
                "No End portal target world loaded for source world {}",
                source_world.key
            );
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        self.jobs.poll_now_or_spawn(
            Arc::downgrade(self),
            tick_count,
            runs_normally,
            EndPortalTeleportJob::entry_to_end(entity, source_world, target_world, pending_token),
        );
    }

    fn queue_end_portal_player_return_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        let (target_world, respawn_data) =
            match self.strict_respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    log::warn!(
                        "No End portal return target world loaded for source world {}: {error}",
                        source_world.key
                    );
                    clear_pending_world_change(&entity, pending_token);
                    return;
                }
            };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        match EndPortalTeleportJob::returning_player(
            Arc::clone(&entity),
            source_world,
            target_world,
            respawn_data,
            pending_token,
        ) {
            Ok(job) => {
                self.jobs
                    .poll_now_or_spawn(Arc::downgrade(self), tick_count, runs_normally, job);
            }
            Err(error) => {
                clear_pending_world_change(&entity, pending_token);
                log::error!("Failed to schedule End portal player return: {error}");
            }
        }
    }

    fn queue_end_portal_entity_return_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        let (target_world, respawn_data) =
            match self.strict_respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    log::warn!(
                        "No End portal return target world loaded for source world {}: {error}",
                        source_world.key
                    );
                    clear_pending_world_change(&entity, pending_token);
                    return;
                }
            };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        self.jobs.poll_now_or_spawn(
            Arc::downgrade(self),
            tick_count,
            runs_normally,
            EndPortalTeleportJob::returning_entity(
                entity,
                source_world,
                target_world,
                respawn_data,
                pending_token,
            ),
        );
    }

    fn queue_end_gateway_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        portal_pos: BlockPos,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        if !portal_entity_still_valid(&entity, &source_world, pending_token) {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        let source_is_end = is_end_dimension_type(&source_world);
        let Some(job) = EndGatewayTeleportJob::new(
            Arc::clone(&entity),
            source_world,
            portal_pos,
            source_is_end,
            pending_token,
        ) else {
            tracing::debug!("End gateway world change ignored because no destination is available");
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        self.jobs
            .poll_now_or_spawn(Arc::downgrade(self), tick_count, runs_normally, job);
    }

    pub(super) fn can_teleport_between_worlds(
        &self,
        entity: &dyn Entity,
        source_world: &World,
        target_world: &World,
    ) -> bool {
        can_teleport_between_worlds(entity, source_world, target_world, |uuid| {
            self.projectile_owner_seen_credits_in_domain(source_world.domain(), uuid)
        })
    }

    fn projectile_owner_seen_credits_in_domain(
        &self,
        domain: &str,
        uuid: &uuid::Uuid,
    ) -> Option<bool> {
        self.worlds
            .values()
            .filter(|world| world.domain() == domain)
            .find_map(|world| {
                world
                    .get_entity_by_uuid(uuid)
                    .and_then(|entity| entity.as_player().map(Player::has_seen_credits))
            })
    }

    fn strict_respawn_world_and_data_for_domain(
        &self,
        domain: &str,
    ) -> Result<(Arc<World>, RespawnData), String> {
        let default_world = self
            .worlds
            .default_world(domain)
            .cloned()
            .ok_or_else(|| format!("domain {domain} has no default world"))?;
        let respawn_data = {
            let level_data = default_world.level_data.read();
            level_data.data().respawn_data_or_local(&default_world.key)
        };
        let target_world = self
            .worlds
            .get(respawn_data.dimension())
            .filter(|world| world.domain() == domain)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "respawn dimension {} is not loaded in domain {domain}",
                    respawn_data.dimension()
                )
            })?;
        Ok((target_world, respawn_data))
    }

    /// Queues a player domain switch for processing at the server tick safe point.
    pub fn queue_domain_switch(
        &self,
        player: Arc<Player>,
        target_domain: String,
    ) -> Result<(), String> {
        if !self.worlds.has_domain(&target_domain) {
            return Err(format!("unknown domain {target_domain}"));
        }

        let current_domain = player.get_world().domain().to_owned();
        if current_domain == target_domain {
            return Err(format!("already in domain {target_domain}"));
        }
        if player.connection.closed() {
            return Err("player is disconnecting".to_owned());
        }
        if !player.begin_domain_switch() {
            return Err("domain switch already in progress".to_owned());
        }

        self.pending_domain_switches
            .lock()
            .push(DomainSwitchRequest {
                player,
                target_domain,
                target_world: None,
                restore_saved_location: true,
            });
        Ok(())
    }

    /// Queues a cross-domain teleport using saved target-domain location or target-world spawn.
    pub fn queue_domain_switch_to_world(
        &self,
        player: Arc<Player>,
        target_world: Arc<World>,
    ) -> Result<(), String> {
        let target_domain = target_world.domain().to_owned();
        if player.connection.closed() {
            return Err("player is disconnecting".to_owned());
        }
        if !player.begin_domain_switch() {
            return Err("domain switch already in progress".to_owned());
        }

        self.pending_domain_switches
            .lock()
            .push(DomainSwitchRequest {
                player,
                target_domain,
                target_world: Some(target_world),
                restore_saved_location: true,
            });
        Ok(())
    }

    pub(super) async fn process_domain_switches(&self) {
        let switches = mem::take(&mut *self.pending_domain_switches.lock());

        for request in switches {
            let player = request.player.clone();
            let player_name = player.gameprofile.name.clone();
            let result = self.process_domain_switch(request).await;
            player.finish_domain_switch();

            if let Err(error) = result {
                log::error!("Failed to switch {player_name} domain: {error}");
                if !player.connection.closed() {
                    player.disconnect("Failed to switch domain");
                }
            }
        }
    }

    async fn process_domain_switch(&self, request: DomainSwitchRequest) -> Result<(), String> {
        let DomainSwitchRequest {
            player,
            target_domain,
            target_world,
            restore_saved_location,
        } = request;
        if player.connection.closed() {
            return Ok(());
        }
        if !self.worlds.has_domain(&target_domain) {
            return Err(format!("unknown domain {target_domain}"));
        }

        let current_domain = player.get_world().domain().to_owned();
        if current_domain == target_domain {
            return Ok(());
        }

        let current_data = PersistentPlayerData::from_player(&player);
        if let Err(e) = self
            .player_data_storage
            .save_domain_data(&current_domain, player.gameprofile.id, &current_data)
            .await
        {
            return Err(format!("failed to save current domain data: {e}"));
        }

        if player.connection.closed() {
            return Ok(());
        }

        let target_state = match self
            .load_domain_player_state(
                &player,
                &target_domain,
                target_world.clone(),
                restore_saved_location,
            )
            .await
        {
            Ok(state) => state,
            Err(error) => {
                return Err(error);
            }
        };

        if player.connection.closed() {
            return Ok(());
        }

        let restore_player = Arc::clone(&player);
        player.reset_after_domain_save_and_restore(target_state.world.clone(), || {
            Self::apply_domain_player_state(&restore_player, &target_state);
        });
        let pos = player.position();
        let rotation = player.rotation();
        if !player.spawn(pos, rotation, ResetReason::WorldChange) {
            return Err("failed to add player to target world".to_owned());
        }
        self.schedule_root_vehicle_restore(&player, &target_state);
        self.schedule_ender_pearl_restores(&player, &target_state);

        if let Err(e) = self
            .player_data_storage
            .save_global(
                player.gameprofile.id,
                &GlobalPlayerData {
                    last_active_domain: target_domain,
                },
            )
            .await
        {
            log::error!(
                "Failed to save global player data for {} after domain switch: {e}",
                player.gameprofile.name
            );
        }

        Ok(())
    }

    /// Queues a world change to be processed after the current tick.
    pub fn queue_world_change(&self, entity: SharedEntity, request: WorldChangeRequest) {
        self.pending_world_changes.lock().push((entity, request));
    }
}
