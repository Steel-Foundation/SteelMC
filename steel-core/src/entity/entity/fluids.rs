//! Water, lava, rain, bubble columns, swimming and air supply.

use super::*;

/// Water, lava, rain, bubble columns, swimming and air supply.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityProps`], never by implementing this trait.
pub trait EntityFluids: Entity {
    /// Sets the vanilla swimming shared flag.
    fn set_shared_swimming(&self, swimming: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_swimming(swimming);
        }
    }

    /// Returns the fluid-surface height threshold used for fluid-jump
    /// physics: 0.0 for entities with eye height under 0.4, otherwise 0.4.
    fn fluid_jump_threshold(&self) -> f64 {
        if self.eye_height() < 0.4 { 0.0 } else { 0.4 }
    }

    /// Returns true if this entity is currently touching water.
    fn is_in_water(&self) -> bool {
        self.fluid_contact().water_height() > 0.0
    }

    /// Returns true if rain falls at this entity's feet or at the top of its bounding box.
    fn is_in_rain(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        let pos = self.block_position();
        world.is_raining_at(pos)
            || world.is_raining_at(BlockPos::new(
                pos.x(),
                self.bounding_box().max_y().floor() as i32,
                pos.z(),
            ))
    }

    /// Returns true if this entity is touching water or standing in rain.
    fn is_in_water_or_rain(&self) -> bool {
        self.is_in_water() || self.is_in_rain()
    }

    /// Returns true if this entity is currently touching lava.
    fn is_in_lava(&self) -> bool {
        self.base().is_in_lava()
    }

    /// Returns true if this entity's eyes are currently inside water.
    fn is_eye_in_water(&self) -> bool {
        self.fluid_contact().eye_in_water()
    }

    /// Returns true if this entity's eyes are currently inside lava.
    fn is_eye_in_lava(&self) -> bool {
        self.fluid_contact().eye_in_lava()
    }

    /// Returns vanilla underwater state.
    fn is_under_water(&self) -> bool {
        self.base().was_eye_in_water() && self.is_in_water()
    }

    /// Returns cached fluid contact from the last entity fluid refresh.
    fn fluid_contact(&self) -> EntityFluidContact {
        self.base().fluid_contact()
    }

    /// Refreshes cached fluid contact from this entity's current bounding box.
    fn refresh_fluid_contact(&self) -> EntityFluidContact {
        self.scan_and_store_fluid_contact(false)
    }

    /// Runs vanilla `Entity.updateFluidInteraction`: refreshes cached contact,
    /// emits the water-entry splash on transition, resets fall distance in
    /// water, and applies fluid currents.
    ///
    /// Returns whether any fluid touches the entity.
    fn update_fluid_interaction(&self) -> bool {
        let was_touching_water = self.base().was_touching_water();
        let contact = self.scan_and_store_fluid_contact(true);
        let in_water = contact.water_height() > 0.0;
        if in_water {
            self.base().reset_fall_distance_in_water();
            if !was_touching_water && !self.is_first_tick() {
                self.do_water_splash_effect();
            }
        }
        if let Some(world) = self.level() {
            self.apply_fluid_current_impulses(&world, contact);
        }

        contact.any_fluid()
    }

    /// Scans current fluid contact and stores it on the entity base.
    ///
    /// `advance_eye_water_history` scans currents and records `wasTouchingWater` for
    /// vanilla `updateFluidInteraction`.
    fn scan_and_store_fluid_contact(&self, advance_eye_water_history: bool) -> EntityFluidContact {
        let Some(world) = self.level() else {
            let contact = EntityFluidContact::default();
            if advance_eye_water_history {
                self.base().set_fluid_interaction(contact);
            } else {
                self.base().set_fluid_contact(contact);
            }
            return contact;
        };

        let contact = if advance_eye_water_history {
            EntityFluidContact::scan_with_currents(
                &world,
                self.position(),
                self.eye_y(),
                self.bounding_box(),
                self.is_pushed_by_fluid(),
            )
        } else {
            EntityFluidContact::scan(&world, self.position(), self.eye_y(), self.bounding_box())
        };
        if advance_eye_water_history {
            self.base().set_fluid_interaction(contact);
        } else {
            self.base().set_fluid_contact(contact);
        }
        contact
    }

    /// Applies vanilla water/lava current impulses from a fluid interaction update.
    fn apply_fluid_current_impulses(&self, world: &Arc<World>, contact: EntityFluidContact) {
        if !self.is_pushed_by_fluid() {
            return;
        }

        let is_player = self.entity_type() == &vanilla_entities::PLAYER;
        let old_velocity = self.velocity();
        let water_impulse =
            contact.water_current_impulse(is_player, old_velocity, WATER_ENTITY_FLOW_SCALE);
        self.apply_fluid_current_impulse(water_impulse);

        let old_velocity = old_velocity + water_impulse;
        let lava_impulse = contact.lava_current_impulse(
            is_player,
            old_velocity,
            LavaFluid::entity_flow_scale(world),
        );
        self.apply_fluid_current_impulse(lava_impulse);
    }

    /// Applies a non-zero fluid current impulse and marks velocity sync.
    fn apply_fluid_current_impulse(&self, impulse: DVec3) {
        if impulse.length_squared() > 0.0 {
            self.push_impulse(impulse);
        }
    }

    /// Returns this entity's current air supply in ticks, synced to clients.
    fn air_supply(&self) -> i32 {
        self.base().air_supply()
    }

    /// Sets this entity's air supply in ticks, syncing the change to clients.
    fn set_air_supply(&self, air_supply: i32) {
        self.base().set_air_supply(air_supply);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_air_supply(air_supply);
        }
    }

    /// Returns this entity's maximum vanilla air supply.
    fn max_air_supply(&self) -> i32 {
        DEFAULT_MAX_AIR_SUPPLY
    }
}

impl<T: Entity + ?Sized> EntityFluids for T {}

/// `Entity`'s [`EntityProps::is_pushed_by_fluid`].
pub fn is_pushed_by_fluid<E: Entity + ?Sized>(entity: &E) -> bool {
    entity.entity_type().flags.is_pushed_by_fluid
}

/// `Entity`'s [`EntityProps::on_above_bubble_column`].
pub fn on_above_bubble_column<E: Entity + ?Sized>(entity: &E, drag_down: bool, pos: BlockPos) {
    let velocity = entity.velocity();
    let y = if drag_down {
        (velocity.y - BUBBLE_COLUMN_DOWN_ACCELERATION).max(BUBBLE_COLUMN_ABOVE_DOWN_MIN_SPEED)
    } else {
        (velocity.y + BUBBLE_COLUMN_ABOVE_UP_ACCELERATION).min(BUBBLE_COLUMN_ABOVE_UP_MAX_SPEED)
    };
    entity.set_velocity(DVec3::new(velocity.x, y, velocity.z));

    if let Some(world) = entity.level() {
        world.send_bubble_column_particles(pos);
    }
}

/// `Entity`'s [`EntityProps::on_inside_bubble_column`].
pub fn on_inside_bubble_column<E: Entity + ?Sized>(entity: &E, drag_down: bool) {
    let velocity = entity.velocity();
    let y = if drag_down {
        (velocity.y - BUBBLE_COLUMN_DOWN_ACCELERATION).max(BUBBLE_COLUMN_INSIDE_DOWN_MIN_SPEED)
    } else {
        (velocity.y + BUBBLE_COLUMN_INSIDE_UP_ACCELERATION).min(BUBBLE_COLUMN_INSIDE_UP_MAX_SPEED)
    };
    entity.set_velocity(DVec3::new(velocity.x, y, velocity.z));
    entity.reset_fall_distance();
}

/// `Entity`'s [`EntityProps::update_swimming`].
pub fn update_swimming<E: Entity + ?Sized>(entity: &E) {
    let Some(world) = entity.level() else {
        return;
    };

    let block_fluid = get_fluid_state(&world, entity.block_position());
    let swimming = select_swimming_state(
        entity.is_swimming(),
        SwimmingEnvironment {
            sprinting: entity
                .as_living_entity()
                .is_some_and(|living| living.is_sprinting()),
            passenger: entity.is_passenger(),
            in_water: entity.is_in_water(),
            under_water: entity.is_under_water(),
            block_fluid_is_water: block_fluid.is_water(),
        },
    );
    entity.set_shared_swimming(swimming);
}

/// `Entity`'s [`EntityProps::is_swimming`].
pub fn is_swimming<E: Entity + ?Sized>(entity: &E) -> bool {
    entity
        .synced_data()
        .is_some_and(EntitySyncedData::is_swimming)
}

/// `Entity`'s [`EntityProps::do_water_splash_effect`].
pub fn do_water_splash_effect<E: Entity + ?Sized>(entity: &E) {
    let (movement, volume_modifier) = if let Some(passenger) = entity.controlling_passenger() {
        (passenger.velocity(), 0.9)
    } else {
        (entity.velocity(), 0.2)
    };
    let speed =
        ((movement.x * movement.x * 0.2 + movement.y * movement.y + movement.z * movement.z * 0.2)
            .sqrt() as f32
            * volume_modifier)
            .min(1.0);
    let pitch = 1.0 + (rng().random::<f32>() - rng().random::<f32>()) * 0.4;
    if speed < 0.25 {
        entity.play_sound(entity.swim_splash_sound(), speed, pitch);
    } else {
        entity.play_sound(entity.swim_high_speed_splash_sound(), speed, pitch);
    }

    // VANILLA CLIENT-LOCAL: the bubble/splash particle loop uses `Level.addParticle`.

    entity.game_event(&vanilla_game_events::SPLASH);
}
