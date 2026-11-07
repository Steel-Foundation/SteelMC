//! `Player`'s overrides of the entity hooks.

use super::*;

impl Player {
    pub(super) const ENTITY: EntityProps<Self> = EntityProps {
        is_hard_collision_relevant: |_| false,
        dimension_changing_delay: |_| 10,
        check_fall_damage: Self::entity_check_fall_damage,
        on_explosion_hit: |player, explosion_source| {
            let from_wind_charge = explosion_source
                .is_some_and(|source| source.entity_type() == &vanilla_entities::WIND_CHARGE);
            player
                .living()
                .set_ignore_fall_damage_from_current_impulse(from_wind_charge, player.position());
        },
        block_speed_factor: |player| {
            if player.abilities.lock().flying || player.living().is_fall_flying() {
                1.0
            } else {
                player.living().living_block_speed_factor()
            }
        },
        do_water_splash_effect: Self::entity_do_water_splash_effect,
        swim_splash_sound: |_| &sound_events::ENTITY_PLAYER_SPLASH,
        swim_high_speed_splash_sound: |_| &sound_events::ENTITY_PLAYER_SPLASH_HIGH_SPEED,
        scoreboard_name: |entity| entity.gameprofile.name().to_owned(),
        name: |entity| TextComponent::plain(entity.gameprofile.name().to_owned()),
        display_name: Self::entity_display_name,
        plain_text_name: |entity| entity.gameprofile.name().to_owned(),
        look_at: Self::entity_look_at,
        look_at_entity: Self::entity_look_at_entity,
        is_always_ticking: |_| true,
        update_swimming: Self::entity_update_swimming,
        ride_tick: Self::entity_ride_tick,
        stop_riding: Self::entity_stop_riding,
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
        can_freeze: Self::entity_can_freeze,
        make_stuck_in_block: Self::entity_make_stuck_in_block,
        can_be_hit_by_projectile: Self::entity_can_be_hit_by_projectile,
        uses_client_movement_packets: |_| true,
        can_simulate_movement: |_| true,
        is_effective_ai: |_| true,
        known_movement: Self::entity_known_movement,
        known_speed: Self::entity_known_speed,
        is_suppressing_bounce: |entity| entity.is_crouching(),
        cause_fall_damage: Self::entity_cause_fall_damage,
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
        on_below_world: Self::entity_on_below_world,
        dimensions_for_pose: Self::entity_dimensions_for_pose,
        hurt: Self::entity_hurt,
        killed_entity: Self::entity_killed_entity,
        award_kill_score: Self::entity_award_kill_score,
        ..living_entity::entity_props()
    };

    fn entity_do_water_splash_effect(&self) {
        if !self.is_spectator() {
            self.do_water_splash_effect_default();
        }
    }

    /// `ServerPlayer.checkFallDamage`: the mace smash burst on landing, then the living step.
    fn entity_check_fall_damage(
        &self,
        entity: &SharedEntity,
        vertical_movement: f64,
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

        self.living().check_living_fall_damage(
            entity,
            vertical_movement,
            on_ground,
            on_state,
            pos,
            world,
        );
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

    fn entity_update_swimming(&self) {
        if self.is_flying() {
            self.set_shared_swimming(false);
        } else {
            self.update_swimming_default();
        }
    }

    fn entity_ride_tick(&self, entity: &SharedEntity) {
        let pre = self.position();
        if self.wants_to_stop_riding() && self.is_passenger() {
            self.stop_riding();
        } else {
            Arc::clone(entity).ride_tick_default();
            self.reset_fall_distance();
        }
        self.check_riding_statistics(self.position() - pre);
    }

    fn entity_stop_riding(&self) {
        let old_vehicle = self.vehicle();
        self.base().stop_riding();
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
        self.living().get_health() > 0.0 && !self.is_spectator() && !self.on_climbable()
    }

    fn entity_can_freeze(&self) -> bool {
        if self.is_spectator() {
            return false;
        }

        self.living().default_living_can_freeze()
    }

    fn entity_make_stuck_in_block(&self, state: BlockStateId, speed_multiplier: DVec3) {
        if !self.is_flying() {
            self.make_stuck_in_block_default(state, speed_multiplier);
        }

        // TODO: Reset current impulse context once vehicle/player impulse contexts exist.
    }

    fn entity_can_be_hit_by_projectile(&self) -> bool {
        self.living().get_health() > 0.0 && self.is_pickable()
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

    fn entity_cause_fall_damage(
        &self,
        _entity: &SharedEntity,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if self.abilities.lock().may_fly {
            return false;
        }

        if fall_distance >= 2.0 {
            self.award_custom_stat_with_count(
                &vanilla_custom_stats::FALL_ONE_CM,
                (fall_distance * 100.0).round() as i32,
            );
        }

        self.living()
            .cause_living_fall_damage(fall_distance, damage_modifier, source)
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

    fn entity_on_below_world(&self) {
        let world = self.get_world();
        self.hurt(
            &world,
            &DamageSource::environment(&vanilla_damage_types::OUT_OF_WORLD),
            4.0,
        );
    }

    fn entity_dimensions_for_pose(&self, pose: EntityPose) -> EntityDimensions {
        let dimensions = Player::dimensions_for_pose(pose);
        if pose == EntityPose::Sleeping || self.entity_type().fixed {
            dimensions
        } else {
            dimensions.scale(self.living().get_scale())
        }
    }

    fn entity_hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        // Delegates to Player's inherent hurt method which handles
        // player-specific prechecks before the shared living hurt path.
        Player::hurt(self, world, source, amount)
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
