//! Admission of ephemeral server-controlled players into actual loaded worlds.
use super::Server;
use crate::{
    entity::next_entity_id,
    player::{
        ClientInformation, Player, PlayerConnection, PlayerSession, ResetReason,
        connection::ServerControlledConnection,
    },
};
use glam::DVec3;
use std::sync::Arc;
use steel_registry::game_profile::GameProfile;
use steel_utils::{ChunkPos, Identifier};
use uuid::Uuid;
impl Server {
    /// Spawns an ephemeral player bot at a prepared position in a temporary domain.
    /// Bots do not consume client connection slots. Call from a server tick callback
    /// or job, outside world worker execution.
    pub fn spawn_bot(
        self: &Arc<Self>,
        key: &Identifier,
        name: &str,
        position: DVec3,
    ) -> Result<Arc<Player>, String> {
        let world = self.worlds.get(key).ok_or("bot world is not loaded")?;
        if !self.player_data_storage.is_temporary_domain(world.domain())
            || self.temporary_worlds.is_closing(world.domain())
        {
            return Err("bots require an active temporary domain".into());
        }
        if !position.is_finite()
            || !world.is_in_valid_bounds(steel_utils::BlockPos::new(
                position.x.floor() as i32,
                position.y.floor() as i32,
                position.z.floor() as i32,
            ))
        {
            return Err("invalid bot position".into());
        }
        let chunk = ChunkPos::new(
            (position.x.floor() as i32) >> 4,
            (position.z.floor() as i32) >> 4,
        );
        if world.chunk_map.with_full_chunk(chunk, |_| ()).is_none() {
            return Err("bot spawn chunk is not loaded".into());
        }
        if self
            .get_players()
            .iter()
            .any(|player| player.gameprofile.name() == name)
        {
            return Err("player name is already in use".into());
        }
        let profile = GameProfile::new(Uuid::new_v4(), name.to_owned(), Vec::new())
            .map_err(|e| e.to_string())?;
        let uuid = profile.id();
        let reservation = self
            .try_reserve_player_join(uuid)
            .ok_or("bot UUID admission failed")?;
        let player = Arc::new(Player::new(
            profile,
            Arc::new(PlayerConnection::Other(Box::new(
                ServerControlledConnection::default(),
            ))),
            Arc::new(PlayerSession::new(
                self.config.chat_spam_threshold_seconds,
                self.config.command_spam_threshold_seconds,
            )),
            Arc::clone(&world),
            Arc::downgrade(self),
            Arc::clone(&self.config),
            next_entity_id(),
            ClientInformation::default(),
        ));
        if !player.session.bind_initial_player(&player) {
            return Err("bot session admission failed".into());
        }
        self.admit_reserved_player(Arc::clone(&player))
            .map_err(|e| format!("bot admission failed: {e:?}"))?;
        drop(reservation);
        self.apply_cached_or_default_permission_state(&player);
        player.reset(world, ResetReason::InitialJoin);
        self.sync_tab_list(&player);
        if !player.spawn(position, (0.0, 0.0), ResetReason::InitialJoin) {
            self.queue_player_disconnect(Arc::clone(&player));
            return Err("bot world admission failed".into());
        }
        let _ = player.mark_joined_world();
        player.set_client_loaded(true);
        Ok(player)
    }
    /// Uses the normal disconnect lifecycle to remove an owned bot and its tab entry.
    pub fn despawn_bot(&self, player: &Arc<Player>) -> Result<(), String> {
        if !player.is_server_controlled() {
            return Err("player is not server controlled".into());
        }
        player.disconnect("Bot removed");
        self.queue_player_disconnect(Arc::clone(player));
        Ok(())
    }
}
