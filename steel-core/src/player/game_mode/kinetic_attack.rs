//! Kinetic-weapon charge attacks (vanilla `KineticWeapon.damageEntities`).

// TODO: Run kinetic charges for mobs once they have an active item-use tick; vanilla
// scales mob attackers' speed thresholds by 0.2.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::data_components::components::{KineticWeapon, KineticWeaponCondition};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_attributes;
use steel_utils::TICKS_PER_SECOND;
use steel_utils::entity_events::EntityStatus;
use steel_utils::types::InteractionHand;

use crate::entity::{Entity, LivingEntity as _, PartEntity};
use crate::player::Player;

/// Kinetic conditions compare speeds in blocks per second; known speed is per tick.
const KNOWN_SPEED_TO_BLOCKS_PER_SECOND: f64 = TICKS_PER_SECOND as f64;
/// Players test kinetic conditions against their unscaled speed thresholds.
const PLAYER_ACTION_FACTOR: f64 = 1.0;

/// Returns an entity's velocity in blocks per second; non-player passengers move with
/// their root vehicle.
fn kinetic_motion(entity: &dyn Entity) -> DVec3 {
    let known_speed = if entity.as_player().is_none()
        && let Some(root_vehicle) = entity.root_vehicle()
    {
        root_vehicle.known_speed()
    } else {
        entity.known_speed()
    };
    known_speed * KNOWN_SPEED_TO_BLOCKS_PER_SECOND
}

impl Player {
    /// Stabs every entity along the look direction that this tick of an active
    /// kinetic-weapon use reaches, applying each action whose condition holds.
    ///
    /// `ticks_used` counts active-use ticks including the weapon's start delay.
    pub(in crate::player) fn kinetic_damage_entities(
        self: &Arc<Self>,
        kinetic_weapon: &KineticWeapon,
        weapon: &ItemStack,
        hand: InteractionHand,
        ticks_used: i32,
    ) {
        if ticks_used < kinetic_weapon.delay_ticks() {
            return;
        }
        let ticks_after_delay = ticks_used - kinetic_weapon.delay_ticks();

        let look = self.look_angle();
        let attacker_speed = look.dot(kinetic_motion(self.as_ref()));
        let base_damage =
            self.attributes()
                .lock()
                .required_base_value(vanilla_attributes::ATTACK_DAMAGE) as f32;
        let world = self.get_world();
        let game_time = world.game_time();
        let mut affected = false;

        let ray_look = self.calculate_view_vector(
            self.rotation().1,
            self.living_rotation_state().y_head_rot_o(),
        );

        for hit in self.piercing_hit_entities(weapon, &world, Some(ray_look)) {
            let parent = hit.as_part_entity().and_then(PartEntity::parent);
            let target = parent.unwrap_or(hit);
            if self.living_base.was_recently_stabbed(
                target.id(),
                kinetic_weapon.contact_cooldown_ticks(),
                game_time,
            ) {
                continue;
            }
            self.living_base
                .remember_stabbed_entity(target.id(), game_time);

            let target_speed = look.dot(kinetic_motion(target.as_ref()));
            let relative_speed = (attacker_speed - target_speed).max(0.0);
            let applies = |condition: Option<&KineticWeaponCondition>| {
                condition.is_some_and(|condition| {
                    condition.test(
                        ticks_after_delay,
                        attacker_speed,
                        relative_speed,
                        PLAYER_ACTION_FACTOR,
                    )
                })
            };
            let dismounts = applies(kinetic_weapon.dismount_conditions());
            let deals_knockback = applies(kinetic_weapon.knockback_conditions());
            let deals_damage = applies(kinetic_weapon.damage_conditions());
            if !dismounts && !deals_knockback && !deals_damage {
                continue;
            }

            let speed_damage =
                (relative_speed * f64::from(kinetic_weapon.damage_multiplier())).floor() as i32;
            affected |= self.stab_attack(
                hand,
                &target,
                base_damage + speed_damage as f32,
                deals_damage,
                deals_knockback,
                dismounts,
            );
        }

        if affected {
            self.broadcast_entity_event(EntityStatus::KineticHit);
            // TODO: Trigger CriteriaTriggers.SPEAR_MOBS_TRIGGER once the foundation for advancements exists.
        }
    }
}
