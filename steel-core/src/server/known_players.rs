use super::{
    Arc, GameProfile, KnownPlayer, KnownPlayerNameLookup, KnownPlayers, ProfileLookupError, Server,
    Uuid, io, is_valid_player_name, lookup_online_profile, offline_uuid,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UncachedPlayerTarget {
    DirectUuid(Uuid),
    OfflineName,
    OnlineName,
}

pub(super) fn classify_uncached_player_target(
    target: &str,
    online_mode: bool,
) -> UncachedPlayerTarget {
    if let Ok(uuid) = Uuid::parse_str(target) {
        return UncachedPlayerTarget::DirectUuid(uuid);
    }
    if online_mode {
        UncachedPlayerTarget::OnlineName
    } else {
        UncachedPlayerTarget::OfflineName
    }
}

pub(super) fn direct_uuid_profile(uuid: Uuid) -> KnownPlayer {
    KnownPlayer::new(uuid, uuid.to_string())
}

pub(super) struct KnownPlayerCacheState {
    players: KnownPlayers,
    generation: u64,
    worker_running: bool,
    closed: bool,
}

impl KnownPlayerCacheState {
    pub(super) const fn new(players: KnownPlayers) -> Self {
        Self {
            players,
            generation: 0,
            worker_running: false,
            closed: false,
        }
    }

    pub(super) fn record(&mut self, uuid: Uuid, name: String) -> bool {
        if self.closed || !self.players.record(uuid, name) {
            return false;
        }
        self.mark_changed()
    }

    pub(super) const fn mark_changed(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.worker_running {
            false
        } else {
            self.worker_running = true;
            true
        }
    }

    pub(super) fn snapshot(&self) -> (KnownPlayers, u64) {
        (self.players.clone(), self.generation)
    }

    pub(super) const fn is_current(&self, generation: u64) -> bool {
        !self.closed && self.generation == generation
    }

    pub(super) const fn finish_save(&mut self, generation: u64) -> KnownPlayerSaveStep {
        if !self.closed && self.generation != generation {
            KnownPlayerSaveStep::SaveAgain
        } else {
            self.worker_running = false;
            KnownPlayerSaveStep::Finished
        }
    }

    pub(super) fn close_if_idle(&mut self) -> Option<KnownPlayers> {
        if self.worker_running {
            return None;
        }
        self.closed = true;
        Some(self.players.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KnownPlayerSaveStep {
    SaveAgain,
    Finished,
}

impl Server {
    /// Returns a snapshot of player identities known to this server.
    #[must_use]
    pub fn known_players(&self) -> KnownPlayers {
        self.known_players.lock().players.clone()
    }

    /// Records a connected player identity in the persistent profile cache.
    /// Returns the previous cached name for this UUID, if any.
    pub fn record_known_player(self: &Arc<Self>, profile: &GameProfile) -> Option<String> {
        let mut known = self.known_players.lock();
        let previous = known
            .players
            .by_uuid(profile.id)
            .map(|entry| entry.last_known_name().to_owned());
        let start_worker = known.record(profile.id, profile.name.clone());
        drop(known);
        if start_worker {
            self.start_known_player_save_worker();
        }
        previous
    }

    /// Records a UUID and last-known name in the persistent profile cache.
    pub fn record_known_profile(self: &Arc<Self>, uuid: Uuid, last_known_name: impl Into<String>) {
        let start_worker = self
            .known_players
            .lock()
            .record(uuid, last_known_name.into());
        if start_worker {
            self.start_known_player_save_worker();
        }
    }

    /// Resolves a vanilla game-profile command target by name or UUID.
    ///
    /// Online players and cached profiles are checked first. Uncached UUIDs remain
    /// direct UUID targets in either server mode. Offline-mode names use vanilla's
    /// deterministic UUID, while online mode queries the configured profile service.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile is unknown or the profile service fails.
    pub async fn resolve_player_profile(
        self: &Arc<Self>,
        name: &str,
    ) -> Result<KnownPlayer, ProfileLookupError> {
        if let Some(profile) = self.cached_player_profile(name) {
            return Ok(profile);
        }

        match classify_uncached_player_target(name, self.config.online_mode) {
            UncachedPlayerTarget::DirectUuid(uuid) => {
                // No verified name is available, so use the canonical UUID for
                // feedback without adding a synthetic identity-cache entry.
                return Ok(direct_uuid_profile(uuid));
            }
            UncachedPlayerTarget::OfflineName => {
                let profile = KnownPlayer::new(offline_uuid(name), name.to_owned());
                self.record_known_profile(profile.uuid(), profile.last_known_name().to_owned());
                return Ok(profile);
            }
            UncachedPlayerTarget::OnlineName => {}
        }
        if !is_valid_player_name(name) {
            return Err(ProfileLookupError::UnknownPlayer(name.to_owned()));
        }

        let profile = lookup_online_profile(
            &self.profile_lookup_client,
            self.config.profile_server.as_deref(),
            name,
        )
        .await?;
        self.record_known_profile(profile.uuid(), profile.last_known_name().to_owned());
        Ok(profile)
    }

    fn cached_player_profile(self: &Arc<Self>, name: &str) -> Option<KnownPlayer> {
        let uuid = Uuid::parse_str(name).ok();
        if let Some(player) = self.get_players().into_iter().find(|player| {
            player.gameprofile.name.eq_ignore_ascii_case(name)
                || uuid.is_some_and(|uuid| player.gameprofile.id == uuid)
        }) {
            return Some(KnownPlayer::new(
                player.gameprofile.id,
                player.gameprofile.name.clone(),
            ));
        }

        let mut known = self.known_players.lock();
        if let Some(uuid) = uuid {
            return known.players.resolve_uuid(uuid);
        }
        let (profile, start_worker) = match known
            .players
            .resolve_name(name, chrono::Utc::now().timestamp_millis())
        {
            KnownPlayerNameLookup::Found(profile) => (Some(profile), false),
            KnownPlayerNameLookup::Missing => (None, false),
            KnownPlayerNameLookup::Expired => {
                let start_worker = known.mark_changed();
                (None, start_worker)
            }
        };
        drop(known);
        if start_worker {
            self.start_known_player_save_worker();
        }
        profile
    }

    fn start_known_player_save_worker(self: &Arc<Self>) {
        let server = Arc::clone(self);
        tokio::spawn(async move {
            server.run_known_player_save_worker().await;
        });
    }

    async fn run_known_player_save_worker(self: &Arc<Self>) {
        loop {
            let (players, generation) = self.known_players.lock().snapshot();
            let result = self
                .player_data_storage
                .save_known_players_if_current(&players, || {
                    self.known_players.lock().is_current(generation)
                })
                .await;
            match result {
                Ok(true | false) => {}
                Err(error) => {
                    tracing::error!(%error, "failed to save known player cache");
                }
            }
            let step = self.known_players.lock().finish_save(generation);
            if step == KnownPlayerSaveStep::SaveAgain {
                continue;
            }
            self.known_player_save_idle.notify_one();
            return;
        }
    }

    /// Waits for the coalesced identity-cache writer and persists the final snapshot.
    ///
    /// Later identity observations are ignored because the server is shutting down.
    ///
    /// # Errors
    ///
    /// Returns an error when the final rebuildable cache snapshot cannot be persisted.
    pub async fn flush_known_players(&self) -> io::Result<()> {
        let players = loop {
            let idle = self.known_player_save_idle.notified();
            let snapshot = self.known_players.lock().close_if_idle();
            if let Some(players) = snapshot {
                break players;
            }
            idle.await;
        };
        self.player_data_storage
            .save_known_players_if_current(&players, || true)
            .await
            .map(|_| ())
    }
}
