use super::{
    Arc, CLogin, CSetDefaultSpawnPosition, CommonPlayerSpawnInfo, DVec3, DomainPlayerData,
    DomainPlayerState, EnderPearlRestoreJob, Entity, IMMEDIATE_RESPAWN, Identifier,
    LIMITED_CRAFTING, PersistentEnderPearl, PersistentRootVehicle, Player, PreparedSpawn,
    REDUCED_DEBUG_INFO, RegistryEntry, RespawnData, RootVehicleRestoreJob, Server, Uuid, World,
    apply_default_spawn, local_respawn_data_for_world,
};

impl Server {
    pub(super) async fn load_join_domain(&self, player: &Player) -> Result<String, String> {
        match self
            .player_data_storage
            .load_global(player.gameprofile.id)
            .await
        {
            Ok(Some(global)) if self.worlds.has_domain(&global.last_active_domain) => {
                Ok(global.last_active_domain)
            }
            Ok(Some(global)) => {
                log::warn!(
                    "Player {} last active domain {} no longer exists, using default domain",
                    player.gameprofile.name,
                    global.last_active_domain
                );
                Ok(self.worlds.default_domain().to_owned())
            }
            Ok(None) => Ok(self.worlds.default_domain().to_owned()),
            Err(e) => Err(format!("failed to load global player data: {e}")),
        }
    }

    pub(super) async fn load_domain_player_state(
        &self,
        player: &Player,
        target_domain: &str,
        fallback_world: Option<Arc<World>>,
        restore_saved_location: bool,
    ) -> Result<DomainPlayerState, String> {
        let explicit_target_world = fallback_world.is_some();
        let mut world = self
            .worlds
            .default_world(target_domain)
            .cloned()
            .ok_or_else(|| format!("domain {target_domain} has no default world"))?;
        if let Some(fallback_world) = fallback_world {
            world = fallback_world;
        }

        match self
            .player_data_storage
            .load_domain(target_domain, player.gameprofile.id)
            .await
        {
            Ok(Some(saved_data)) => {
                let restore_location = restore_saved_location
                    && self.resolve_saved_world(
                        &saved_data.world,
                        target_domain,
                        &mut world,
                        &player.gameprofile.name,
                    );
                let (data, spawn_position) = if restore_location {
                    let spawn_position =
                        DVec3::new(saved_data.pos[0], saved_data.pos[1], saved_data.pos[2]);
                    (
                        DomainPlayerData::SavedRestored {
                            data: Box::new(saved_data),
                        },
                        spawn_position,
                    )
                } else {
                    let (default_world, default_spawn) = self
                        .prepare_domain_default_spawn(target_domain, explicit_target_world, &world)
                        .await?;
                    world = default_world;
                    (
                        DomainPlayerData::SavedWithoutLocation {
                            data: Box::new(saved_data),
                            default_spawn,
                        },
                        default_spawn.position,
                    )
                };
                let spawn_chunk_request = world.prepare_player_spawn_chunks(spawn_position).await?;
                log::info!("Loaded saved data for player {}", player.gameprofile.name);
                Ok(DomainPlayerState {
                    world,
                    data,
                    _spawn_chunk_request: spawn_chunk_request,
                })
            }
            Ok(None) => {
                log::debug!(
                    "No saved data for player {} in domain {}, using defaults",
                    player.gameprofile.name,
                    target_domain
                );
                let (default_world, default_spawn) = self
                    .prepare_domain_default_spawn(target_domain, explicit_target_world, &world)
                    .await?;
                world = default_world;
                let spawn_chunk_request = world
                    .prepare_player_spawn_chunks(default_spawn.position)
                    .await?;
                Ok(DomainPlayerState {
                    world,
                    data: DomainPlayerData::FirstVisit { default_spawn },
                    _spawn_chunk_request: spawn_chunk_request,
                })
            }
            Err(e) => Err(format!(
                "failed to load domain player data for {} in domain {}: {e}",
                player.gameprofile.name, target_domain
            )),
        }
    }

    async fn prepare_domain_default_spawn(
        &self,
        target_domain: &str,
        explicit_target_world: bool,
        world: &Arc<World>,
    ) -> Result<(Arc<World>, PreparedSpawn), String> {
        if explicit_target_world {
            return Ok((world.clone(), Self::prepare_default_spawn(world).await?));
        }

        let (world, respawn_data) = self.respawn_world_and_data_for_domain(target_domain)?;
        let spawn = Self::prepare_respawn_spawn(&world, &respawn_data).await?;
        Ok((world, spawn))
    }

    async fn prepare_default_spawn(world: &Arc<World>) -> Result<PreparedSpawn, String> {
        let (spawn, spawn_pos) = {
            let level_data = world.level_data.read();
            (
                level_data.data().spawn.clone(),
                level_data.data().spawn_pos(),
            )
        };
        let position = world
            .find_adjusted_shared_spawn_pos(spawn_pos, world.default_gamemode)
            .await?;
        Ok(PreparedSpawn {
            position,
            rotation: (spawn.angle, 0.0),
        })
    }

    async fn prepare_respawn_spawn(
        world: &Arc<World>,
        respawn_data: &RespawnData,
    ) -> Result<PreparedSpawn, String> {
        let position = world
            .find_adjusted_shared_spawn_pos(respawn_data.pos(), world.default_gamemode)
            .await?;
        Ok(PreparedSpawn {
            position,
            rotation: (respawn_data.yaw, respawn_data.pitch),
        })
    }

    fn resolve_saved_world(
        &self,
        saved_world: &str,
        target_domain: &str,
        world: &mut Arc<World>,
        player_name: &str,
    ) -> bool {
        let Ok(saved_world_key) = saved_world.parse::<Identifier>() else {
            log::warn!(
                "Saved world {saved_world} for player {player_name} is invalid, using domain default spawn"
            );
            return false;
        };
        if saved_world_key.namespace.as_ref() != target_domain {
            log::warn!(
                "Saved world {saved_world_key} for player {player_name} is outside target domain {target_domain}, using domain default spawn"
            );
            return false;
        }
        let Some(saved_world) = self.worlds.get(&saved_world_key) else {
            log::warn!(
                "Saved world {saved_world_key} for player {player_name} is missing, using domain default spawn"
            );
            return false;
        };
        *world = saved_world.clone();
        true
    }

    pub(super) fn apply_domain_player_state(player: &Arc<Player>, state: &DomainPlayerState) {
        match &state.data {
            DomainPlayerData::SavedRestored { data } => {
                data.apply_to_player(player);
            }
            DomainPlayerData::SavedWithoutLocation {
                data,
                default_spawn,
            } => {
                apply_default_spawn(player, &state.world, *default_spawn);
                data.apply_to_player_without_location(player);
            }
            DomainPlayerData::FirstVisit { default_spawn } => {
                apply_default_spawn(player, &state.world, *default_spawn);
            }
        }
    }

    pub(super) fn schedule_root_vehicle_restore(
        &self,
        player: &Arc<Player>,
        state: &DomainPlayerState,
    ) {
        let Some(root_vehicle) = Self::root_vehicle_to_restore(state) else {
            player.clear_pending_root_vehicle();
            return;
        };
        player.set_pending_root_vehicle(&state.world, root_vehicle.clone());
        let Some(job) =
            RootVehicleRestoreJob::new(Arc::clone(player), Arc::clone(&state.world), &root_vehicle)
        else {
            player.clear_pending_root_vehicle();
            return;
        };
        self.jobs.spawn(job);
    }

    fn root_vehicle_to_restore(state: &DomainPlayerState) -> Option<PersistentRootVehicle> {
        match &state.data {
            DomainPlayerData::SavedRestored { data } => data.root_vehicle.clone(),
            DomainPlayerData::SavedWithoutLocation { .. } | DomainPlayerData::FirstVisit { .. } => {
                None
            }
        }
    }

    /// Spawns a restore job per persisted ender pearl, each in its own world
    /// (vanilla `ServerPlayer.loadAndSpawnEnderPearls`).
    pub(super) fn schedule_ender_pearl_restores(
        &self,
        player: &Arc<Player>,
        state: &DomainPlayerState,
    ) {
        let pearls = Self::ender_pearls_to_restore(state);
        if pearls.is_empty() {
            player.clear_pending_ender_pearls();
            return;
        }
        player.set_pending_ender_pearls(pearls.clone());
        for pearl in pearls {
            let pearl_uuid = Uuid::from_bytes(pearl.entity.uuid);
            let Some(world) = self.resolve_pearl_world(&pearl.world, player) else {
                player.remove_pending_ender_pearl(pearl_uuid);
                continue;
            };
            if let Some(job) = EnderPearlRestoreJob::new(Arc::clone(player), world, pearl.entity) {
                self.jobs.spawn(job);
            } else {
                player.remove_pending_ender_pearl(pearl_uuid);
            }
        }
    }

    fn ender_pearls_to_restore(state: &DomainPlayerState) -> Vec<PersistentEnderPearl> {
        match &state.data {
            DomainPlayerData::SavedRestored { data }
            | DomainPlayerData::SavedWithoutLocation { data, .. } => data.ender_pearls.clone(),
            DomainPlayerData::FirstVisit { .. } => Vec::new(),
        }
    }

    fn resolve_pearl_world(&self, world_key: &str, player: &Player) -> Option<Arc<World>> {
        let Ok(key) = world_key.parse::<Identifier>() else {
            log::warn!(
                "Saved ender pearl world {world_key} for player {} is invalid, skipping",
                player.gameprofile.name
            );
            return None;
        };
        let Some(world) = self.worlds.get(&key) else {
            log::warn!(
                "Saved ender pearl world {key} for player {} is missing, skipping",
                player.gameprofile.name
            );
            return None;
        };
        Some(world.clone())
    }

    pub(super) fn send_login_packet(&self, player: &Player, world: &World) {
        let reduced_debug_info = world.get_game_rule(&REDUCED_DEBUG_INFO);
        let immediate_respawn = world.get_game_rule(&IMMEDIATE_RESPAWN);
        let do_limited_crafting = world.get_game_rule(&LIMITED_CRAFTING);

        // Get world data
        let hashed_seed = world.obfuscated_seed();

        player.send_packet(CLogin {
            player_id: player.id(),
            hardcore: false,
            levels: self.worlds.keys().cloned().collect(),
            max_players: self.config.max_players as i32,
            chunk_radius: self.config.view_distance.into(),
            simulation_distance: self.config.simulation_distance.into(),
            reduced_debug_info,
            show_death_screen: !immediate_respawn,
            do_limited_crafting,
            common_player_spawn_info: CommonPlayerSpawnInfo {
                dimension_type: world.dimension_type.id() as i32,
                dimension: world.key.clone(),
                seed: hashed_seed,
                game_type: player.game_mode(),
                previous_game_type: player.previous_game_mode(),
                is_debug: false,
                is_flat: world.is_flat,
                last_death_location: None,
                portal_cooldown: 0,
                sea_level: world.sea_level,
            },
            online_mode: self.config.online_mode,
            enforces_secure_chat: self.config.enforce_secure_chat,
        });
    }

    /// Gets all the players on the server
    pub fn get_players(&self) -> Vec<Arc<Player>> {
        let mut players = vec![];
        self.online_players.iter_players(|_, p: &Arc<Player>| {
            players.push(p.clone());
            true
        });
        players
    }

    /// Returns the total number of players currently online across all worlds.
    #[must_use]
    pub fn player_count(&self) -> usize {
        self.online_players.len()
    }

    /// Returns a sample of up to 12 online players for the server list ping.
    #[must_use]
    pub fn player_sample(&self) -> Vec<(String, String)> {
        const MAX_SAMPLE: usize = 12;

        let players = self.get_players();
        if players.is_empty() {
            return vec![];
        }

        let sample_size = players.len().min(MAX_SAMPLE);
        // Random starting offset into the player list
        let offset = if players.len() > sample_size {
            (rand::random::<u64>() as usize) % (players.len() - sample_size + 1)
        } else {
            0
        };

        let mut sample: Vec<(String, String)> = players[offset..offset + sample_size]
            .iter()
            .map(|p| {
                (
                    p.gameprofile.name.clone(),
                    p.gameprofile.id.hyphenated().to_string(),
                )
            })
            .collect();

        // Shuffle using Fisher-Yates with random indices
        for i in (1..sample.len()).rev() {
            let j = (rand::random::<u64>() as usize) % (i + 1);
            sample.swap(i, j);
        }

        sample
    }

    /// Returns the server default world or if not exists the first world.
    /// # Panics
    /// if no world exists on this server crisis is there!
    pub fn overworld(&self) -> &Arc<World> {
        self.worlds.server_default_world().unwrap_or_else(|| {
            self.worlds
                .values()
                .next()
                .expect("At least one world must exist")
        })
    }

    /// Resolves the default respawn world and data for a domain.
    pub fn respawn_world_and_data_for_domain(
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

        let Some(target_world) = self
            .worlds
            .get(respawn_data.dimension())
            .filter(|world| world.domain() == domain)
            .cloned()
        else {
            let respawn_data = default_world
                .world_border_adjusted_respawn_data(local_respawn_data_for_world(&default_world));
            return Ok((default_world.clone(), respawn_data));
        };

        let respawn_data = target_world.world_border_adjusted_respawn_data(respawn_data);
        Ok((target_world, respawn_data))
    }

    /// Returns the default respawn data sent to clients in the given domain.
    pub fn respawn_data_for_domain(&self, domain: &str) -> Result<RespawnData, String> {
        self.respawn_world_and_data_for_domain(domain)
            .map(|(_, respawn_data)| respawn_data)
    }

    /// Sets the default respawn data for the respawn data's domain and broadcasts it.
    pub fn set_respawn_data(&self, respawn_data: RespawnData) -> Result<(), String> {
        let domain = respawn_data.dimension().namespace.as_ref();
        let default_world = self
            .worlds
            .default_world(domain)
            .cloned()
            .ok_or_else(|| format!("domain {domain} has no default world"))?;
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

        if Arc::ptr_eq(&default_world, &target_world) {
            let mut level_data = default_world.level_data.write();
            let data = level_data.data_mut();
            data.set_spawn_pos(respawn_data.pos());
            data.spawn.angle = respawn_data.yaw;
            data.set_respawn_data(respawn_data.clone());
        } else {
            default_world
                .level_data
                .write()
                .data_mut()
                .set_respawn_data(respawn_data.clone());

            let mut level_data = target_world.level_data.write();
            let data = level_data.data_mut();
            data.set_spawn_pos(respawn_data.pos());
            data.spawn.angle = respawn_data.yaw;
        }

        let packet = CSetDefaultSpawnPosition {
            global_pos: respawn_data.global_pos.clone(),
            yaw: respawn_data.yaw,
            pitch: respawn_data.pitch,
        };
        for world in self
            .worlds
            .values()
            .filter(|world| world.domain() == domain)
        {
            world.broadcast_to_all(packet.clone());
        }

        Ok(())
    }

    /// Returns the default domain's conventional nether world, if present.
    pub fn nether(&self) -> Option<&Arc<World>> {
        let key = Identifier::new(self.worlds.default_domain().to_owned(), "the_nether");
        self.worlds.get(&key)
    }

    /// Returns the default domain's conventional end world, if present.
    pub fn the_end(&self) -> Option<&Arc<World>> {
        let key = Identifier::new(self.worlds.default_domain().to_owned(), "the_end");
        self.worlds.get(&key)
    }
}
