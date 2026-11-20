use crate::entity::prelude::*;
use steel_protocol::packets::game::CommonPlayerSpawnInfo;

use super::{
    Arc, BlockBreakingManager, CContainerClose, CGameEvent, CRespawn, CSetDefaultSpawnPosition,
    CSetHeldSlot, CSetPassengers, DVec3, Entity, GameEventType, MenuRemovalStatus,
    MobEffectSyncChange, MobEffectSyncPacket, Player, RegistryEntry, RelativeMovement, ResetReason,
    World,
};

impl Player {
    /// Resets the transient state of a player about to join its world for the
    /// first time.
    pub(crate) fn reset_for_initial_join(self: &Arc<Self>) {
        self.get_world().chunk_map.remove_player(self);
        self.set_client_loaded(false);
        self.reset_transient_state();
    }

    fn reset_transient_state(&self) {
        self.set_velocity(DVec3::ZERO);
        self.movement.lock().reset_last_known_client_movement();
        self.set_on_ground(false);
        self.set_shared_swimming(false);
        self.set_shared_shift_key_down(false);
        self.living().clear_sleeping_pos();
        self.set_fall_flying(false);
        self.living().set_sprinting(false);
        *self.block_breaking.lock() = BlockBreakingManager::new();
    }

    pub(crate) fn move_to_world(self: &Arc<Self>, world: &Arc<World>) -> bool {
        let Some(server) = self.server.upgrade() else {
            return false;
        };

        if !server.owns_player_for_world_change(self, false) {
            return false;
        }

        self.close_menus_for_world_change();
        self.send_respawn_packet(world, ResetReason::WorldChange);
        self.get_world().remove_player_for_world_change(self);

        if self
            .living_base
            .recheck_combat_status(self.tick_count(), false)
        {
            self.on_leave_combat();
        }

        self.set_world(Arc::clone(world));

        true
    }

    pub(crate) fn enter_domain_world(
        self: &Arc<Self>,
        world: &Arc<World>,
        restore: impl FnOnce(&Arc<Self>),
    ) -> bool {
        let Some(server) = self.server.upgrade() else {
            return false;
        };

        if !server.owns_player_for_world_change(self, true) {
            return false;
        }

        self.close_menus_for_world_change();
        let combat_ended = self
            .living_base
            .recheck_combat_status(self.tick_count(), false);
        self.set_world(Arc::clone(world));
        self.reset_transient_state();
        restore(self);

        self.base.damage_history().clear();
        self.send_respawn_packet(world, ResetReason::WorldChange);
        if combat_ended {
            self.on_leave_combat();
        }

        true
    }

    fn close_menus_for_world_change(self: &Arc<Self>) {
        assert_eq!(
            self.remove_all_menus(),
            MenuRemovalStatus::Complete,
            "player world change menu removal must run outside a menu callback"
        );
        self.send_packet(CContainerClose { container_id: 0 });
    }

    /// Prepares a newly allocated player for a death or End-credits respawn.
    pub(crate) fn prepare_respawn_replacement(&self, reason: ResetReason) {
        debug_assert!(matches!(
            reason,
            ResetReason::Respawn | ResetReason::EndCredits
        ));
        self.set_client_loaded(false);
        self.send_respawn_packet(&self.get_world(), reason);
    }

    fn send_respawn_packet(&self, world: &World, reason: ResetReason) {
        self.send_packet(CRespawn {
            common_player_spawn_info: CommonPlayerSpawnInfo {
                dimension_type: world.dimension_type.id() as i32,
                dimension: world.key.clone(),
                seed: world.obfuscated_seed(),
                game_type: self.game_mode(),
                previous_game_type: self.previous_game_mode(),
                is_debug: false,
                is_flat: world.is_flat,
                last_death_location: None,
                portal_cooldown: self.portal_cooldown(),
                sea_level: world.sea_level,
            },
            data_kept: reason.respawn_data_kept(),
        });
    }

    /// Spawns the player into their current world at the given position.
    ///
    /// This is the shared "enter world" path used by initial join, respawn, and
    /// world change. Sends position sync, abilities, inventory, time, weather,
    /// and adds the player to the world as appropriate for the given reason.
    ///
    /// # Panics
    /// Panics if the `advance_time` gamerule is not a bool.
    #[must_use]
    pub(crate) fn spawn(
        self: &Arc<Self>,
        position: DVec3,
        rotation: (f32, f32),
        reason: ResetReason,
    ) -> bool {
        self.spawn_with_velocity(position, rotation, DVec3::ZERO, reason)
    }

    #[must_use]
    pub(crate) fn spawn_with_velocity(
        self: &Arc<Self>,
        position: DVec3,
        rotation: (f32, f32),
        velocity: DVec3,
        reason: ResetReason,
    ) -> bool {
        self.spawn_with_velocity_packet(
            position,
            rotation,
            velocity,
            reason,
            position,
            rotation,
            velocity,
            RelativeMovement::NONE,
        )
    }

    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "packet-relative teleports must keep resolved and protocol values separate"
    )]
    pub(crate) fn spawn_with_velocity_packet(
        self: &Arc<Self>,
        position: DVec3,
        rotation: (f32, f32),
        velocity: DVec3,
        reason: ResetReason,
        packet_position: DVec3,
        packet_rotation: (f32, f32),
        packet_velocity: DVec3,
        relatives: RelativeMovement,
    ) -> bool {
        let world = self.get_world();
        self.synchronize_spawn_position(
            position,
            velocity,
            rotation,
            packet_position,
            packet_velocity,
            packet_rotation,
            relatives,
        );

        if reason == ResetReason::WorldChange {
            if !world.add_player(Arc::clone(self), reason) {
                return false;
            }
            self.stop_using_item();
            log::info!(
                "Player {} changed world to {}",
                self.gameprofile.name(),
                world.key
            );
        }

        self.synchronize_spawn_state(&world, true);

        match reason {
            ResetReason::WorldChange => true,
            ResetReason::InitialJoin => world.add_player(Arc::clone(self), reason),
            ResetReason::Respawn | ResetReason::EndCredits => {
                if world.players.get_by_entity_id(self.id()).is_none() {
                    return world.add_respawned_player(Arc::clone(self));
                }

                // Same world — re-enter chunk tracking
                world.chunk_map.remove_player(self);
                world.player_area_map.remove_by_entity_id(self.id());
                world.entity_tracker().on_player_leave(self);

                self.send_packet(CGameEvent {
                    event: GameEventType::LevelChunksLoadStart,
                    data: 0.0,
                });
                world.register_respawned_player_entity(self);
                true
            }
        }
    }

    /// Sends the spawn synchronization for a fresh respawn replacement without
    /// inserting it into world indexes. The replacement transaction owns that step.
    pub(crate) fn synchronize_respawn_replacement(
        self: &Arc<Self>,
        position: DVec3,
        rotation: (f32, f32),
    ) {
        self.synchronize_spawn_position(
            position,
            DVec3::ZERO,
            rotation,
            position,
            DVec3::ZERO,
            rotation,
            RelativeMovement::NONE,
        );

        self.synchronize_spawn_state(&self.get_world(), false);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "packet-relative teleports must keep resolved and protocol values separate"
    )]
    fn synchronize_spawn_position(
        &self,
        position: DVec3,
        velocity: DVec3,
        rotation: (f32, f32),
        packet_position: DVec3,
        packet_velocity: DVec3,
        packet_rotation: (f32, f32),
        relatives: RelativeMovement,
    ) {
        if let Err(error) = self.teleport_with_velocity_packet(
            position,
            velocity,
            rotation,
            packet_position,
            packet_velocity,
            packet_rotation,
            relatives,
        ) {
            panic!(
                "failed to synchronize player {} spawn position: {error}",
                self.id()
            );
        }
        self.movement.lock().reset_for_position_sync(position);
        self.reset_flying_ticks();
    }

    fn synchronize_spawn_state(self: &Arc<Self>, world: &World, resend_player_context: bool) {
        self.send_spawn_state_packets(world);
        self.reset_sent_info();
        if resend_player_context {
            self.server().resend_player_context(self);
        }
        self.send_active_effects_for_self();
    }

    fn send_spawn_state_packets(&self, world: &World) {
        self.send_abilities();
        self.send_packet(CSetHeldSlot {
            slot: i32::from(self.inventory.lock().get_selected_slot()),
        });
        self.send_time_sync(world);
        self.send_packet(world.initialize_border_packet());
        self.send_default_spawn_position(world);
        self.send_weather_sync(world);
    }

    fn send_time_sync(&self, world: &World) {
        self.send_packet(world.time_sync_packet());
    }

    fn send_default_spawn_position(&self, world: &World) {
        if let Some(server) = self.server.upgrade() {
            match server.respawn_data_for_domain(world.domain()) {
                Ok(respawn_data) => {
                    self.send_packet(CSetDefaultSpawnPosition {
                        global_pos: respawn_data.global_pos,
                        yaw: respawn_data.yaw,
                        pitch: respawn_data.pitch,
                    });
                }
                Err(error) => {
                    log::error!(
                        "Failed to send default spawn position to player {}: {error}",
                        self.gameprofile.name()
                    );
                }
            }
        }
    }

    fn send_weather_sync(&self, world: &World) {
        if !world.can_have_weather() || !world.is_raining() {
            return;
        }

        let (rain_level, thunder_level) = {
            let weather = world.weather.lock();
            (weather.rain_level, weather.thunder_level)
        };

        self.send_packet(CGameEvent {
            event: GameEventType::StartRaining,
            data: 0.0,
        });
        self.send_packet(CGameEvent {
            event: GameEventType::RainLevelChange,
            data: rain_level,
        });
        self.send_packet(CGameEvent {
            event: GameEventType::ThunderLevelChange,
            data: thunder_level,
        });
    }

    pub(in crate::player) fn passenger_ids_for_packet(entity: &dyn Entity) -> Vec<i32> {
        entity
            .passengers()
            .iter()
            .map(|passenger| passenger.id())
            .collect()
    }

    pub(in crate::player) fn send_mob_effect_sync_packet(&self, packet: MobEffectSyncPacket) {
        match packet {
            MobEffectSyncPacket::Update(packet) => self.send_packet(packet),
            MobEffectSyncPacket::Remove(packet) => self.send_packet(packet),
        }
    }

    fn send_active_effects_for_self(&self) {
        for effect in self.living_base.active_mob_effects() {
            self.send_mob_effect_sync_packet(
                MobEffectSyncChange::Update {
                    effect,
                    blend_for_self: false,
                }
                .packet(self.id(), true),
            );
        }
    }

    pub(in crate::player) fn send_active_effects_for_vehicle(&self, vehicle: &dyn Entity) {
        let Some(living_vehicle) = vehicle.as_living_entity() else {
            return;
        };
        for effect in living_vehicle.active_mob_effects() {
            self.send_mob_effect_sync_packet(
                MobEffectSyncChange::Update {
                    effect,
                    blend_for_self: false,
                }
                .packet(vehicle.id(), false),
            );
        }
    }

    pub(crate) fn send_restored_vehicle_mount_sync(&self, vehicle: &dyn Entity) {
        self.send_active_effects_for_vehicle(vehicle);
        self.send_packet(CSetPassengers::new(
            vehicle.id(),
            Self::passenger_ids_for_packet(vehicle),
        ));
    }

    pub(in crate::player) fn remove_active_effects_for_vehicle(&self, vehicle: &dyn Entity) {
        let Some(living_vehicle) = vehicle.as_living_entity() else {
            return;
        };
        for effect in living_vehicle.active_mob_effects() {
            self.send_mob_effect_sync_packet(
                MobEffectSyncChange::Remove {
                    effect: effect.effect(),
                }
                .packet(vehicle.id(), false),
            );
        }
    }
}
