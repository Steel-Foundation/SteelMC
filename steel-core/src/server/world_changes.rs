use super::{
    Arc, BlockPos, DomainSwitchJob, DomainSwitchRequest, EndGatewayTeleportJob,
    EndPortalTeleportJob, Entity, MenuRemovalStatus, NetherPortalTeleportJob, NetworkConnection,
    PendingWorldChangeToken, Player, PlayerAdmissionState, PortalKind, RespawnData, Server,
    SharedEntity, World, WorldChangeRequest, WorldSpawnTeleportJob, can_teleport_between_worlds,
    change_entity_world, clear_pending_world_change, is_allowed_to_enter_portal,
    is_nether_dimension_type, mem, nether_portal, portal_entity_still_valid,
};
use crate::entity::LivingEntity as _;

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
                WorldChangeRequest::WorldSpawn {
                    target_world,
                    pending_token,
                } => self.process_player_world_selection(
                    entity,
                    target_world,
                    pending_token,
                    tick_count,
                    runs_normally,
                ),
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
        if !source_world.is_end_dimension_type() {
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
        let source_is_end = source_world.is_end_dimension_type();
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

    fn begin_player_relocation(
        &self,
        player: &Player,
    ) -> Result<(Arc<World>, PendingWorldChangeToken), String> {
        let current_world = self
            .live_world_for_player(player)
            .ok_or_else(|| "player is not present in a live world".to_owned())?;
        if player.connection.closed() {
            return Err("player is disconnecting".to_owned());
        }
        if player.get_health() <= 0.0 {
            return Err("cannot change worlds while dead".to_owned());
        }
        if player.has_won_game() {
            return Err("cannot change worlds from the End credits screen".to_owned());
        }
        let Some(pending_token) = player.begin_pending_world_change() else {
            return Err("another player relocation is already in progress".to_owned());
        };
        Ok((current_world, pending_token))
    }

    /// Queues a selected loaded world through the player's relocation lease.
    ///
    /// Same-domain selections move to the world's spawn. Cross-domain
    /// selections restore that domain while honoring the explicit target.
    pub fn queue_player_world_selection(
        &self,
        player: Arc<Player>,
        target_world: Arc<World>,
    ) -> Result<(), String> {
        let target_world = self
            .worlds
            .get(&target_world.key)
            .filter(|registered| Arc::ptr_eq(registered, &target_world))
            .cloned()
            .ok_or_else(|| "target world is not the registered loaded world".to_owned())?;
        let target_domain = target_world.domain().to_owned();
        if !self.worlds.has_domain(&target_domain) {
            return Err(format!("unknown domain {target_domain}"));
        }
        let (current_world, pending_token) = self.begin_player_relocation(&player)?;

        if current_world.domain() == target_domain {
            self.pending_world_changes.lock().push((
                player,
                WorldChangeRequest::WorldSpawn {
                    target_world,
                    pending_token,
                },
            ));
            return Ok(());
        }

        if !player.begin_domain_switch(pending_token) {
            player.finish_pending_world_change(pending_token);
            return Err("domain switch already in progress".to_owned());
        }
        self.pending_domain_switches
            .lock()
            .push(DomainSwitchRequest {
                player,
                target_domain,
                target_world: Some(target_world),
                pending_token,
            });
        Ok(())
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

        let (current_world, pending_token) = self.begin_player_relocation(&player)?;
        let current_domain = current_world.domain().to_owned();
        if current_domain == target_domain {
            player.finish_pending_world_change(pending_token);
            return Err(format!("already in domain {target_domain}"));
        }
        if !player.begin_domain_switch(pending_token) {
            player.finish_pending_world_change(pending_token);
            return Err("domain switch already in progress".to_owned());
        }

        self.pending_domain_switches
            .lock()
            .push(DomainSwitchRequest {
                player,
                target_domain,
                target_world: None,
                pending_token,
            });
        Ok(())
    }

    pub(super) fn process_domain_switches(self: &Arc<Self>) {
        let switches = mem::take(&mut *self.pending_domain_switches.lock());

        for request in switches {
            let player = Arc::clone(&request.player);
            let player_name = player.gameprofile.name.clone();
            let pending_token = request.pending_token;
            if let Err(error) = self.start_domain_switch(request) {
                player.finish_domain_switch(pending_token);
                clear_pending_world_change(&(Arc::clone(&player) as SharedEntity), pending_token);
                log::warn!("Did not start domain switch for {player_name}: {error}");
            }
        }
    }

    fn start_domain_switch(self: &Arc<Self>, request: DomainSwitchRequest) -> Result<(), String> {
        let DomainSwitchRequest {
            player,
            target_domain,
            target_world,
            pending_token,
        } = request;
        if player.connection.closed() {
            return Err("player is disconnecting".to_owned());
        }
        if !player.is_domain_switch_queued(pending_token)
            || !player.is_world_change_token_pending(pending_token)
        {
            return Err("domain switch no longer owns the player relocation".to_owned());
        }
        if !self.worlds.has_domain(&target_domain) {
            return Err(format!("unknown domain {target_domain}"));
        }

        let current_world = self
            .live_world_for_player(&player)
            .ok_or_else(|| "player is not present in a live world".to_owned())?;
        let current_domain = current_world.domain().to_owned();
        if current_domain == target_domain {
            return Err(format!("already in domain {target_domain}"));
        }
        if player.get_health() <= 0.0 {
            return Err("player died before the domain switch started".to_owned());
        }
        if player.has_won_game() {
            return Err("player entered the End credits screen".to_owned());
        }
        if let Some(target_world) = target_world.as_ref() {
            let registered = self
                .worlds
                .get(&target_world.key)
                .ok_or_else(|| "target world is no longer loaded".to_owned())?;
            if !Arc::ptr_eq(registered, target_world) || target_world.domain() != target_domain {
                return Err("target world registration changed before the switch".to_owned());
            }
        }

        if player.remove_all_menus() != MenuRemovalStatus::Complete {
            return Err("cannot save domain data while a menu callback is active".to_owned());
        }
        if !self.reserve_player_relocation(&player) {
            return Err("domain switch could not reserve player persistence".to_owned());
        }
        if !player.mark_domain_switch_detached(pending_token) {
            self.release_player_admission(player.gameprofile.id, PlayerAdmissionState::Relocating);
            return Err("domain switch lost ownership before detaching".to_owned());
        }
        let Some((current_data, residence_token)) =
            current_world.detach_player_for_domain_switch(&player)
        else {
            self.release_player_admission(player.gameprofile.id, PlayerAdmissionState::Relocating);
            return Err("player is not present in the current world".to_owned());
        };
        self.jobs.spawn(DomainSwitchJob::new(
            self,
            player,
            current_domain,
            current_data,
            target_domain,
            target_world,
            pending_token,
            residence_token,
        ));

        Ok(())
    }

    fn process_player_world_selection(
        self: &Arc<Self>,
        entity: SharedEntity,
        target_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        if !entity.is_world_change_token_pending(pending_token) {
            return;
        }
        let Some(player) = entity.as_player() else {
            tracing::error!("world selection request does not belong to a player");
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        let Some(source_world) = self.live_world_for_player(player) else {
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        let job = match WorldSpawnTeleportJob::new(
            Arc::clone(&entity),
            source_world,
            target_world,
            pending_token,
        ) {
            Ok(job) => job,
            Err(error) => {
                clear_pending_world_change(&entity, pending_token);
                log::warn!("Did not start player world selection: {error}");
                return;
            }
        };
        self.jobs
            .poll_now_or_spawn(Arc::downgrade(self), tick_count, runs_normally, job);
    }

    /// Queues a world change to be processed after the current tick.
    pub fn queue_world_change(&self, entity: SharedEntity, request: WorldChangeRequest) {
        self.pending_world_changes.lock().push((entity, request));
    }
}
