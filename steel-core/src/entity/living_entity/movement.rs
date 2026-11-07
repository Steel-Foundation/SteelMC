//! Falling, jumping, gliding, climbing, impulses and pushing.

use super::*;

impl Living<'_> {
    /// Returns the small or big fall sound based on `damage`.
    #[must_use]
    pub fn fall_damage_sound(&self, damage: i32) -> SoundEventRef {
        let (small, big) = self.fall_sounds();
        if damage > 4 { big } else { small }
    }

    /// Plays the sound of the block this entity is standing in/on after a fall.
    pub fn play_block_fall_sound(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let position = self.position();
        let pos = BlockPos::new(
            position.x.floor() as i32,
            (position.y - f64::from(0.2_f32)).floor() as i32,
            position.z.floor() as i32,
        );
        let state = world.get_block_state(pos);
        if state.is_air() {
            return;
        }

        let sound_type = state.get_block().config.sound_type;
        self.play_sound(
            sound_type.fall_sound,
            sound_type.volume * 0.5,
            sound_type.pitch * 0.75,
        );
    }

    /// `LivingEntity.checkFallDamage`: refreshes fluid contact and sends the
    /// landing burst before the entity fall-damage step.
    pub fn check_living_fall_damage(
        &self,
        entity: &SharedEntity,
        vertical_movement: f64,
        on_ground: bool,
        on_state: BlockStateId,
        pos: BlockPos,
        world: &Arc<World>,
    ) {
        // Vanilla refreshes its fluid interaction when the cached state is not
        // already water. Steel refreshes contact during movement, so the last
        // interaction result is the vanilla check.
        if !self.base().was_touching_water() {
            self.update_fluid_interaction();
        }

        if on_ground && self.fall_distance() > 0.0 {
            self.send_landing_particles(world, on_state, pos);
        }

        Arc::clone(entity).check_fall_damage_default(
            vertical_movement,
            on_ground,
            on_state,
            pos,
            world,
        );
    }

    /// Sends the vanilla landing burst when the fall was powerful enough.
    fn send_landing_particles(&self, world: &World, on_state: BlockStateId, pos: BlockPos) {
        if on_state.is_air() {
            return;
        }

        let safe_fall_distance = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::SAFE_FALL_DISTANCE)
            .unwrap_or(vanilla_attributes::SAFE_FALL_DISTANCE.default_value);
        let Some(count) = landing_particle_count(self.fall_distance(), safe_fall_distance) else {
            return;
        };

        world.send_particles(
            ParticleData::new(
                &vanilla_particle_types::BLOCK,
                BlockParticleOption::new(on_state),
            ),
            landing_particle_position(self.position(), pos),
            count,
            DVec3::ZERO,
            f64::from(0.15_f32),
        );
    }

    /// `LivingEntity.getBlockSpeedFactor`: Movement Efficiency moves the block
    /// factor towards 1.
    #[must_use]
    pub fn living_block_speed_factor(&self) -> f32 {
        let efficiency =
            self.attributes()
                .lock()
                .required_value(vanilla_attributes::MOVEMENT_EFFICIENCY) as f32;
        let block_factor = self.block_speed_factor_default();
        block_factor + efficiency * (1.0 - block_factor)
    }

    /// Applies fall damage to a living entity, factoring in effects that reduce or negate it.
    #[must_use]
    pub fn cause_living_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        let effective_fall_distance =
            if let Some(impact_pos) = self.living_base().current_impulse_impact_pos() {
                let effective_fall_distance = fall_distance.min(impact_pos.y - self.position().y);
                if effective_fall_distance <= 0.0 {
                    self.reset_current_impulse_context();
                } else {
                    self.try_reset_current_impulse_context();
                }
                effective_fall_distance
            } else {
                fall_distance
            };

        if self.is_fall_damage_immune() {
            return false;
        }

        self.propagate_fall_to_passengers(effective_fall_distance, damage_modifier, source);

        let attributes = self.attributes().lock();
        let safe_fall_distance = attributes
            .get_value(vanilla_attributes::SAFE_FALL_DISTANCE)
            .unwrap_or(vanilla_attributes::SAFE_FALL_DISTANCE.default_value);
        let fall_damage_multiplier = attributes
            .get_value(vanilla_attributes::FALL_DAMAGE_MULTIPLIER)
            .unwrap_or(vanilla_attributes::FALL_DAMAGE_MULTIPLIER.default_value);
        drop(attributes);

        let damage = LivingEntityBase::calculate_fall_damage(
            effective_fall_distance,
            damage_modifier,
            safe_fall_distance,
            fall_damage_multiplier,
        );
        if damage <= 0 {
            return false;
        }

        self.reset_current_impulse_context();
        self.play_sound(self.fall_damage_sound(damage), 1.0, 1.0);
        self.play_block_fall_sound();
        if let Some(world) = self.level() {
            self.hurt(&world, source, damage as f32);
        }
        true
    }

    /// Gets the gravity value from the attribute system.
    #[must_use]
    pub fn attribute_gravity(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::GRAVITY)
    }

    /// Returns this entity's gravity, reduced while falling under Slow Falling.
    #[must_use]
    pub fn effective_gravity(&self) -> f64 {
        let gravity = self.gravity();
        if self.velocity().y <= 0.0 && self.has_mob_effect(vanilla_mob_effects::SLOW_FALLING) {
            gravity.min(0.01)
        } else {
            gravity
        }
    }

    /// Checks if the entity is fall flying (using elytra).
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        self.living_base().is_fall_flying()
    }

    /// Sets whether this entity is fall flying.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        self.set_shared_fall_flying(fall_flying);
        self.living_base().set_fall_flying(fall_flying);
    }

    /// Returns ticks this entity has been fall-flying.
    #[must_use]
    pub fn fall_flying_ticks(&self) -> i32 {
        self.living_base().fall_flying_ticks()
    }

    /// Returns whether the given item in the given slot allows elytra-style gliding.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub fn can_glide_using_default(&self, item_stack: &ItemStack, slot: EquipmentSlot) -> bool {
        let Some(equippable) = item_stack.get_equippable() else {
            return false;
        };

        item_stack.has(GLIDER) && equippable.slot == slot && !item_stack.next_damage_will_break()
    }

    /// Returns whether the item in `slot` can be used for vanilla gliding.
    #[must_use]
    pub fn can_glide_using_equipment_slot(&self, slot: EquipmentSlot) -> bool {
        let mut can_glide = false;
        self.with_equipment_slot(slot, &mut |item_stack| {
            can_glide = self.can_glide_using(item_stack, slot);
        });
        can_glide
    }

    /// Damages a random equipped item slot that's currently enabling this
    /// entity to glide, breaking it if durability runs out.
    pub fn damage_random_glider(&self) {
        let mut slots_with_gliders = Vec::new();
        for slot in EquipmentSlot::ALL {
            if self.can_glide_using_equipment_slot(slot) {
                slots_with_gliders.push(slot);
            }
        }

        let slot_count = slots_with_gliders.len();
        if slot_count == 0 {
            return;
        }

        let slot_index = rand::random_range(0..slot_count);
        let slot_to_damage = slots_with_gliders[slot_index];
        let has_infinite_materials = self.has_infinite_materials();
        let mut item_broke = false;
        let mut item_ref = &*vanilla_items::AIR;
        self.with_equipment_slot_mut(slot_to_damage, &mut |item_stack| {
            item_ref = item_stack.item;
            item_broke = item_stack.hurt_and_break(1, has_infinite_materials);
        });
        if item_broke {
            self.on_equipped_item_broken(item_ref, slot_to_damage);
        }
    }

    /// Returns whether this entity meets the base conditions to glide: not
    /// grounded, not a passenger, not levitating, and wearing something that
    /// allows gliding.
    #[must_use]
    pub fn default_can_glide(&self) -> bool {
        !self.on_ground()
            && !self.is_passenger()
            && !self.has_mob_effect(vanilla_mob_effects::LEVITATION)
            && EquipmentSlot::ALL
                .iter()
                .any(|&slot| self.can_glide_using_equipment_slot(slot))
    }

    /// Returns whether the entity currently meets the conditions to glide with an elytra.
    #[must_use]
    pub fn can_glide_default(&self) -> bool {
        self.default_can_glide()
    }

    /// Puts the entity into the fall-flying (elytra gliding) state.
    pub fn start_fall_flying(&self) {
        self.set_fall_flying(true);
    }

    /// Attempts to start fall flying if the entity isn't already gliding, can glide, and isn't in water.
    #[must_use]
    pub fn try_to_start_fall_flying(&self) -> bool {
        if !self.is_fall_flying() && self.can_glide() && !self.is_in_water() {
            self.start_fall_flying();
            return true;
        }

        false
    }

    /// Returns the last climbable block position this living entity touched.
    #[must_use]
    pub fn last_climbable_pos(&self) -> Option<BlockPos> {
        self.living_base().last_climbable_pos()
    }

    /// Records the last climbable block position this living entity touched.
    pub fn set_last_climbable_pos(&self, pos: BlockPos) {
        self.living_base().set_last_climbable_pos(pos);
    }

    /// Returns whether this entity is currently touching a climbable block;
    /// always false for spectators.
    #[must_use]
    pub fn default_living_on_climbable(&self) -> bool {
        if self.is_spectator() {
            return false;
        }

        let pos = self.block_position();
        let Some(world) = self.level() else {
            return false;
        };
        let state = world.get_block_state(pos);
        let block = state.get_block();

        if self.is_fall_flying() && block.has_tag(&BlockTag::CAN_GLIDE_THROUGH) {
            return false;
        }

        let climbable = block.has_tag(&BlockTag::CLIMBABLE)
            || block.has_tag(&BlockTag::TRAPDOORS)
                && trapdoor_usable_as_ladder_state(state, world.get_block_state(pos.below()));

        if climbable {
            self.set_last_climbable_pos(pos);
        }

        climbable
    }

    /// Returns whether vanilla living travel should skip friction damping.
    #[must_use]
    pub fn should_discard_friction(&self) -> bool {
        self.living_base().should_discard_friction()
    }

    /// Sets whether vanilla living travel should skip friction damping.
    pub fn set_discard_friction(&self, discard_friction: bool) {
        self.living_base().set_discard_friction(discard_friction);
    }

    /// Returns whether this living entity is currently applying jump input.
    #[must_use]
    pub fn is_jumping(&self) -> bool {
        self.living_base().is_jumping()
    }

    /// Sets whether this living entity is currently applying jump input.
    pub fn set_jumping(&self, jumping: bool) {
        self.living_base().set_jumping(jumping);
    }

    /// Returns vanilla living travel input.
    #[must_use]
    pub fn travel_input(&self) -> LivingTravelInput {
        self.living_base().travel_input()
    }

    /// Sets vanilla living travel input.
    pub fn set_travel_input(&self, input: LivingTravelInput) {
        self.living_base().set_travel_input(input);
    }

    /// Applies horizontal damping to the entity's stored travel input for this tick.
    pub fn apply_input(&self) {
        self.living_base().dampen_travel_input();
    }

    /// Returns vanilla jump cooldown ticks.
    #[must_use]
    pub fn no_jump_delay(&self) -> i32 {
        self.living_base().no_jump_delay()
    }

    /// Sets vanilla jump cooldown ticks.
    pub fn set_no_jump_delay(&self, ticks: i32) {
        self.living_base().set_no_jump_delay(ticks);
    }

    /// Decrements vanilla jump cooldown once per living AI step.
    pub fn tick_no_jump_delay(&self) {
        self.living_base().tick_no_jump_delay();
    }

    /// returns the remaining hurt animation ticks
    #[must_use]
    pub fn hurt_time(&self) -> i32 {
        self.living_base().hurt_time()
    }

    /// Returns vanilla `LivingEntity.isImmobile()`.
    /// Returns whether this entity is immobile because it's dead or dying.
    #[must_use]
    pub fn default_is_immobile(&self) -> bool {
        self.is_dead_or_dying()
    }

    /// Returns whether this entity is currently immobile.
    #[must_use]
    pub fn is_immobile_default(&self) -> bool {
        self.default_is_immobile()
    }

    /// Zeroes out velocity components too small to matter, using a stricter
    /// horizontal threshold for players than other entities.
    pub fn apply_living_velocity_thresholds(&self) {
        let movement = self.velocity();
        let mut dx = movement.x;
        let mut dy = movement.y;
        let mut dz = movement.z;

        if self.entity_type() == &vanilla_entities::PLAYER {
            if movement.x.mul_add(movement.x, movement.z * movement.z) < 9.0E-6 {
                dx = 0.0;
                dz = 0.0;
            }
        } else {
            if movement.x.abs() < 0.003 {
                dx = 0.0;
            }
            if movement.z.abs() < 0.003 {
                dz = 0.0;
            }
        }

        if movement.y.abs() < 0.003 {
            dy = 0.0;
        }

        self.set_velocity(DVec3::new(dx, dy, dz));
    }

    /// Server AI hook called from vanilla `LivingEntity.aiStep()`.
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub fn server_ai_step_default(&self, _entity: &SharedEntity) {}

    /// Returns the jump-height bonus from the Jump Boost effect, scaling with amplifier.
    #[must_use]
    pub fn jump_boost_power(&self) -> f32 {
        self.mob_effect(vanilla_mob_effects::JUMP_BOOST)
            .map_or(0.0, |effect| 0.1 * (effect.amplifier() as f32 + 1.0))
    }

    /// Returns jump velocity from the jump-strength attribute, scaled by
    /// `multiplier`, the standing block's jump factor, and Jump Boost.
    #[must_use]
    pub fn jump_power_with_multiplier(&self, multiplier: f32) -> f32 {
        let jump_strength =
            self.attributes()
                .lock()
                .get_value(vanilla_attributes::JUMP_STRENGTH)
                .unwrap_or(vanilla_attributes::JUMP_STRENGTH.default_value) as f32;
        jump_strength * multiplier * self.block_jump_factor() + self.jump_boost_power()
    }

    /// Returns this entity's jump power at the default multiplier.
    #[must_use]
    pub fn jump_power(&self) -> f32 {
        self.jump_power_with_multiplier(1.0)
    }

    /// Applies this entity's jump velocity, unless jump power is negligible.
    pub fn default_jump_from_ground(&self) {
        let jump_power = self.jump_power();
        if jump_power <= 1.0E-5 {
            return;
        }

        let movement = self.velocity();
        self.set_velocity(DVec3::new(
            movement.x,
            movement.y.max(f64::from(jump_power)),
            movement.z,
        ));
        if self.is_sprinting() {
            let angle = self.rotation().0.to_radians();
            self.set_velocity(
                self.velocity()
                    + DVec3::new(
                        -f64::from(trig::sin(angle)) * 0.2,
                        0.0,
                        f64::from(trig::cos(angle)) * 0.2,
                    ),
            );
        }

        self.mark_velocity_sync();
    }

    /// Applies the entity's jump velocity.
    pub fn jump_from_ground_default(&self) {
        self.default_jump_from_ground();
    }

    /// Applies downward velocity while submerged and not jumping out of a liquid.
    pub fn go_down_in_water(&self) {
        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(-0.04_f32), 0.0));
    }

    /// Applies upward velocity to jump out of the given liquid.
    pub fn jump_in_liquid(&self, _fluid_tag: &Identifier) {
        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    }

    /// Handles jump input each AI step: jumps from the ground when clear of
    /// jump delay, or jumps within water/lava when only partially submerged.
    pub fn handle_living_jump(&self) {
        if !self.is_jumping() || !self.is_affected_by_fluids() {
            self.set_no_jump_delay(0);
            return;
        }

        let fluid_height = if self.is_in_lava() {
            self.fluid_contact().lava_height()
        } else {
            self.fluid_contact().water_height()
        };
        let in_water_and_has_fluid_height = self.is_in_water() && fluid_height > 0.0;
        let fluid_jump_threshold = self.fluid_jump_threshold();
        if !in_water_and_has_fluid_height
            || self.on_ground() && fluid_height <= fluid_jump_threshold
        {
            if !self.is_in_lava() || self.on_ground() && fluid_height <= fluid_jump_threshold {
                if (self.on_ground()
                    || in_water_and_has_fluid_height && fluid_height <= fluid_jump_threshold)
                    && self.no_jump_delay() == 0
                {
                    self.jump_from_ground();
                    self.set_no_jump_delay(10);
                }
            } else {
                self.jump_in_liquid(&vanilla_fluid_tags::FluidTag::LAVA);
            }
        } else {
            self.jump_in_liquid(&vanilla_fluid_tags::FluidTag::WATER);
        }
    }

    /// Ticks side effects of a rider controlling this entity, such as steering-based input.
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn tick_ridden_default(&self, _controller: &Player, _ridden_input: DVec3) {}

    /// Transforms the rider's raw input into this entity's movement input while ridden.
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn ridden_input_default(&self, _controller: &Player, self_input: DVec3) -> DVec3 {
        self_input
    }

    /// Default for [`Self::ridden_speed`]: the current speed.
    pub fn ridden_speed_default(&self, _controller: &Player) -> f32 {
        self.speed()
    }

    /// Handles movement while this entity is being ridden and steered by a controlling player.
    pub fn travel_ridden(
        &self,
        entity: &SharedEntity,
        controller: &Player,
        self_input: DVec3,
    ) -> Option<MoveResult> {
        let ridden_input = self.ridden_input(controller, self_input);
        self.tick_ridden(controller, ridden_input);
        if self.can_simulate_movement() {
            self.set_speed(self.ridden_speed(controller));
            return self.travel(entity, ridden_input);
        }

        self.set_velocity(DVec3::ZERO);
        None
    }

    /// Default vanilla-shaped `LivingEntity.aiStep()` movement foundation for overrides.
    ///
    /// This covers the shared travel state Steel currently has; mob AI and
    /// equipment ticking are still separate follow-up work.
    pub fn default_ai_step(&self, entity: &SharedEntity) -> Option<MoveResult> {
        self.tick_no_jump_delay();
        if !self.can_simulate_movement() {
            self.set_velocity(self.velocity() * 0.98);
        }

        self.tick_equipment();
        self.apply_living_velocity_thresholds();
        self.apply_input();
        if self.is_immobile() {
            self.set_jumping(false);
            let input = self.travel_input();
            self.set_travel_input(LivingTravelInput::new(0.0, input.vertical(), 0.0));
        } else if self.is_effective_ai() {
            self.server_ai_step(entity);
        }

        self.handle_living_jump();

        if self.is_fall_flying() {
            self.update_fall_flying();
        }

        if self.has_mob_effect(vanilla_mob_effects::SLOW_FALLING)
            || self.has_mob_effect(vanilla_mob_effects::LEVITATION)
        {
            self.reset_fall_distance();
        }

        let input = self.travel_input();
        let input = DVec3::new(
            f64::from(input.sideways()),
            f64::from(input.vertical()),
            f64::from(input.forward()),
        );
        let result = if Entity::is_alive(self.entity())
            && let Some(controller_entity) = self.controlling_passenger()
            && let Some(controller) = controller_entity.as_player()
        {
            self.travel_ridden(entity, controller, input)
        } else if self.can_simulate_movement() && self.is_effective_ai() {
            self.travel(entity, input)
        } else {
            None
        };

        self.apply_effects_from_blocks();
        self.tick_freezing();
        self.push_entities();
        result
    }

    /// Runs one AI-driven movement and effect tick for the entity.
    pub fn ai_step_default(&self, entity: &SharedEntity) -> Option<MoveResult> {
        self.default_ai_step(entity)
    }

    /// Pushes nearby colliding entities away from this one.
    pub fn push_entities(&self) {
        EntityPush::push_entities(self);
    }

    /// Pushes against `entities` in the given order. The caller applies cramming damage first.
    pub fn push_entities_in_order(&self, entities: &[SharedEntity]) {
        EntityPush::push_entities_in_order(self.as_entity_event_source(), entities);
    }

    /// Applies vanilla max entity cramming damage from `LivingEntity.pushEntities()`.
    pub fn apply_entity_cramming_damage(&self, world: &World, pushable_entities: &[SharedEntity]) {
        let max_cramming = world.get_game_rule(&MAX_ENTITY_CRAMMING);

        if max_cramming <= 0 || pushable_entities.len() <= (max_cramming - 1) as usize {
            return;
        }

        let random_roll = rand::random_range(0..4);
        let non_passenger_count = pushable_entities
            .iter()
            .filter(|entity| !entity.is_passenger())
            .count();

        if should_apply_entity_cramming_damage(
            max_cramming,
            pushable_entities.len(),
            non_passenger_count,
            random_roll,
        ) {
            self.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::CRAMMING),
                6.0,
            );
        }
    }

    /// Checks if the entity is sprinting.
    #[must_use]
    pub fn is_sprinting(&self) -> bool {
        self.living_base().is_sprinting()
    }

    /// Sets whether the entity is sprinting.
    pub fn set_sprinting(&self, sprinting: bool) {
        self.set_shared_sprinting(sprinting);
        self.living_base().set_sprinting(sprinting);
    }

    /// Gets the entity's cached movement speed.
    #[must_use]
    pub fn speed(&self) -> f32 {
        self.living_base().speed()
    }

    /// Sets the entity's cached movement speed.
    pub fn set_speed(&self, speed: f32) {
        self.living_base().set_speed(speed);
    }

    /// Applies vanilla post-impulse movement validation grace.
    pub fn apply_post_impulse_grace_time(&self, ticks: i32) {
        self.living_base().apply_post_impulse_grace_time(ticks);
    }

    /// Sets whether fall damage from the current knockback/impulse impact should be ignored.
    pub fn set_ignore_fall_damage_from_current_impulse(
        &self,
        ignore_fall_damage: bool,
        new_impulse_impact_pos: DVec3,
    ) {
        self.living_base()
            .set_ignore_fall_damage_from_current_impulse(
                ignore_fall_damage,
                new_impulse_impact_pos,
            );
    }

    /// Returns whether fall damage should currently be ignored due to a
    /// recent knockback/impulse impact.
    #[must_use]
    pub fn is_ignoring_fall_damage_from_current_impulse(&self) -> bool {
        self.living_base()
            .is_ignoring_fall_damage_from_current_impulse()
    }

    /// Returns the position of the most recent knockback/impulse impact, if still active.
    #[must_use]
    pub fn current_impulse_impact_pos(&self) -> Option<DVec3> {
        self.living_base().current_impulse_impact_pos()
    }

    /// Attempts to reset the current impulse context.
    pub fn try_reset_current_impulse_context(&self) {
        self.living_base().try_reset_current_impulse_context();
    }

    /// Resets the current impulse context.
    pub fn reset_current_impulse_context(&self) {
        self.living_base().reset_current_impulse_context();
    }

    /// Returns whether movement validation is inside post-impulse grace.
    #[must_use]
    pub fn is_in_post_impulse_grace_time(&self) -> bool {
        self.living_base().is_in_post_impulse_grace_time()
    }

    /// Decrements post-impulse grace once per living-entity tick.
    pub fn tick_post_impulse_grace_time(&self) {
        self.living_base().tick_post_impulse_grace_time();
    }

    /// Returns `true` and commits the move on success, or
    /// `false` and leaves the entity untouched on failure.
    pub fn random_teleport(
        &self,
        world: &Arc<World>,
        x: f64,
        y: f64,
        z: f64,
        broadcast: bool,
    ) -> bool {
        let Some(landing_y) = random_teleport_ground_y(world, x, y, z) else {
            return false;
        };

        let dimensions = self.base().dimensions();
        let aabb = WorldAabb::entity_box(
            x,
            landing_y,
            z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        );
        let collision = WorldCollisionProvider::for_entity(world, self.as_entity_event_source());
        if collision.has_entity_collision(&aabb)
            || collision.has_block_collision_with_context(&aabb, BlockCollisionContext::empty())
            || aabb_contains_any_liquid(world, aabb)
        {
            return false;
        }

        if self.teleport_to(DVec3::new(x, landing_y, z)).is_err() {
            return false;
        }

        if let Some(pathfinder) = self.as_pathfinder_mob() {
            pathfinder.mob_base().navigation().lock().stop();
        }

        if broadcast {
            self.broadcast_entity_event(EntityStatus::Teleport);
        }
        true
    }
}

/// Walks down from `y` until standing on a block that blocks motion,
/// mirroring the descent loop in vanilla `Entity.randomTeleport`. Returns
/// `None` if the world's floor is reached without finding solid ground.
fn random_teleport_ground_y(world: &Arc<World>, x: f64, y: f64, z: f64) -> Option<f64> {
    let mut pos = BlockPos::containing(x, y, z);
    let mut current_y = y;
    while pos.y() > world.get_min_y() {
        let below = pos.below();
        if world.get_block_state(below).blocks_motion() {
            return Some(current_y);
        }
        current_y -= 1.0;
        pos = below;
    }
    None
}
