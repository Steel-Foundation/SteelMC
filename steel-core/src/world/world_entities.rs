//! This module contains the implementation of the world's entity-related methods.
use std::sync::Arc;

use steel_protocol::packets::game::{CGameEvent, GameEventType};
use steel_registry::vanilla_entities;
use steel_utils::ChunkPos;

use crate::{
    entity::{
        Entity, EntityOwnership, NullEntityCallback, PlayerEntityCallback, RemovalReason,
        SharedEntity,
    },
    player::connection::NetworkConnection,
    player::player_data::PersistentPlayerData,
    player::{Player, ResetReason},
    world::World,
};

impl World {
    fn attach_player_entity_callback(self: &Arc<Self>, player: &Arc<Player>) {
        let callback = Arc::new(PlayerEntityCallback::new(player.id(), Arc::downgrade(self)));
        player.set_level_callback(callback);
    }

    fn register_player_entity(self: &Arc<Self>, player: &Arc<Player>) {
        self.attach_player_entity_callback(player);

        let entity: SharedEntity = player.clone();
        let lifecycle = match self
            .entity_manager()
            .add_live_entity(entity.clone(), EntityOwnership::External)
        {
            Ok(lifecycle) => lifecycle,
            Err(error) => panic!("failed to register player entity: {error}"),
        };
        self.apply_entity_lifecycle_changes(lifecycle);
    }

    fn unride_player_for_removal(&self, player: &Player, store_root_vehicle: bool) {
        for passenger in player.passengers() {
            passenger.stop_riding();
            self.mark_chunk_dirty(ChunkPos::from_entity_pos(passenger.position()));
        }

        if store_root_vehicle
            && let Some(root_vehicle) = player.root_vehicle()
            && root_vehicle.id() != player.id()
            && root_vehicle.has_exactly_one_player_passenger()
        {
            Self::remove_root_vehicle_tree_stored_with_player(root_vehicle);
            return;
        }

        if let Some(vehicle) = player.vehicle() {
            player.stop_riding();
            self.mark_chunk_dirty(ChunkPos::from_entity_pos(vehicle.position()));
        }
    }

    fn remove_root_vehicle_tree_stored_with_player(entity: SharedEntity) {
        let passengers = entity.passengers();
        entity.set_removed(RemovalReason::StoredWithPlayer);

        for passenger in passengers {
            if passenger.entity_type() == &vanilla_entities::PLAYER {
                continue;
            }
            Self::remove_root_vehicle_tree_stored_with_player(passenger);
        }
    }

    pub(crate) fn unregister_player_entity(&self, player: &Player) {
        let entity_id = player.id();
        self.remove_entity_from_tracker(entity_id);

        self.entity_manager()
            .remove_live_entity(entity_id, RemovalReason::ChangedWorld);
        player.set_level_callback(Arc::new(NullEntityCallback));
    }

    pub(crate) fn register_respawned_player_entity(self: &Arc<Self>, player: &Arc<Player>) {
        self.register_player_entity(player);
        self.chunk_map.update_player_status(player);
    }

    pub(crate) fn add_respawned_player(self: &Arc<Self>, player: Arc<Player>) -> bool {
        if !self.players.insert(player.clone()) {
            player.connection.close();
            return false;
        }

        self.register_respawned_player_entity(&player);
        player.send_packet(CGameEvent {
            event: GameEventType::LevelChunksLoadStart,
            data: 0.0,
        });
        true
    }

    /// Detaches a disconnecting player from live world state and snapshots it.
    ///
    /// Persistence happens asynchronously after the server's pre-tick phase completes.
    pub(crate) fn detach_player_for_disconnect(
        self: &Arc<Self>,
        player: Arc<Player>,
    ) -> Option<(Arc<Player>, String, PersistentPlayerData)> {
        let Some(player) = self.players.remove_player_sync(&player) else {
            if !player.has_won_game() {
                return None;
            }

            let domain = self.domain().to_owned();
            let player_data = PersistentPlayerData::from_player(&player);
            player.store_ender_pearls_with_player();
            return Some((player, domain, player_data));
        };
        let entity_id = player.id();
        let domain = self.domain().to_owned();
        let player_data = PersistentPlayerData::from_player(&player);

        self.unride_player_for_removal(&player, true);
        player.store_ender_pearls_with_player();
        self.unregister_player_entity(&player);

        // Remove player from entity tracking (stop tracking all entities for this player)
        self.entity_tracker().on_player_leave(entity_id);

        self.player_area_map.on_player_leave(&player);
        self.chunk_map.remove_player(&player);

        Some((player, domain, player_data))
    }

    /// Removes a player from the world during a world change.
    ///
    /// Unlike `remove_player`, this is synchronous and skips player data saving and tab list
    /// removal — the player stays in the global tab list since they are only switching worlds.
    pub(crate) fn remove_player_for_world_change(self: &Arc<Self>, player: &Arc<Player>) {
        let Some(player) = self.players.remove_player_sync(player) else {
            return;
        };
        let entity_id = player.id();

        self.unride_player_for_removal(&player, false);
        self.unregister_player_entity(&player);
        self.entity_tracker().on_player_leave(entity_id);
        self.player_area_map.on_player_leave(&player);
        // Note: no CRemovePlayerInfo — player stays in the global tab list
        self.chunk_map.remove_player(&player);
    }

    /// Removes a player during a domain switch after the caller has saved
    /// the player's current-domain data.
    pub(crate) fn remove_player_for_domain_switch(self: &Arc<Self>, player: &Arc<Player>) {
        let Some(player) = self.players.remove_player_sync(player) else {
            return;
        };
        let entity_id = player.id();

        self.unride_player_for_removal(&player, true);
        player.store_ender_pearls_with_player();
        self.unregister_player_entity(&player);
        self.entity_tracker().on_player_leave(entity_id);
        self.player_area_map.on_player_leave(&player);
        self.chunk_map.remove_player(&player);
    }

    /// Adds a player to the world.
    ///
    /// On `InitialJoin`, sends full tab list + entity spawn synchronization to/from all
    /// players. On `WorldChange`, this is skipped — the player already exists in all
    /// clients' tab lists and the entity tracker handles spawning as chunks load.
    #[must_use]
    pub(crate) fn add_player(self: &Arc<Self>, player: Arc<Player>, _reason: ResetReason) -> bool {
        if !self.players.insert(player.clone()) {
            player.connection.close();
            return false;
        }

        self.register_player_entity(&player);
        self.chunk_map.update_player_status(&player);

        player.send_packet(CGameEvent {
            event: GameEventType::LevelChunksLoadStart,
            data: 0.0,
        });

        player.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: player.game_mode().into(),
        });

        true
    }
}
