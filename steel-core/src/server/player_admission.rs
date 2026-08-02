use super::{
    Arc, CPlayerInfoUpdate, CRemovePlayerInfo, ClientPacket, ConnectionProtocol, DomainPlayerState,
    EncodedPacket, Entity, GlobalPlayerData, Instant, JoinSet, NetworkConnection,
    PendingWorldChangeToken, PersistentPlayerData, Player, ResetReason, SegQueue, Server,
    SyncMutex, Uuid, mpsc,
};

pub(super) struct PendingPlayerJoin {
    pub(super) player: Arc<Player>,
    pub(super) state: Result<DomainPlayerState, String>,
}

pub(super) struct PendingPlayerDisconnect {
    player: Arc<Player>,
    domain: String,
    player_data: Arc<PersistentPlayerData>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PlayerAdmissionState {
    Joining,
    Relocating,
    Disconnecting,
}

pub(super) struct PlayerJoinQueue {
    sender: mpsc::Sender<PendingPlayerJoin>,
    receiver: SyncMutex<mpsc::Receiver<PendingPlayerJoin>>,
}

impl PlayerJoinQueue {
    pub(super) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: SyncMutex::new(receiver),
        }
    }

    fn send(&self, join: PendingPlayerJoin) {
        let _ = self.sender.send(join);
    }

    fn drain(&self) -> Vec<PendingPlayerJoin> {
        let receiver = self.receiver.lock();
        let mut joins = Vec::new();
        while let Ok(join) = receiver.try_recv() {
            joins.push(join);
        }
        joins
    }
}

pub(super) struct PlayerDisconnectQueue {
    queued: SegQueue<Arc<Player>>,
    prepared: SegQueue<PendingPlayerDisconnect>,
}

impl PlayerDisconnectQueue {
    pub(super) const fn new() -> Self {
        Self {
            queued: SegQueue::new(),
            prepared: SegQueue::new(),
        }
    }

    fn send(&self, player: Arc<Player>) {
        self.queued.push(player);
    }

    fn pop(&self) -> Option<Arc<Player>> {
        self.queued.pop()
    }

    fn send_prepared(&self, disconnect: PendingPlayerDisconnect) {
        self.prepared.push(disconnect);
    }

    fn drain_prepared(&self) -> Vec<PendingPlayerDisconnect> {
        let mut prepared = Vec::new();
        while let Some(disconnect) = self.prepared.pop() {
            prepared.push(disconnect);
        }
        prepared
    }

    pub(super) fn clear(&self) {
        while self.queued.pop().is_some() {}
        while self.prepared.pop().is_some() {}
    }
}

impl Server {
    /// Queues initial player join work.
    ///
    /// Persistent data is loaded asynchronously, then world insertion is finalized at the
    /// game tick safe point so the socket reader can enter play immediately.
    pub fn queue_player_join(self: &Arc<Self>, player: Arc<Player>) {
        if player.connection.closed() {
            return;
        }
        if !self.reserve_player_join(&player) {
            player.disconnect("You are already connected to this server");
            return;
        }

        let server = Arc::clone(self);
        tokio::spawn(async move {
            let state = server.prepare_player_join(&player).await;
            server
                .pending_player_joins
                .send(PendingPlayerJoin { player, state });
        });
    }

    async fn prepare_player_join(&self, player: &Player) -> Result<DomainPlayerState, String> {
        let target_domain = self.load_join_domain(player).await?;
        self.load_domain_player_state(player, &target_domain, None)
            .await
    }

    pub(super) fn process_player_joins(self: &Arc<Self>) {
        for join in self.pending_player_joins.drain() {
            self.finish_prepared_player_join(join);
        }
    }

    pub(super) fn finish_prepared_player_join(self: &Arc<Self>, join: PendingPlayerJoin) {
        let PendingPlayerJoin { player, state } = join;
        let uuid = player.gameprofile.id;
        if player.connection.closed() {
            self.release_player_admission(uuid, PlayerAdmissionState::Joining);
            return;
        }

        let state = match state {
            Ok(state) => state,
            Err(error) => {
                self.release_player_admission(uuid, PlayerAdmissionState::Joining);
                log::error!(
                    "Failed to load player data for {}: {error}",
                    player.gameprofile.name
                );
                player.disconnect("Failed to load player data");
                return;
            }
        };

        if !self.admit_reserved_player(Arc::clone(&player)) {
            player.disconnect("You are already connected to this server");
            return;
        }

        self.apply_cached_or_default_permission_state(&player);
        Self::apply_domain_player_state(&player, &state);
        self.send_login_packet(&player, &state.world);

        player.reset(Arc::clone(&state.world), ResetReason::InitialJoin);
        Self::apply_domain_player_state(&player, &state);
        let residence_token = player.domain_residence_token();
        let restores = self.prepare_domain_restores(&player, &state);
        if !Self::install_domain_restores(&player, residence_token, &restores) {
            tracing::error!(
                player = %player.gameprofile.name,
                "Initial admission lost its domain residence before restore installation"
            );
            player.connection.close();
            self.remove_online_player_sync(&player);
            return;
        }
        let pos = player.position();
        let rotation = player.rotation();
        // The client drops a player entity spawn when it does not already know that
        // player's profile. Vanilla publishes player info before adding the player to
        // the level, which can immediately start entity tracking for existing players.
        self.sync_tab_list(&player);
        let admitted = player.spawn(pos, rotation, ResetReason::InitialJoin);
        if !admitted {
            self.remove_online_player_sync(&player);
            self.broadcast_to_online(CRemovePlayerInfo { uuids: vec![uuid] });
            return;
        }
        let previous_name = self.record_known_player(&player.gameprofile);
        self.broadcast_player_join_message(&player, previous_name.as_deref());
        if player.mark_joined_world() {
            player.send_inventory_to_remote();
        }
        self.schedule_domain_restores(&player, residence_token, restores);
        if player.connection.closed() {
            self.queue_player_disconnect(player);
        }
    }

    pub(super) fn reserve_player_join(&self, player: &Player) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.contains_key(&uuid) {
            return false;
        }
        if self.online_players.get_by_uuid(&uuid).is_some() {
            return false;
        }
        admissions
            .insert(uuid, PlayerAdmissionState::Joining)
            .is_none()
    }

    fn admit_reserved_player(&self, player: Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.get(&uuid) != Some(&PlayerAdmissionState::Joining) {
            return false;
        }

        let admitted = self.online_players.insert(player);
        let _ = admissions.remove(&uuid);
        admitted
    }

    fn reserve_player_disconnect(&self, player: &Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.contains_key(&uuid) {
            return false;
        }
        if !self
            .online_players
            .get_by_uuid(&uuid)
            .is_some_and(|current| Arc::ptr_eq(&current, player))
        {
            return false;
        }
        admissions
            .insert(uuid, PlayerAdmissionState::Disconnecting)
            .is_none()
    }

    pub(super) fn reserve_player_relocation(&self, player: &Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.contains_key(&uuid) {
            return false;
        }
        if !self
            .online_players
            .get_by_uuid(&uuid)
            .is_some_and(|current| Arc::ptr_eq(&current, player))
        {
            return false;
        }
        admissions
            .insert(uuid, PlayerAdmissionState::Relocating)
            .is_none()
    }

    fn transition_player_relocation_to_disconnect(&self, player: &Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.get(&uuid) != Some(&PlayerAdmissionState::Relocating) {
            return false;
        }
        if !self
            .online_players
            .get_by_uuid(&uuid)
            .is_some_and(|current| Arc::ptr_eq(&current, player))
        {
            return false;
        }
        let _ = admissions.insert(uuid, PlayerAdmissionState::Disconnecting);
        true
    }

    pub(super) fn release_player_admission(&self, uuid: Uuid, state: PlayerAdmissionState) {
        let mut admissions = self.player_admissions.lock();
        if admissions.get(&uuid) == Some(&state) {
            let _ = admissions.remove(&uuid);
        }
    }

    fn remove_online_player_sync(&self, player: &Arc<Player>) {
        let _ = self.online_players.remove_player_sync(player);
    }

    pub(crate) fn queue_player_disconnect(&self, player: Arc<Player>) {
        debug_assert!(
            player.connection.closed(),
            "only closed players may enter the disconnect queue"
        );
        self.pending_player_disconnects.send(player);
    }

    pub(crate) fn queue_detached_player_disconnect(
        &self,
        player: Arc<Player>,
        domain: String,
        player_data: Arc<PersistentPlayerData>,
        pending_token: PendingWorldChangeToken,
    ) {
        let uuid = player.gameprofile.id;
        let live_memberships = self
            .worlds
            .values()
            .filter(|world| world.contains_player(&player))
            .cloned()
            .collect::<Vec<_>>();
        if !live_memberships.is_empty() {
            tracing::error!(
                player = %player.gameprofile.name,
                membership_count = live_memberships.len(),
                "Cleaning live world membership after detached player admission failed"
            );
            for world in live_memberships {
                world.remove_player_for_world_change(&player);
            }
        }

        player.finish_player_transition(pending_token);
        player.finish_pending_world_change(pending_token);
        if !self.reserve_player_disconnect(&player) {
            return;
        }

        self.broadcast_player_leave_message(&player);
        self.remove_online_player_sync(&player);
        self.broadcast_to_online(CRemovePlayerInfo { uuids: vec![uuid] });
        self.pending_player_disconnects
            .send_prepared(PendingPlayerDisconnect {
                player,
                domain,
                player_data,
            });
    }

    pub(crate) fn queue_relocating_player_disconnect(
        &self,
        player: Arc<Player>,
        domain: String,
        player_data: Arc<PersistentPlayerData>,
        pending_token: PendingWorldChangeToken,
    ) {
        let uuid = player.gameprofile.id;
        let live_memberships = self
            .worlds
            .values()
            .filter(|world| world.contains_player(&player))
            .cloned()
            .collect::<Vec<_>>();
        if !live_memberships.is_empty() {
            tracing::error!(
                player = %player.gameprofile.name,
                membership_count = live_memberships.len(),
                "Cleaning live world membership after relocating player admission failed"
            );
            for world in live_memberships {
                world.remove_player_for_world_change(&player);
            }
        }

        player.finish_player_transition(pending_token);
        player.finish_pending_world_change(pending_token);
        if !self.transition_player_relocation_to_disconnect(&player) {
            tracing::error!(
                player = %player.gameprofile.name,
                "Relocating player lost its exclusive disconnect-save ownership"
            );
            return;
        }

        self.broadcast_player_leave_message(&player);
        self.remove_online_player_sync(&player);
        self.broadcast_to_online(CRemovePlayerInfo { uuids: vec![uuid] });
        self.pending_player_disconnects
            .send_prepared(PendingPlayerDisconnect {
                player,
                domain,
                player_data,
            });
    }

    pub(super) fn process_player_disconnects(&self) -> Vec<PendingPlayerDisconnect> {
        let mut pending = Vec::new();
        while let Some(player) = self.pending_player_disconnects.pop() {
            if let Some(disconnect) = self.process_player_disconnect(player) {
                pending.push(disconnect);
            }
        }

        if !pending.is_empty() {
            // Steel batches the protocol-supported UUID list to avoid quadratic broadcast work
            // during mass disconnects; Vanilla normally emits one packet per player.
            let uuids = pending
                .iter()
                .map(|disconnect| disconnect.player.gameprofile.id)
                .collect();
            self.broadcast_to_online(CRemovePlayerInfo { uuids });
        }
        pending
    }

    pub(super) fn process_player_disconnect(
        &self,
        player: Arc<Player>,
    ) -> Option<PendingPlayerDisconnect> {
        let uuid = player.gameprofile.id;
        if !self.reserve_player_disconnect(&player) {
            return None;
        }

        let world = player.get_world();
        let (player, domain, player_data) = world.detach_player_for_disconnect(Arc::clone(&player));

        // Vanilla broadcasts before removing the player from its global player list.
        self.broadcast_player_leave_message(&player);
        let player = self.online_players.remove_player_sync(&player);

        let Some(player) = player else {
            self.release_player_admission(uuid, PlayerAdmissionState::Disconnecting);
            return None;
        };

        Some(PendingPlayerDisconnect {
            player,
            domain,
            player_data: Arc::new(player_data),
        })
    }

    async fn save_disconnected_player(&self, pending: PendingPlayerDisconnect) {
        let PendingPlayerDisconnect {
            player,
            domain,
            player_data,
        } = pending;
        let uuid = player.gameprofile.id;
        let start = Instant::now();

        if let Err(e) = self
            .player_data_storage
            .save_domain_data(&domain, uuid, player_data.as_ref())
            .await
        {
            log::error!("Failed to save player domain data for {uuid}: {e}");
        }
        if let Err(e) = self
            .player_data_storage
            .save_global(
                uuid,
                &GlobalPlayerData {
                    last_active_domain: domain,
                },
            )
            .await
        {
            log::error!("Failed to save global player data for {uuid}: {e}");
        }

        player.cleanup();
        self.release_player_admission(uuid, PlayerAdmissionState::Disconnecting);
        log::info!("Player {uuid} removed in {:?}", start.elapsed());
    }

    pub(super) fn start_player_disconnect_saves(self: &Arc<Self>, saves: &mut JoinSet<()>) {
        let mut pending = self.pending_player_disconnects.drain_prepared();
        pending.extend(self.process_player_disconnects());
        for pending in pending {
            let server = Arc::clone(self);
            saves.spawn(async move {
                server.save_disconnected_player(pending).await;
            });
        }

        while let Some(result) = saves.try_join_next() {
            if let Err(error) = result {
                log::error!("Player disconnect save task failed: {error}");
            }
        }
    }

    /// Broadcasts a packet to every online player, regardless of world membership.
    pub fn broadcast_to_online<P: ClientPacket>(&self, packet: P) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.config.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.online_players.iter_players(|_, player| {
            player.connection.send_encoded(encoded.clone());
            true
        });
    }

    pub(super) fn broadcast_to_online_with<P: ClientPacket, F: Fn(&Player) -> P>(&self, packet: F) {
        self.online_players.iter_players(|_, player| {
            player.send_packet(packet(player));
            true
        });
    }

    /// Sends full tab list synchronization for a newly joined player.
    ///
    /// Server membership mirrors vanilla `PlayerList`; world entity spawning remains
    /// owned by the per-world entity tracker.
    fn sync_tab_list(&self, player: &Arc<Player>) {
        self.online_players.iter_players(|_, existing_player| {
            if existing_player.gameprofile.id == player.gameprofile.id {
                return true;
            }

            let add_existing = CPlayerInfoUpdate::create_player_initializing(
                existing_player.gameprofile.id,
                existing_player.gameprofile.name.clone(),
                existing_player.gameprofile.properties.clone(),
                existing_player.game_mode().into(),
                existing_player.connection.latency(),
                None,
                true,
            );
            player.send_packet(add_existing);

            if let Some(session) = existing_player.chat_session()
                && let Ok(protocol_data) = session.as_data().to_protocol_data()
            {
                player.send_packet(CPlayerInfoUpdate::update_chat_session(
                    existing_player.gameprofile.id,
                    protocol_data,
                ));
            }

            true
        });

        let player_info_packet = CPlayerInfoUpdate::create_player_initializing(
            player.gameprofile.id,
            player.gameprofile.name.clone(),
            player.gameprofile.properties.clone(),
            player.game_mode().into(),
            player.connection.latency(),
            None,
            true,
        );
        self.broadcast_to_online(player_info_packet);
    }

    pub(super) fn broadcast_player_latency_updates(&self) {
        let mut latency_entries = Vec::new();
        self.online_players.iter_players(|uuid, player| {
            latency_entries.push((*uuid, player.connection.latency()));
            true
        });

        if !latency_entries.is_empty() {
            self.broadcast_to_online(CPlayerInfoUpdate::update_latency(latency_entries));
        }
    }
}
