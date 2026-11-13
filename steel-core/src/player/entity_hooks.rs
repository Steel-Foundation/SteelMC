//! `Player`'s overrides of the entity hooks.

use super::*;
use crate::entity::entity;
use crate::entity::prelude::*;

impl Player {
    pub(super) const ENTITY: EntityProps<Self> = EntityProps {
        // `ServerPlayer.tick` never calls `super`: the player runs the living
        // tick's pieces itself, around the client-driven movement.
        tick: |player| player.tick_player(),
        hurt: |player, world, source, amount| {
            let Some(amount) = player.hurt_amount_before_super(world, source, amount) else {
                return false;
            };
            living_entity::hurt(player, world, source, amount)
        },
        ride_tick: |player| player.ride_tick_player(),
        remove_vehicle: |player| {
            let old_vehicle = player.vehicle();
            entity::remove_vehicle(player);
            player.after_remove_vehicle(old_vehicle);
        },
        check_fall_damage: |player, vertical_movement, on_ground, on_state, pos, world| {
            player.check_fall_damage_before_super(on_ground, on_state, pos, world);
            living_entity::check_fall_damage(
                player,
                vertical_movement,
                on_ground,
                on_state,
                pos,
                world,
            );
        },
        // `Player.causeFallDamage` before `super`: players who may fly take no
        // fall damage; others count the distance fallen.
        cause_fall_damage: |player, fall_distance, damage_modifier, source| {
            if player.abilities.lock().may_fly {
                return false;
            }
            if fall_distance >= 2.0 {
                player.award_custom_stat_with_count(
                    &vanilla_custom_stats::FALL_ONE_CM,
                    (fall_distance * 100.0).round() as i32,
                );
            }
            living_entity::cause_fall_damage(player, fall_distance, damage_modifier, source)
        },
        // `Player.getBlockSpeedFactor`: flying and gliding players keep full speed.
        block_speed_factor: |player| {
            if player.abilities.lock().flying || player.living().is_fall_flying() {
                return 1.0;
            }
            living_entity::block_speed_factor(player)
        },
        // `Player.canFreeze`: spectators never freeze.
        can_freeze: |player| !player.is_spectator() && living_entity::can_freeze(player),
        // `Player.doWaterSplashEffect`: spectators make no splash.
        do_water_splash_effect: |player| {
            if !player.is_spectator() {
                entity::do_water_splash_effect(player);
            }
        },
        // `Player.updateSwimming`: flying players never swim.
        update_swimming: |player| {
            if player.is_flying() {
                player.set_shared_swimming(false);
            } else {
                entity::update_swimming(player);
            }
        },
        // `Player.makeStuckInBlock`: flying players do not get stuck.
        // TODO: Reset current impulse context once vehicle/player impulse contexts exist.
        make_stuck_in_block: |player, state, speed_multiplier| {
            if !player.is_flying() {
                entity::make_stuck_in_block(player, state, speed_multiplier);
            }
        },
        dimension_changing_delay: |_| 10,
        on_explosion_hit: |player, explosion_source| {
            let from_wind_charge = explosion_source
                .is_some_and(|source| source.entity_type() == &vanilla_entities::WIND_CHARGE);
            player
                .living()
                .set_ignore_fall_damage_from_current_impulse(from_wind_charge, player.position());
        },
        swim_splash_sound: |_| &sound_events::ENTITY_PLAYER_SPLASH,
        swim_high_speed_splash_sound: |_| &sound_events::ENTITY_PLAYER_SPLASH_HIGH_SPEED,
        scoreboard_name: |entity| entity.gameprofile.name().to_owned(),
        name: |entity| TextComponent::plain(entity.gameprofile.name().to_owned()),
        display_name: Self::entity_display_name,
        plain_text_name: |entity| entity.gameprofile.name().to_owned(),
        look_at: Self::entity_look_at,
        look_at_entity: Self::entity_look_at_entity,
        is_always_ticking: |_| true,
        teleport_to: Self::entity_teleport_to,
        start_riding: Self::entity_start_riding,
        broadcast_to_player: Self::entity_broadcast_to_player,
        fall_sounds: |_| {
            (
                &sound_events::ENTITY_PLAYER_SMALL_FALL,
                &sound_events::ENTITY_PLAYER_BIG_FALL,
            )
        },
        blocks_building: |_| true,
        is_pickable: |entity| !entity.is_spectator() && !entity.is_removed(),
        is_pushable: Self::entity_is_pushable,
        on_climbable: Self::on_climbable,
        is_spectator: |entity| entity.game_mode() == GameType::Spectator,
        is_flying_player: |entity| entity.is_flying(),
        fire_immune_ticks: |_| 20,
        remaining_fire_ticks_cap: |entity| entity.abilities.lock().invulnerable.then_some(1),
        fire_ignite_extra_ticks: |_| rand::random_range(1..=2),
        can_be_hit_by_projectile: Self::entity_can_be_hit_by_projectile,
        uses_client_movement_packets: |_| true,
        can_simulate_movement: |_| true,
        is_effective_ai: |_| true,
        known_movement: Self::entity_known_movement,
        known_speed: Self::entity_known_speed,
        is_suppressing_bounce: |entity| entity.is_crouching(),
        backs_off_from_edge: |entity| entity.is_crouching() && !entity.is_flying(),
        is_pushed_by_fluid: |entity| !entity.is_flying(),
        is_crouching: Self::is_crouching,
        may_interact: |entity, world, pos| world.may_interact(entity, pos),
        is_swimming: Self::is_swimming,
        sound_source: |_| SoundSource::Players,
        play_sound: Self::entity_play_sound,
        swim_sound: |_| &sound_events::ENTITY_PLAYER_SWIM,
        play_step_sound: Self::entity_play_step_sound,
        movement_emission: Self::entity_movement_emission,
        killed_entity: Self::entity_killed_entity,
        award_kill_score: Self::entity_award_kill_score,
        ..living_entity::entity_props()
    };

    /// `ServerPlayer.checkFallDamage` before `super`: the mace smash burst on
    /// landing.
    fn check_fall_damage_before_super(
        &self,
        on_ground: bool,
        on_state: BlockStateId,
        pos: BlockPos,
        world: &Arc<World>,
    ) {
        let fall_distance = self.fall_distance();
        if on_ground && fall_distance > 0.0 && self.spawn_extra_particles_on_fall() {
            let count = (50.0 * fall_distance).clamp(0.0, 200.0) as i32;
            let spread = f64::from(0.3_f32);
            world.send_particles(
                ParticleData::new(
                    &vanilla_particle_types::BLOCK,
                    BlockParticleOption::new(on_state),
                ),
                DVec3::new(
                    f64::from(pos.x()) + 0.5,
                    f64::from(pos.y()) + 1.0,
                    f64::from(pos.z()) + 0.5,
                ),
                count,
                DVec3::splat(spread),
                f64::from(0.15_f32),
            );
            self.set_spawn_extra_particles_on_fall(false);
        }
    }

    /// Whether the next landing sends the mace smash burst.
    #[must_use]
    pub fn spawn_extra_particles_on_fall(&self) -> bool {
        self.spawn_extra_particles_on_fall.load(Ordering::Relaxed)
    }

    /// Sets whether the next landing sends the mace smash burst.
    pub fn set_spawn_extra_particles_on_fall(&self, spawn: bool) {
        self.spawn_extra_particles_on_fall
            .store(spawn, Ordering::Relaxed);
    }

    fn entity_display_name(&self) -> TextComponent {
        self.name()
            .click_event(ClickEvent::suggest_command(format!(
                "/tell {} ",
                self.gameprofile.name()
            )))
            .hover_event(HoverEvent::show_entity(
                "minecraft:player",
                self.uuid(),
                Some(self.name()),
            ))
            .insertion(self.gameprofile.name().to_owned())
    }

    fn entity_look_at(&self, from_anchor: EntityAnchor, target: DVec3) {
        apply_entity_look_at(self, from_anchor, target);
        self.send_packet(CPlayerLookAt::position(
            protocol_look_at_anchor(from_anchor),
            target,
        ));
    }

    fn entity_look_at_entity(
        &self,
        from_anchor: EntityAnchor,
        target: &dyn Entity,
        target_anchor: EntityAnchor,
    ) {
        let target_position = target_anchor.position(target);
        apply_entity_look_at(self, from_anchor, target_position);
        self.send_packet(CPlayerLookAt::entity(
            protocol_look_at_anchor(from_anchor),
            target_position,
            target.id(),
            protocol_look_at_anchor(target_anchor),
        ));
    }

    /// `ServerPlayer.rideTick` around `super`: a player wanting off dismounts
    /// instead, and riding statistics count the distance.
    pub(super) fn ride_tick_player(&self) {
        let pre = self.position();
        if self.wants_to_stop_riding() && self.is_passenger() {
            self.stop_riding();
        } else {
            living_entity::ride_tick(self);
        }
        self.check_riding_statistics(self.position() - pre);
    }

    /// `Player.removeVehicle` and `ServerPlayer.removeVehicle`, after
    /// `Entity`'s: clears the boarding cooldown, the vehicle's effects and
    /// tells the client the vehicle lost its passenger.
    pub(super) fn after_remove_vehicle(&self, old_vehicle: Option<SharedEntity>) {
        self.base.set_boarding_cooldown(0);
        let Some(old_vehicle) = old_vehicle else {
            return;
        };

        self.remove_active_effects_for_vehicle(old_vehicle.as_ref());
        self.send_packet(CSetPassengers::new(
            old_vehicle.id(),
            Self::passenger_ids_for_packet(old_vehicle.as_ref()),
        ));
    }

    fn entity_teleport_to(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        let (yaw, pitch) = self.rotation();
        self.teleport(pos, yaw, pitch)
    }

    fn entity_start_riding(&self, entity_to_ride: &SharedEntity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(passenger) = world.get_entity_by_id(self.id()) else {
            return false;
        };
        if !start_riding_entities(&passenger, entity_to_ride) {
            return false;
        }

        entity_to_ride.position_rider(self.as_entity_event_source());
        let position = self.position();
        let (yaw, pitch) = self.rotation();
        if let Err(error) = self.teleport(position, yaw, pitch) {
            panic!(
                "failed to synchronize player {} mounted position: {error}",
                self.id()
            );
        }
        self.send_active_effects_for_vehicle(entity_to_ride.as_ref());
        self.send_packet(CSetPassengers::new(
            entity_to_ride.id(),
            Self::passenger_ids_for_packet(entity_to_ride.as_ref()),
        ));
        true
    }

    fn entity_broadcast_to_player(&self, player: &Player) -> bool {
        if player.is_spectator() {
            true
        } else {
            !self.is_spectator()
        }
    }

    fn entity_is_pushable(&self) -> bool {
        self.living().health() > 0.0 && !self.is_spectator() && !self.on_climbable()
    }

    fn entity_can_be_hit_by_projectile(&self) -> bool {
        self.living().health() > 0.0 && self.is_pickable()
    }

    fn entity_known_movement(&self) -> DVec3 {
        if let Some(vehicle) = self.vehicle()
            && vehicle
                .controlling_passenger()
                .is_none_or(|controller| controller.id() != self.id())
        {
            return vehicle.known_movement();
        }

        self.movement.lock().last_known_client_movement()
    }

    fn entity_known_speed(&self) -> DVec3 {
        if let Some(vehicle) = self.vehicle()
            && vehicle
                .controlling_passenger()
                .is_none_or(|controller| controller.id() != self.id())
        {
            return vehicle.known_speed();
        }

        self.movement.lock().last_known_client_movement()
    }

    /// Matches vanilla `Player.playSound`, which excludes the source player.
    fn entity_play_sound(&self, sound: SoundEventRef, volume: f32, pitch: f32) {
        if let Some(world) = self.level() {
            world.play_sound_at(
                sound,
                self.sound_source(),
                self.position(),
                volume,
                pitch,
                Some(self.id()),
            );
        }
    }

    fn entity_play_step_sound(&self, on_pos: BlockPos, on_state: BlockStateId) {
        if self.is_in_water() {
            self.water_swim_sound();
            self.play_muffled_step_sound(on_state);
            return;
        }

        let primary_step_sound_pos = self.primary_step_sound_block_pos(on_pos);
        if primary_step_sound_pos == on_pos {
            self.play_block_step_sound(on_state);
        } else {
            let primary_state = self.get_world().get_block_state(primary_step_sound_pos);
            if primary_state
                .get_block()
                .has_tag(&BlockTag::COMBINATION_STEP_SOUND_BLOCKS)
            {
                self.play_combination_step_sounds(primary_state, on_state);
            } else {
                self.play_block_step_sound(primary_state);
            }
        }
    }

    fn entity_movement_emission(&self) -> EntityMovementEmission {
        if self.is_flying() || self.on_ground() && self.is_discrete() {
            EntityMovementEmission::None
        } else {
            EntityMovementEmission::All
        }
    }

    fn entity_killed_entity(
        &self,
        _world: &World,
        entity: &Living<'_>,
        _source: &DamageSource,
    ) -> bool {
        log::debug!("1");
        self.award_stat(&vanilla_stat_types::ENTITY_KILLED, entity.entity_type());
        true
    }

    fn entity_award_kill_score(&self, victim: &dyn Entity, _killing_blow: &DamageSource) {
        if self.id() != victim.id() {
            // TODO: Trigger advancement criteria.
            // TODO: Increment the score of some objectives of this player.
            self.award_custom_stat(if victim.as_player().is_some() {
                &vanilla_custom_stats::PLAYER_KILLS
            } else {
                &vanilla_custom_stats::MOB_KILLS
            });
            // TODO: Handle team kill
        }
    }
}

const fn protocol_look_at_anchor(anchor: EntityAnchor) -> LookAtAnchor {
    match anchor {
        EntityAnchor::Feet => LookAtAnchor::Feet,
        EntityAnchor::Eyes => LookAtAnchor::Eyes,
    }
}
