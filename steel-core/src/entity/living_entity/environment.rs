//! Air supply, drowning, in-wall damage and freezing.

use super::*;

impl Living<'_> {
    /// Returns whether vanilla effects keep this entity from drowning.
    #[must_use]
    pub fn has_water_breathing(&self) -> bool {
        self.has_mob_effect(vanilla_mob_effects::WATER_BREATHING)
            || self.has_mob_effect(vanilla_mob_effects::CONDUIT_POWER)
            || self.has_mob_effect(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS)
    }

    /// Returns whether active vanilla effects refill this entity's air supply.
    #[must_use]
    pub fn should_effects_refill_air_supply(&self) -> bool {
        !self.has_mob_effect(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS)
            || self.has_mob_effect(vanilla_mob_effects::WATER_BREATHING)
            || self.has_mob_effect(vanilla_mob_effects::CONDUIT_POWER)
    }

    /// Returns whether this entity's type can breathe underwater.
    #[must_use]
    pub fn can_breathe_underwater(&self) -> bool {
        self.entity_type().flags.can_breathe_underwater
    }

    /// Returns whether this entity can lose air and take drowning damage.
    #[must_use]
    pub fn can_drown_in_water(&self) -> bool {
        if self.can_breathe_underwater() || self.has_water_breathing() {
            return false;
        }

        !self
            .as_player()
            .is_some_and(|player| player.abilities.lock().invulnerable)
    }

    /// Returns whether the entity's eye block is a bubble column.
    #[must_use]
    pub fn is_eye_in_bubble_column(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        world
            .get_block_state(BlockPos::new(
                self.position().x.floor() as i32,
                self.eye_y().floor() as i32,
                self.position().z.floor() as i32,
            ))
            .get_block()
            == &vanilla_blocks::BUBBLE_COLUMN
    }

    /// Decreases air supply while drowning, occasionally skipped when the oxygen bonus effect applies.
    #[must_use]
    pub fn decrease_air_supply(&self, current_supply: i32) -> i32 {
        let oxygen_bonus = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::OXYGEN_BONUS)
            .unwrap_or(0.0);
        if oxygen_bonus > 0.0 && rand::random::<f64>() >= 1.0 / (oxygen_bonus + 1.0) {
            current_supply
        } else {
            current_supply - 1
        }
    }

    /// Refills air supply while out of water, capped at the entity's maximum.
    #[must_use]
    pub fn increase_air_supply(&self, current_supply: i32) -> i32 {
        (current_supply + 4).min(self.max_air_supply())
    }

    /// Returns whether air supply has dropped low enough to start dealing drowning damage.
    #[must_use]
    pub fn should_take_drowning_damage(&self) -> bool {
        self.air_supply() <= -20
    }

    /// Ticks vanilla living air-supply and drowning behavior from `baseTick`.
    pub fn tick_living_air_supply(&self) {
        if !self.is_alive() {
            return;
        }

        let eye_in_water = self.is_eye_in_water() && !self.is_eye_in_bubble_column();
        if eye_in_water {
            if self.can_drown_in_water() {
                self.set_air_supply(self.decrease_air_supply(self.air_supply()));
                if self.should_take_drowning_damage() {
                    self.set_air_supply(0);
                    self.broadcast_entity_event(EntityStatus::DrownParticles);
                    if let Some(world) = self.level() {
                        self.hurt(
                            &world,
                            &DamageSource::environment(&vanilla_damage_types::DROWN),
                            2.0,
                        );
                    }
                }
            } else if self.air_supply() < self.max_air_supply()
                && self.should_effects_refill_air_supply()
            {
                self.set_air_supply(self.increase_air_supply(self.air_supply()));
            }

            if self
                .vehicle()
                .is_some_and(|vehicle| vehicle.dismounts_underwater())
            {
                self.stop_riding();
            }
            return;
        }

        if self.air_supply() < self.max_air_supply() {
            self.set_air_supply(self.increase_air_supply(self.air_supply()));
        }
    }

    /// Applies vanilla living in-wall damage from `baseTick`.
    pub fn tick_in_wall_damage(&self) {
        if !self.is_alive() || !self.is_in_wall() {
            return;
        }

        if let Some(world) = self.level() {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::IN_WALL),
                1.0,
            );
        }
    }

    /// Applies vanilla living environmental damage in `LivingEntity.baseTick` order.
    pub fn tick_living_environmental_damage(&self) {
        if !self.is_alive() {
            return;
        }

        if self.is_in_wall() {
            self.tick_in_wall_damage();
        } else if self.as_player().is_some()
            && let Some(world) = self.level()
        {
            let border = world.world_border_snapshot();
            let position = self.position();
            if let Some(damage) =
                border.outside_damage_amount(position.x, position.z, self.bounding_box())
            {
                self.hurt(
                    &world,
                    &DamageSource::environment(&vanilla_damage_types::OUTSIDE_BORDER),
                    damage,
                );
            }
        }

        self.tick_living_air_supply();
    }

    /// Returns vanilla `LivingEntity.canFreeze()` after concrete entity exemptions.
    ///
    /// Vanilla keeps the entity-type freeze immunity on `Entity` and the equipment
    /// immunity on `LivingEntity`. Steel keeps this helper separate so concrete
    /// `Entity::can_freeze` implementations can delegate without downcasting.
    #[must_use]
    /// `LivingEntity.canFreeze` around `super`: freeze-immune armor blocks it.
    pub(super) fn living_can_freeze(&self) -> bool {
        for slot in EquipmentSlot::ALL {
            if !slot.is_armor() {
                continue;
            }
            let mut is_freeze_immune = false;
            self.with_equipment_slot(slot, &mut |item_stack| {
                is_freeze_immune = REGISTRY
                    .items
                    .is_in_tag(item_stack.item(), &ItemTag::FREEZE_IMMUNE_WEARABLES);
            });

            if is_freeze_immune {
                return false;
            }
        }

        entity::can_freeze(self.entity())
    }

    /// Removes the powder snow speed penalty modifier.
    pub fn remove_frost(&self) {
        self.attributes().lock().remove_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            &SPEED_MODIFIER_POWDER_SNOW_ID,
        );
    }

    /// Applies the powder snow speed penalty modifier if the entity is frozen and can freeze.
    pub fn try_add_frost(&self) {
        if !self.is_on_non_air_block_for_frost() || self.ticks_frozen() <= 0 {
            return;
        }

        self.attributes().lock().add_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            AttributeModifier {
                id: SPEED_MODIFIER_POWDER_SNOW_ID,
                amount: f64::from(-0.05_f32 * self.percent_frozen()),
                operation: AttributeModifierOperation::AddValue,
            },
            false,
        );
    }

    /// Ticks freeze-related state each AI step: cools `ticks_frozen` when not
    /// in powder snow (or unable to freeze), then refreshes the powder-snow
    /// speed penalty.
    pub fn tick_freezing(&self) {
        if !self.is_in_powder_snow() || !self.can_freeze() {
            self.set_ticks_frozen((self.ticks_frozen() - 2).max(0));
        }

        self.remove_frost();
        self.try_add_frost();
        if self.tick_count() % 40 == 0
            && self.is_fully_frozen()
            && self.can_freeze()
            && let Some(world) = self.level()
        {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::FREEZE),
                1.0,
            );
        }
    }
}

/// `LivingEntity`'s [`LivingProps::is_affected_by_fluids`].
///
/// Returns whether this entity is affected by fluid physics; true by default.
#[must_use]
pub const fn is_affected_by_fluids<E: Parts>(_entity: &E) -> bool {
    true
}

/// `LivingEntity`'s [`LivingProps::can_stand_on_fluid`].
///
/// Returns whether this entity can stand on the given fluid's surface; false by default.
#[must_use]
pub const fn can_stand_on_fluid<E: Parts>(_entity: &E, _fluid_state: FluidState) -> bool {
    false
}

/// `LivingEntity`'s [`LivingProps::is_on_non_air_block_for_frost`].
///
/// Returns whether the block this entity is standing on is non-air,
/// required for the powder-snow speed penalty to apply.
#[must_use]
pub fn is_on_non_air_block_for_frost<E: Parts>(entity: &E) -> bool {
    let living = &entity.living();
    let Some(world) = living.level() else {
        return false;
    };
    let Some(pos) = living.on_pos_legacy() else {
        return false;
    };

    world.get_block_state(pos).get_block() != &vanilla_blocks::AIR
}
