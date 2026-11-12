//! Velocity, gravity, collisions, ground contact and moving.

use super::*;

/// Velocity, gravity, collisions, ground contact and moving.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityHooks`], never by implementing this trait.
pub trait EntityMotion: Entity {
    /// Returns whether the bounding box shifted by `delta` is free of block
    /// collisions and liquids.
    fn is_free(&self, delta: DVec3) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        let target_box = self.bounding_box().translate(delta);
        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        if collision_world.has_collision_with_context(
            &target_box.deflate(COLLISION_EPSILON),
            physics_state_for_move(self.as_entity_event_source()).block_collision_context(),
        ) {
            return false;
        }

        !aabb_contains_any_liquid(&world, target_box)
    }

    /// Default for [`Entity::is_flying_player`].
    fn is_flying_player_default(&self) -> bool {
        false
    }

    /// Default for [`Entity::can_be_collided_with`].
    fn can_be_collided_with_default(&self, _other: Option<&dyn Entity>) -> bool {
        false
    }

    /// Default for [`Entity::is_hard_collision_relevant`].
    fn is_hard_collision_relevant_default(&self) -> bool {
        true
    }

    /// Default for [`Entity::can_collide_with`].
    fn can_collide_with_default(&self, other: &dyn Entity) -> bool {
        other.can_be_collided_with(Some(self.as_entity_event_source()))
            && !self.is_passenger_of_same_vehicle(other)
    }

    /// Default for [`Entity::push_impulse`].
    fn push_impulse_default(&self, impulse: DVec3) {
        self.base().push_impulse(impulse);
    }

    /// Default for [`Entity::push_entity`].
    fn push_entity_default(&self, entity: &dyn Entity) {
        if self.is_passenger_of_same_vehicle(entity) || entity.no_physics() || self.no_physics() {
            return;
        }

        let other_position = entity.position();
        let position = self.position();

        let Some(impulse) = EntityPush::impulse(position, other_position) else {
            return;
        };

        if !self.is_vehicle() && self.is_pushable() {
            self.push_impulse(DVec3::new(-impulse.x, 0.0, -impulse.z));
        }

        if !entity.is_vehicle() && entity.is_pushable() {
            entity.push_impulse(impulse);
        }
    }

    /// Returns whether this entity's velocity should sync every tick because
    /// it's currently fall-flying with an elytra.
    fn forces_fall_flying_velocity_sync(&self) -> bool {
        self.as_living_entity()
            .is_some_and(|living| living.is_fall_flying())
    }

    /// Default for [`Entity::uses_client_movement_packets`].
    fn uses_client_movement_packets_default(&self) -> bool {
        if !self.is_removed()
            && let Some(controller) = self.controlling_passenger()
            && controller.id() != self.id()
            && controller.uses_client_movement_packets()
        {
            return true;
        }

        false
    }

    /// Returns true when normal server ticks drive this entity's movement.
    fn is_server_driven_movement(&self) -> bool {
        !self.uses_client_movement_packets()
    }

    /// Default for [`Entity::can_simulate_movement`].
    fn can_simulate_movement_default(&self) -> bool {
        self.is_server_driven_movement()
    }

    /// Default for [`Entity::is_suppressing_bounce`].
    fn is_suppressing_bounce_default(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Sets the vanilla sprinting shared flag.
    fn set_shared_sprinting(&self, sprinting: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_sprinting(sprinting);
        }
    }

    /// Sets the vanilla fall-flying shared flag.
    fn set_shared_fall_flying(&self, fall_flying: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_fall_flying(fall_flying);
        }
    }

    /// Returns whether vanilla excludes this vehicle from floating kicks.
    fn is_flying_vehicle(&self) -> bool {
        false
    }

    /// Default for [`Entity::known_movement`].
    fn known_movement_default(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_movement();
        }

        self.velocity()
    }

    /// Default for [`Entity::known_speed`].
    fn known_speed_default(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_speed();
        }

        self.base().known_speed()
    }

    /// Gets the entity's velocity in blocks per tick.
    fn velocity(&self) -> DVec3 {
        self.base().velocity()
    }

    /// Sets the entity's velocity.
    fn set_velocity(&self, velocity: DVec3) {
        self.base().set_velocity(velocity);
    }

    /// Returns whether this entity's velocity has changed enough to need
    /// syncing to clients.
    fn needs_velocity_sync(&self) -> bool {
        self.base().needs_velocity_sync()
    }

    /// Marks this entity's velocity as needing to be synced to clients.
    fn mark_velocity_sync(&self) {
        self.base().mark_velocity_sync();
    }

    /// Clears the vanilla velocity sync marker after send processing.
    fn clear_velocity_sync(&self) {
        self.base().clear_velocity_sync();
    }

    /// Returns true when vanilla hurt-marked velocity sync is pending.
    fn hurt_marked(&self) -> bool {
        self.base().hurt_marked()
    }

    /// Marks this entity as hurt for vanilla self-inclusive motion sync.
    fn mark_hurt(&self) {
        self.base().mark_hurt();
    }

    /// Clears the vanilla hurt-marked motion sync flag.
    fn clear_hurt_mark(&self) {
        self.base().clear_hurt_mark();
    }

    /// Returns accumulated vanilla fall distance.
    fn fall_distance(&self) -> f64 {
        self.base().fall_distance()
    }

    /// Sets accumulated vanilla fall distance.
    fn set_fall_distance(&self, fall_distance: f64) {
        self.base().set_fall_distance(fall_distance);
    }

    /// Resets accumulated vanilla fall distance.
    fn reset_fall_distance(&self) {
        self.base().reset_fall_distance();
    }

    /// Caps the accumulated fall distance at 1 once the entity is no longer falling faster than 0.5 blocks per tick.
    fn check_fall_distance_accumulation(&self) {
        if self.velocity().y > -0.5 && self.fall_distance() > 1.0 {
            self.set_fall_distance(1.0);
        }
    }

    /// Returns true if this entity type ignores vanilla fall damage.
    fn is_fall_damage_immune(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::FALL_DAMAGE_IMMUNE)
    }

    /// Propagates vanilla fall-damage handling to passengers.
    fn propagate_fall_to_passengers(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) {
        for passenger in self.passengers() {
            passenger.cause_fall_damage(fall_distance, damage_modifier, source);
        }
    }

    /// Default for [`Entity::cause_fall_damage`].
    fn cause_fall_damage_default(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if self.is_fall_damage_immune() {
            return false;
        }

        self.propagate_fall_to_passengers(fall_distance, damage_modifier, source);
        false
    }

    /// Returns true if the entity is on the ground.
    fn on_ground(&self) -> bool {
        self.base().on_ground()
    }

    /// Returns true if the last movement was clipped horizontally.
    fn horizontal_collision(&self) -> bool {
        self.base().horizontal_collision()
    }

    /// Returns true if the last movement was clipped vertically.
    fn vertical_collision(&self) -> bool {
        self.base().vertical_collision()
    }

    /// Returns true if the last vertical collision was below the entity.
    fn vertical_collision_below(&self) -> bool {
        self.base().vertical_collision_below()
    }

    /// Returns true when movement bypasses collision physics.
    fn no_physics(&self) -> bool {
        self.base().no_physics()
    }

    /// Returns true when vanilla block-contact effects may run for this entity.
    fn is_affected_by_blocks(&self) -> bool {
        !self.is_removed() && !self.no_physics()
    }

    /// Sets whether this entity bypasses collision physics.
    fn set_no_physics(&self, no_physics: bool) {
        self.base().set_no_physics(no_physics);
    }

    /// Updates item-style `noPhysics` from the entity's current collision state.
    fn update_no_physics_from_current_collision(&self) {
        let Some(world) = self.level() else {
            self.set_no_physics(false);
            return;
        };

        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        let colliding = collision_world.has_collision_with_context(
            &self.bounding_box().deflate(NO_PHYSICS_COLLISION_EPSILON),
            BlockCollisionContext::empty(),
        );
        self.set_no_physics(colliding);
        if colliding {
            let bounding_box = self.bounding_box();
            self.move_towards_closest_space(
                self.position().x,
                f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
                self.position().z,
            );
        }
    }

    /// Nudges velocity toward the closest non-full collision block.
    fn move_towards_closest_space(&self, x: f64, y: f64, z: f64) {
        let Some(world) = self.level() else {
            return;
        };

        let block_pos = BlockPos::containing(x, y, z);
        let fractional_position = DVec3::new(
            x - f64::from(block_pos.x()),
            y - f64::from(block_pos.y()),
            z - f64::from(block_pos.z()),
        );
        let closest_direction =
            closest_open_space_direction(block_pos, fractional_position, |neighbor_pos| {
                let block_state = world.get_block_state(neighbor_pos);
                let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
                let collision_shape = behavior.get_collision_shape(
                    block_state,
                    world.as_ref(),
                    neighbor_pos,
                    BlockCollisionContext::empty(),
                );
                is_shape_full_block(collision_shape)
            });

        let speed = f64::from(rand::random_range(0.1f32..0.3));
        let step = direction_step(closest_direction);
        let scaled_velocity = self.velocity() * 0.75;
        let next_velocity = match closest_direction.axis() {
            Axis::X => DVec3::new(step * speed, scaled_velocity.y, scaled_velocity.z),
            Axis::Y => DVec3::new(scaled_velocity.x, step * speed, scaled_velocity.z),
            Axis::Z => DVec3::new(scaled_velocity.x, scaled_velocity.y, step * speed),
        };
        self.set_velocity(next_velocity);
    }

    /// What vanilla's `/teleport` does once an entity has moved: unless it is gliding, it ends up
    /// on the ground without vertical motion.
    fn land_after_teleport(&self) {
        if self
            .as_living_entity()
            .is_none_or(|living| !living.is_fall_flying())
        {
            let velocity = self.velocity();
            self.set_velocity(DVec3::new(velocity.x, 0.0, velocity.z));
            self.set_on_ground(true);
        }
    }

    /// Sets whether the entity is on the ground.
    fn set_on_ground(&self, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, None);
        let movement_flags = self.base().movement_flags().with_on_ground(on_ground);
        self.base()
            .set_movement_flags(movement_flags, ground_contact);
    }

    /// Sets ground and horizontal collision flags from accepted movement.
    fn set_on_ground_with_movement(
        &self,
        on_ground: bool,
        horizontal_collision: bool,
        movement: DVec3,
    ) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base()
            .set_on_ground_with_movement(on_ground, horizontal_collision, ground_contact);
    }

    /// Default final state application for accepted client-authored movement.
    ///
    /// Mirrors the shared tail of vanilla player and controlled-vehicle movement
    /// handling after rollback/collision validation has accepted the target.
    fn default_apply_accepted_client_movement(
        &self,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        if let Some(position) = accepted.position {
            self.try_set_position(position)?;
        }

        self.set_rotation(accepted.rotation);
        self.set_on_ground_with_movement(
            accepted.on_ground,
            accepted.horizontal_collision,
            accepted.movement,
        );
        if self.do_check_fall_damage(accepted.movement, accepted.on_ground, world) {
            return Ok(AcceptedClientMovementOutcome::Handled);
        }
        if accepted.reset_fall_distance {
            self.reset_fall_distance();
        }

        Ok(AcceptedClientMovementOutcome::Applied)
    }

    /// Applies final state accepted from a client-authored movement packet.
    fn apply_accepted_client_movement(
        &self,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Applies final state accepted from a controlled-vehicle movement packet.
    fn apply_accepted_client_vehicle_movement(
        &self,
        world: &Arc<World>,
        mut accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        accepted.horizontal_collision = self.horizontal_collision();
        accepted.reset_fall_distance = false;
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Default for [`Entity::try_set_position`].
    fn try_set_position_default(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position_base(pos)
    }

    /// The base method to set the entity's position through world lifecycle validation.
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    fn try_set_position_base(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.base().try_set_position(pos)
    }

    /// Default for [`Entity::teleport_to`].
    // TODO: Recursively reposition this entity's passengers (vanilla
    // `Entity.teleportPassengers`, via `getSelfAndPassengers`)
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    fn teleport_to_default(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position(pos)
    }

    /// Sets the vanilla movement-trace old position to the current position.
    fn set_old_position_to_current(&self) {
        self.base().set_old_position_to_current();
    }

    /// Sets the vanilla movement-trace old position explicitly.
    fn set_old_position(&self, old_position: DVec3) {
        self.base().set_old_position(old_position);
    }

    /// Removes the latest movement segment recorded this tick.
    fn remove_latest_movement_recording(&self) {
        self.base().remove_latest_movement_recording();
    }

    /// Default for [`Entity::max_up_step`].
    fn max_up_step_default(&self) -> f32 {
        0.0
    }

    /// Default for [`Entity::backs_off_from_edge`].
    fn backs_off_from_edge_default(&self) -> bool {
        false
    }

    // These mirror vanilla's Entity class methods.

    /// Default for [`Entity::default_gravity`].
    fn default_gravity_default(&self) -> f64 {
        0.0
    }

    /// Returns true if gravity is disabled for this entity.
    fn is_no_gravity(&self) -> bool {
        self.base().no_gravity()
    }

    /// Sets whether this entity ignores gravity, syncing the change to clients.
    fn set_no_gravity(&self, no_gravity: bool) {
        self.base().set_no_gravity(no_gravity);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_no_gravity(no_gravity);
        }
    }

    /// Gets the current gravity value.
    ///
    /// Returns 0 if `no_gravity` is set, otherwise returns `default_gravity()`.
    fn gravity(&self) -> f64 {
        if self.is_no_gravity() {
            0.0
        } else {
            self.default_gravity()
        }
    }

    /// Default for [`Entity::air_drag`].
    fn air_drag_default(&self) -> f32 {
        DEFAULT_AIR_DRAG
    }

    /// Default for [`Entity::effective_gravity`].
    fn effective_gravity_default(&self) -> f64 {
        self.gravity()
    }

    /// Applies gravity to the entity's velocity.
    fn apply_gravity(&self) {
        let gravity = self.gravity();
        if gravity != 0.0 {
            let mut vel = self.velocity();
            vel.y -= gravity;
            self.set_velocity(vel);
        }
    }

    /// Adds `input` scaled by `speed`, rotated to face this entity's current
    /// yaw, to its velocity.
    fn move_relative(&self, speed: f32, input: DVec3) {
        let yaw = self.rotation().0;
        self.set_velocity(self.velocity() + get_input_vector(input, speed, yaw));
    }

    /// Moves the entity without collision physics.
    fn move_without_physics(&self, delta: DVec3) -> Option<MoveResult> {
        let final_position = self.position() + delta;
        if let Err(error) = self.try_set_position(final_position) {
            log::debug!(
                "Rejected no-physics movement for entity {}: {error}",
                self.id()
            );
            return None;
        }
        self.base().clear_collision_flags();

        Some(MoveResult {
            final_position,
            actual_movement: delta,
            on_ground: self.on_ground(),
            horizontal_collision: false,
            vertical_collision: false,
            x_collision: false,
            z_collision: false,
            final_aabb: self.bounding_box(),
        })
    }

    /// Moves the entity with collision detection.
    ///
    /// Updates position, `on_ground`, velocity (on collision), and returns collision info.
    #[expect(
        clippy::too_many_lines,
        reason = "keeps vanilla movement phases in order"
    )]
    /// Default for [`Entity::move_entity`].
    fn move_entity_default(&self, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        if self.no_physics() {
            return self.move_without_physics(delta);
        }

        let mut movement = delta;
        if mover_type == MoverType::Piston {
            let game_time = world.game_time();
            movement = self.base().limit_piston_movement(movement, game_time);
            if movement == DVec3::ZERO {
                return None;
            }
        }
        movement = self
            .base()
            .consume_stuck_speed_multiplier(movement, mover_type != MoverType::Piston);

        let physics_state = physics_state_for_move(self.as_entity_event_source());
        let start_position = physics_state.position();

        // Perform collision detection and movement
        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        let result =
            resolve_entity_movement(&physics_state, movement, mover_type, &collision_world);

        record_movement_for_block_effects(
            self.as_entity_event_source(),
            start_position,
            result.final_position,
            movement,
            result.actual_movement,
        );

        // Update entity state
        if should_apply_resolved_movement(movement, result.actual_movement) {
            self.reset_fall_distance_on_resetting_clip(&world, result.actual_movement);
            if let Err(error) = self.try_set_position(result.final_position) {
                log::debug!(
                    "Rejected resolved movement for entity {}: {error}",
                    self.id()
                );
                self.remove_latest_movement_recording();
                return None;
            }
        }
        let vertical_state_update =
            EntityVerticalMovementStateUpdate::for_move(movement, self.is_server_driven_movement());
        let movement_flags = EntityMovementFlags::after_move_with_previous(
            self.base().movement_flags(),
            vertical_state_update,
            result.on_ground,
            result.horizontal_collision,
            result.vertical_collision,
            movement,
        );
        let ground_contact = if vertical_state_update.refreshes_state() {
            self.ground_contact_after_movement(result.on_ground, Some(result.actual_movement))
        } else {
            self.base().ground_contact()
        };
        self.base()
            .set_movement_flags(movement_flags, ground_contact);

        if self.is_server_driven_movement() && self.apply_fall_damage_after_move(&result, &world) {
            return Some(result);
        }

        // Vanilla: Entity.move() zeros velocity components on collision.
        // Horizontal collision zeros X/Z individually based on which axis collided.
        // Vertical collision calls Block.updateEntityMovementAfterFallOn.
        // The default block behavior zeros Y velocity; block-specific behavior
        // can override this for slime, beds, and similar landing surfaces.
        if result.horizontal_collision {
            let vel = self.velocity();
            self.set_velocity(DVec3::new(
                if result.x_collision { 0.0 } else { vel.x },
                vel.y,
                if result.z_collision { 0.0 } else { vel.z },
            ));
        }
        if result.vertical_collision && self.can_simulate_movement() {
            let velocity = self.velocity();
            let landing_context = EntityLandingContext::new(
                velocity,
                result.actual_movement.y,
                self.is_living_entity(),
                self.is_suppressing_bounce(),
                self.effective_gravity(),
                f64::from(self.air_drag()),
            );

            let next_velocity =
                if let Some(effect_pos) = self.block_pos_below_that_affects_movement() {
                    let effect_state = world.get_block_state(effect_pos);
                    let behavior = BLOCK_BEHAVIORS.get_behavior(effect_state.get_block());
                    let (res, bounced) = behavior.update_entity_movement_after_fall_on(
                        effect_state,
                        &world,
                        effect_pos,
                        landing_context,
                    );
                    if bounced {
                        behavior.bounce_on(
                            effect_state,
                            &world,
                            effect_pos,
                            EntityFallOnContext::from_entity(
                                self.fall_distance(),
                                self.as_entity_event_source(),
                            ),
                        );
                    }
                    res
                } else {
                    landing_context.default_velocity_after_fall_on()
                };
            self.set_velocity(next_velocity);
        }

        self.apply_movement_side_effects_after_move(&world, result.actual_movement);

        let speed_factor = f64::from(self.block_speed_factor());
        let vel = self.velocity();
        self.set_velocity(DVec3::new(
            vel.x * speed_factor,
            vel.y,
            vel.z * speed_factor,
        ));

        Some(result)
    }

    /// Applies vanilla fall-distance bookkeeping after accepted movement.
    fn apply_fall_damage_after_move(&self, result: &MoveResult, world: &Arc<World>) -> bool {
        self.do_check_fall_damage(result.actual_movement, result.on_ground, world)
    }

    /// Resets fall distance when vanilla's fall-damage-resetting clip hits.
    fn reset_fall_distance_on_resetting_clip(&self, world: &Arc<World>, movement: DVec3) {
        let Some(check_to) =
            fall_damage_reset_clip_target(self.position(), movement, self.fall_distance())
        else {
            return;
        };

        let hit = world.clip(
            self.position(),
            check_to,
            ClipBlockShape::FallDamageResetting {
                entity_is_player: self.entity_type() == &vanilla_entities::PLAYER,
            },
            ClipFluid::Water,
        );
        if !hit.is_miss() {
            self.reset_fall_distance();
        }
    }

    /// Applies fall damage from the block fallen on, then returns whether the
    /// entity was removed (e.g. killed) as a result.
    ///
    /// Callers update on-ground/supporting-block state before this method.
    fn do_check_fall_damage(&self, movement: DVec3, on_ground: bool, world: &Arc<World>) -> bool {
        let Some(effect_pos) = self.on_pos_legacy() else {
            return false;
        };
        let effect_state = world.get_block_state(effect_pos);
        self.check_fall_damage(movement.y, on_ground, effect_state, effect_pos, world);
        self.is_removed()
    }

    /// Refreshes vanilla supporting-block state before fall-damage side effects.
    fn refresh_supporting_block_for_fall_damage(&self, movement: DVec3, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base().set_ground_contact(ground_contact);
    }

    /// Default for [`Entity::check_fall_damage`].
    fn check_fall_damage_default(
        &self,
        vertical_movement: f64,
        on_ground: bool,
        on_state: BlockStateId,
        pos: BlockPos,
        world: &Arc<World>,
    ) {
        if !self.is_in_water() && vertical_movement < 0.0 {
            self.base().accumulate_fall_distance(vertical_movement);
        }

        if !on_ground {
            return;
        }

        let fall_distance = self.fall_distance();
        if fall_distance > 0.0 {
            let behavior = BLOCK_BEHAVIORS.get_behavior(on_state.get_block());
            let fall_context =
                EntityFallOnContext::from_entity(fall_distance, self.as_entity_event_source());
            if let Some(fall_damage) = behavior.fall_on(on_state, world, pos, fall_context) {
                let damage_applied = self.cause_fall_damage(
                    fall_damage.fall_distance,
                    fall_damage.damage_modifier,
                    &fall_damage.source,
                );
                behavior.after_fall_on_damage(
                    on_state,
                    world,
                    pos,
                    self.as_entity_event_source(),
                    &fall_damage,
                    damage_applied,
                );
            }

            let supporting_state = self
                .base()
                .supporting_block()
                .map_or(on_state, |supporting_pos| {
                    world.get_block_state(supporting_pos)
                });
            world.game_event(
                &vanilla_game_events::HIT_GROUND,
                BlockPos::new(
                    self.position().x.floor() as i32,
                    self.position().y.floor() as i32,
                    self.position().z.floor() as i32,
                ),
                &GameEventContext::new(Some(self.as_entity_event_source()), Some(supporting_state)),
            );
        }

        self.reset_fall_distance();
    }

    /// Computes vanilla support state for an on-ground update.
    fn ground_contact_after_movement(
        &self,
        on_ground: bool,
        movement: Option<DVec3>,
    ) -> EntityGroundContact {
        let Some(world) = self.level() else {
            return if on_ground {
                EntityGroundContact::on_ground(None)
            } else {
                EntityGroundContact::airborne()
            };
        };

        self.check_supporting_block(on_ground, movement, &world)
    }

    /// Sets position and rotation.
    ///
    /// # Panics
    ///
    /// Panics if the active world entity manager rejects the snap position. This is an invariant
    /// failure for loaded entities.
    fn snap_to(&self, position: DVec3, yaw: f32, pitch: f32) {
        if let Err(error) = self.try_set_position(position) {
            panic!(
                "failed to commit entity {} snap position: {error}",
                self.id()
            );
        }
        self.set_rotation((yaw, pitch));
        self.set_old_position_to_current();
    }
}

impl<T: Entity + ?Sized> EntityMotion for T {}
