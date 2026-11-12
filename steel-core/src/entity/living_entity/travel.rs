//! `LivingEntity.travel` and its fluid, air and gliding variants.

use super::*;

impl Living<'_> {
    /// Returns whether this entity is suppressing sliding down a ladder (sneaking on one).
    #[must_use]
    pub fn is_suppressing_sliding_down_ladder(&self) -> bool {
        self.is_suppressing_bounce()
    }

    /// Returns a levitation velocity adjustment for `travelInAir`.
    #[must_use]
    pub fn levitation_travel_y_delta(&self, movement_y: f64) -> Option<f64> {
        self.mob_effect(vanilla_mob_effects::LEVITATION)
            .map(|effect| (0.05 * f64::from(effect.amplifier() + 1) - movement_y) * 0.2)
    }

    /// Returns whether movement should use fluid physics: currently in water
    /// or lava, affected by fluids, and unable to stand on this fluid.
    #[must_use]
    pub fn should_travel_in_fluid(&self, fluid_state: FluidState) -> bool {
        (self.is_in_water() || self.is_in_lava())
            && self.is_affected_by_fluids()
            && !self.can_stand_on_fluid(fluid_state)
    }

    /// Returns the water-movement slowdown multiplier.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn water_slow_down_default(&self) -> f32 {
        0.8
    }

    /// Returns the water movement efficiency attribute used by fluid travel.
    #[must_use]
    pub fn water_movement_efficiency(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::WATER_MOVEMENT_EFFICIENCY)
            .unwrap_or(0.0) as f32
    }

    /// Returns whether dolphin's grace should apply to water travel.
    #[must_use]
    pub fn has_dolphins_grace(&self) -> bool {
        self.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE)
    }

    /// Returns airborne movement speed: scaled from this entity's speed when
    /// ridden by a player, otherwise a small constant.
    #[must_use]
    pub fn flying_speed_default(&self) -> f32 {
        if self
            .controlling_passenger()
            .is_some_and(|passenger| passenger.entity_type() == &vanilla_entities::PLAYER)
        {
            self.speed() * 0.1
        } else {
            0.02
        }
    }

    /// Returns movement speed adjusted by ground friction; airborne entities
    /// use flying speed instead.
    #[must_use]
    pub fn friction_influenced_speed(&self, block_friction: f32) -> f32 {
        if self.on_ground() {
            self.speed() * (0.216_000_02 / (block_friction * block_friction * block_friction))
        } else {
            self.flying_speed()
        }
    }

    /// Returns the vertical friction used by `travelInAir`.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn air_travel_vertical_friction_default(&self, _horizontal_friction: f32) -> f32 {
        // TODO: FlyingAnimal uses horizontal friction here once animal types exist.
        0.98
    }

    /// Clamps movement while on a climbable block: caps horizontal and
    /// downward speed, and resets fall distance; sneaking players holding on
    /// don't slide down non-scaffolding ladders.
    #[must_use]
    pub fn handle_on_climbable(&self, movement: DVec3) -> DVec3 {
        if !self.on_climbable() {
            return movement;
        }

        self.reset_fall_distance();
        let Some(world) = self.level() else {
            return movement;
        };
        let block_state = self.in_block_state(&world);
        let mut y = movement.y.max(-0.15);
        if y < 0.0
            && block_state.get_block() != &vanilla_blocks::SCAFFOLDING
            && self.is_suppressing_sliding_down_ladder()
            && self.entity_type() == &vanilla_entities::PLAYER
        {
            y = 0.0;
        }

        DVec3::new(
            movement.x.clamp(-0.15, 0.15),
            y,
            movement.z.clamp(-0.15, 0.15),
        )
    }

    /// Applies gravity using vanilla living-entity effective gravity.
    pub fn apply_living_travel_gravity(&self) {
        let gravity = self.effective_gravity();
        if gravity != 0.0 {
            let mut velocity = self.velocity();
            velocity.y -= gravity;
            self.set_velocity(velocity);
        }
    }

    /// Applies friction-adjusted relative movement and returns the resulting velocity and move result.
    #[must_use]
    pub fn handle_relative_friction_and_calculate_movement(
        &self,
        input: DVec3,
        block_friction: f32,
    ) -> Option<(DVec3, MoveResult)> {
        self.move_relative(self.friction_influenced_speed(block_friction), input);
        self.set_velocity(self.handle_on_climbable(self.velocity()));
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        let mut movement = self.velocity();
        if (result.horizontal_collision || self.is_jumping())
            && (self.on_climbable()
                || self.was_in_powder_snow()
                    && PowderSnowBlock::can_entity_walk_on_powder_snow(self.entity()))
        {
            movement.y = 0.2;
        }

        Some((movement, result))
    }

    /// Handles movement, gravity, and air friction while the entity is airborne.
    #[must_use]
    pub fn travel_in_air(&self, input: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        let pos_below = self.block_pos_below_that_affects_movement()?;
        let block_friction = if self.on_ground() {
            world.get_block_state(pos_below).get_block().config.friction
        } else {
            1.0
        };
        let horizontal_friction = block_friction * 0.91;
        let (movement, result) =
            self.handle_relative_friction_and_calculate_movement(input, block_friction)?;
        let movement_y = if let Some(levitation_y) = self.levitation_travel_y_delta(movement.y) {
            movement.y + levitation_y
        } else {
            movement.y - self.effective_gravity()
        };

        if self.should_discard_friction() {
            self.set_velocity(DVec3::new(movement.x, movement_y, movement.z));
        } else {
            let vertical_friction = self.air_travel_vertical_friction(horizontal_friction);
            self.set_velocity(DVec3::new(
                movement.x * f64::from(horizontal_friction),
                movement_y * f64::from(vertical_friction),
                movement.z * f64::from(horizontal_friction),
            ));
        }

        Some(result)
    }

    /// Adjusts vertical movement while falling through a fluid to smooth the fall speed.
    #[must_use]
    pub fn fluid_falling_adjusted_movement(
        &self,
        base_gravity: f64,
        is_falling: bool,
        movement: DVec3,
    ) -> DVec3 {
        if base_gravity == 0.0 || self.is_sprinting() {
            return movement;
        }

        let y = if is_falling
            && (movement.y - 0.005).abs() >= 0.003
            && (movement.y - base_gravity / 16.0).abs() < 0.003
        {
            -0.003
        } else {
            movement.y - base_gravity / 16.0
        };

        DVec3::new(movement.x, y, movement.z)
    }

    /// Gives the entity an upward boost when it collides horizontally while rising out of a fluid.
    pub fn jump_out_of_fluid(&self, old_y: f64) {
        if !self.horizontal_collision() {
            return;
        }

        let movement = self.velocity();
        let target_delta = DVec3::new(
            movement.x,
            movement.y + f64::from(0.6_f32) - self.position().y + old_y,
            movement.z,
        );
        if self.is_free(target_delta) {
            self.set_velocity(DVec3::new(movement.x, f64::from(0.3_f32), movement.z));
        }
    }

    /// Applies buoyancy to keep a ridden entity afloat when it's tagged to float while ridden.
    pub fn float_in_water_while_ridden(&self) {
        if !REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::CAN_FLOAT_WHILE_RIDDEN)
        {
            return;
        }
        if !self.is_vehicle() || self.fluid_contact().water_height() <= self.fluid_jump_threshold()
        {
            return;
        }

        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    }

    /// Handles movement, drag, and buoyancy effects while the entity is swimming in water.
    #[must_use]
    pub fn travel_in_water(
        &self,
        input: DVec3,
        base_gravity: f64,
        is_falling: bool,
        old_y: f64,
    ) -> Option<MoveResult> {
        let mut slow_down = if self.is_sprinting() {
            0.9
        } else {
            self.water_slow_down()
        };
        let mut speed = 0.02;
        let mut water_movement_efficiency = self.water_movement_efficiency();
        if !self.on_ground() {
            water_movement_efficiency *= 0.5;
        }

        if water_movement_efficiency > 0.0 {
            slow_down += (0.546_000_06 - slow_down) * water_movement_efficiency;
            speed += (self.speed() - speed) * water_movement_efficiency;
        }

        if self.has_dolphins_grace() {
            slow_down = 0.96;
        }

        self.move_relative(speed, input);
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        let mut movement = self.velocity();
        if result.horizontal_collision && self.on_climbable() {
            movement.y = 0.2;
        }

        movement = DVec3::new(
            movement.x * f64::from(slow_down),
            movement.y * f64::from(0.8_f32),
            movement.z * f64::from(slow_down),
        );
        self.set_velocity(self.fluid_falling_adjusted_movement(base_gravity, is_falling, movement));
        self.jump_out_of_fluid(old_y);

        Some(result)
    }

    /// Handles movement and drag effects while the entity is moving through lava.
    #[must_use]
    pub fn travel_in_lava(
        &self,
        input: DVec3,
        base_gravity: f64,
        is_falling: bool,
        old_y: f64,
    ) -> Option<MoveResult> {
        self.move_relative(0.02, input);
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        if self.fluid_contact().lava_height() <= self.fluid_jump_threshold() {
            let movement = self.velocity();
            self.set_velocity(DVec3::new(
                movement.x * 0.5,
                movement.y * f64::from(0.8_f32),
                movement.z * 0.5,
            ));
            self.set_velocity(self.fluid_falling_adjusted_movement(
                base_gravity,
                is_falling,
                self.velocity(),
            ));
        } else {
            self.set_velocity(self.velocity() * 0.5);
        }

        if base_gravity != 0.0 {
            self.set_velocity(self.velocity() + DVec3::new(0.0, -base_gravity / 4.0, 0.0));
        }

        self.jump_out_of_fluid(old_y);

        Some(result)
    }

    /// Dispatches fluid movement handling to the water or lava variant based on the current fluid.
    #[must_use]
    pub fn travel_in_fluid(&self, input: DVec3) -> Option<MoveResult> {
        let is_falling = self.velocity().y <= 0.0;
        let old_y = self.position().y;
        let base_gravity = self.effective_gravity();
        if self.is_in_water() {
            let result = self.travel_in_water(input, base_gravity, is_falling, old_y);
            self.float_in_water_while_ridden();
            return result;
        }

        self.travel_in_lava(input, base_gravity, is_falling, old_y)
    }

    /// Mirrors the validation part of vanilla `LivingEntity.updateFallFlying()`.
    pub fn update_fall_flying(&self) {
        self.check_fall_distance_accumulation();
        if self.can_glide() {
            if let Some(free_fall_interval) =
                fall_flying_free_fall_interval(self.fall_flying_ticks())
            {
                if free_fall_interval % 2 == 0 {
                    self.damage_random_glider();
                }
                self.game_event(&vanilla_game_events::ELYTRA_GLIDE);
            }
        } else {
            self.set_fall_flying(false);
        }
    }

    /// Computes the next movement vector for elytra gliding based on look direction and lift.
    #[must_use]
    pub fn update_fall_flying_movement(&self, mut movement: DVec3) -> DVec3 {
        let look_angle = self.look_angle();
        let pitch_radians = self.rotation().1.to_radians();
        let look_horizontal_length = horizontal_distance(look_angle);
        let move_horizontal_length = horizontal_distance(movement);
        let gravity = self.effective_gravity();
        let lift_force = f64::from(pitch_radians).cos().powi(2);
        movement.y += gravity * (-1.0 + lift_force * 0.75);

        if movement.y < 0.0 && look_horizontal_length > 0.0 {
            let convert = movement.y * -0.1 * lift_force;
            movement += DVec3::new(
                look_angle.x * convert / look_horizontal_length,
                convert,
                look_angle.z * convert / look_horizontal_length,
            );
        }

        if pitch_radians < 0.0 && look_horizontal_length > 0.0 {
            let convert = move_horizontal_length * -f64::from(trig::sin(pitch_radians)) * 0.04;
            movement += DVec3::new(
                -look_angle.x * convert / look_horizontal_length,
                convert * 3.2,
                -look_angle.z * convert / look_horizontal_length,
            );
        }

        if look_horizontal_length > 0.0 {
            movement += DVec3::new(
                (look_angle.x / look_horizontal_length * move_horizontal_length - movement.x) * 0.1,
                0.0,
                (look_angle.z / look_horizontal_length * move_horizontal_length - movement.z) * 0.1,
            );
        }

        DVec3::new(
            movement.x * f64::from(0.99_f32),
            movement.y * f64::from(0.98_f32),
            movement.z * f64::from(0.99_f32),
        )
    }

    /// Toggles fall flying off, ending the elytra gliding state.
    pub fn stop_fall_flying(&self) {
        self.set_fall_flying(true);
        self.set_fall_flying(false);
    }

    /// Applies fly-into-wall damage when a horizontal collision occurs while gliding.
    pub fn handle_fall_flying_collisions(
        &self,
        previous_horizontal_speed: f64,
        new_horizontal_speed: f64,
    ) {
        if !self.horizontal_collision() {
            return;
        }

        let damage = fall_flying_collision_damage(previous_horizontal_speed, new_horizontal_speed);
        if damage <= 0.0 {
            return;
        }

        self.play_sound(self.fall_damage_sound(damage as i32), 1.0, 1.0);
        if let Some(world) = self.level() {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::FLY_INTO_WALL),
                damage,
            );
        }
    }

    /// Handles movement, lift, and collision damage while the entity is gliding with an elytra.
    #[must_use]
    pub fn travel_fall_flying(&self, input: DVec3) -> Option<MoveResult> {
        if self.on_climbable() {
            let result = self.travel_in_air(input);
            self.stop_fall_flying();
            return result;
        }

        let previous_movement = self.velocity();
        let previous_horizontal_speed = horizontal_distance(previous_movement);
        self.set_velocity(self.update_fall_flying_movement(previous_movement));
        let result = self.move_entity(MoverType::SelfMovement, self.velocity());
        let new_horizontal_speed = horizontal_distance(self.velocity());
        self.handle_fall_flying_collisions(previous_horizontal_speed, new_horizontal_speed);
        result
    }

    /// Dispatches movement to fluid, fall-flying, or air travel handling
    /// based on current state.
    #[must_use]
    pub fn default_travel(&self, input: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        let fluid_state = get_fluid_state(&world, self.block_position());
        if self.should_travel_in_fluid(fluid_state) {
            return self.travel_in_fluid(input);
        }
        if self.is_fall_flying() {
            return self.travel_fall_flying(input);
        }

        self.travel_in_air(input)
    }
}
