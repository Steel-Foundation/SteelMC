use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::CSetEntityMotion;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{level_events, sound_events, vanilla_damage_types};

use crate::behavior::ItemBehavior;
use crate::enchantment_helper::{self, EnchantmentDamageContext};
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, LivingEntity};
use crate::inventory::equipment::EquipmentSlot;

/// Vanilla mace item combat behavior.
#[item_behavior]
pub struct MaceItem;

impl MaceItem {
    const SMASH_ATTACK_FALL_THRESHOLD: f64 = 1.5;
    const SMASH_ATTACK_HEAVY_THRESHOLD: f64 = 5.0;
    const SMASH_ATTACK_KNOCKBACK_RADIUS: f64 = 3.5;
    const SMASH_ATTACK_KNOCKBACK_POWER: f64 = 0.7;
    const SMASH_ATTACK_PARTICLE_DATA: i32 = 750;

    fn can_smash_attack(attacker: &dyn LivingEntity) -> bool {
        attacker.fall_distance() > Self::SMASH_ATTACK_FALL_THRESHOLD && !attacker.is_fall_flying()
    }

    fn calculate_impact_position(attacker: &dyn LivingEntity) -> DVec3 {
        if let Some(impact_pos) = attacker.current_impulse_impact_pos()
            && impact_pos.y <= attacker.position().y
        {
            return impact_pos;
        }

        attacker.position()
    }

    fn smash_sound(target: &dyn LivingEntity, attacker: &dyn LivingEntity) -> SoundEventRef {
        if !target.on_ground() {
            return &sound_events::ITEM_MACE_SMASH_AIR;
        }

        if attacker.fall_distance() > Self::SMASH_ATTACK_HEAVY_THRESHOLD {
            &sound_events::ITEM_MACE_SMASH_GROUND_HEAVY
        } else {
            &sound_events::ITEM_MACE_SMASH_GROUND
        }
    }

    fn knockback_power(
        attacker: &dyn LivingEntity,
        nearby: &dyn LivingEntity,
        direction: DVec3,
    ) -> f64 {
        let heavy_multiplier = if attacker.fall_distance() > Self::SMASH_ATTACK_HEAVY_THRESHOLD {
            2.0
        } else {
            1.0
        };
        (Self::SMASH_ATTACK_KNOCKBACK_RADIUS - direction.length())
            * Self::SMASH_ATTACK_KNOCKBACK_POWER
            * heavy_multiplier
            * (1.0 - nearby.knockback_resistance())
    }

    fn should_knockback(
        attacker: &dyn LivingEntity,
        target: &dyn LivingEntity,
        nearby: &dyn LivingEntity,
    ) -> bool {
        if nearby.is_spectator() || nearby.id() == attacker.id() || nearby.id() == target.id() {
            return false;
        }
        if attacker.is_allied_to(nearby) || nearby.is_tame_owned_by(target) {
            return false;
        }
        if nearby.is_marker_armor_stand() {
            return false;
        }
        if nearby
            .as_player()
            .is_some_and(|player| player.has_infinite_materials() && player.is_flying())
        {
            return false;
        }

        target.position().distance_squared(nearby.position())
            <= Self::SMASH_ATTACK_KNOCKBACK_RADIUS * Self::SMASH_ATTACK_KNOCKBACK_RADIUS
    }

    fn apply_knockback(attacker: &dyn LivingEntity, target: &dyn LivingEntity) {
        let Some(world) = attacker.level() else {
            return;
        };

        let event_pos = target
            .on_pos(1.0e-5)
            .unwrap_or_else(|| target.block_position());
        world.level_event(
            level_events::PARTICLES_SMASH_ATTACK,
            event_pos,
            Self::SMASH_ATTACK_PARTICLE_DATA,
            None,
        );

        let search_box = target
            .bounding_box()
            .inflate(Self::SMASH_ATTACK_KNOCKBACK_RADIUS);
        for nearby in world.get_entities_in_aabb_matching(&search_box, |entity| {
            let Some(nearby) = entity.as_living_entity() else {
                return false;
            };
            Self::should_knockback(attacker, target, nearby)
        }) {
            let Some(nearby_living) = nearby.as_living_entity() else {
                continue;
            };
            let direction = nearby_living.position() - target.position();
            let knockback_power = Self::knockback_power(attacker, nearby_living, direction);
            if knockback_power <= 0.0 {
                continue;
            }

            let horizontal = if direction.length_squared() > 0.0 {
                direction.normalize() * knockback_power
            } else {
                DVec3::ZERO
            };
            nearby_living.push_impulse(DVec3::new(horizontal.x, 0.7, horizontal.z));
            if let Some(player) = nearby_living.as_player() {
                player.send_packet(CSetEntityMotion::new(
                    nearby_living.id(),
                    nearby_living.velocity(),
                ));
            }
        }
    }
}

impl ItemBehavior for MaceItem {
    fn get_item_damage_source(&self, attacker: &dyn LivingEntity) -> Option<DamageSource> {
        Self::can_smash_attack(attacker).then(|| {
            DamageSource::environment(&vanilla_damage_types::MACE_SMASH)
                .with_causing_entity(attacker.id())
                .with_direct_entity(attacker.id())
                .with_source_position(attacker.position())
        })
    }

    fn get_attack_damage_bonus(
        &self,
        attacker: &dyn LivingEntity,
        victim: &dyn Entity,
        _base_damage: f32,
        damage_source: &DamageSource,
    ) -> f32 {
        if !Self::can_smash_attack(attacker) {
            return 0.0;
        }

        let fall_distance = attacker.fall_distance();
        let damage = if fall_distance <= 3.0 {
            4.0 * fall_distance
        } else if fall_distance <= 8.0 {
            12.0 + 2.0 * (fall_distance - 3.0)
        } else {
            22.0 + fall_distance - 8.0
        };
        let context = EnchantmentDamageContext::new(
            victim.entity_type(),
            Some(attacker.entity_type()),
            Some(attacker.entity_type()),
            damage_source,
        );
        let mut damage_per_fallen_block = 0.0;
        attacker.with_equipment_slot(EquipmentSlot::MainHand, &mut |item| {
            damage_per_fallen_block =
                enchantment_helper::modify_smash_damage_per_fallen_block(item, &context, 0.0);
        });
        (damage + f64::from(damage_per_fallen_block) * fall_distance) as f32
    }

    fn hurt_enemy(
        &self,
        _stack: &mut ItemStack,
        target: &dyn LivingEntity,
        attacker: &dyn LivingEntity,
    ) {
        if !Self::can_smash_attack(attacker) {
            return;
        }

        let velocity = attacker.velocity();
        attacker.set_velocity(DVec3::new(velocity.x, 0.01, velocity.z));
        attacker.set_ignore_fall_damage_from_current_impulse(
            true,
            Self::calculate_impact_position(attacker),
        );
        if let Some(player) = attacker.as_player() {
            player.send_packet(CSetEntityMotion::new(attacker.id(), attacker.velocity()));
        }

        if let Some(world) = attacker.level() {
            world.play_sound_at(
                Self::smash_sound(target, attacker),
                attacker.sound_source(),
                attacker.position(),
                1.0,
                1.0,
                None,
            );
        }
        Self::apply_knockback(attacker, target);
    }

    fn post_hurt_enemy(
        &self,
        _stack: &mut ItemStack,
        _target: &dyn LivingEntity,
        attacker: &dyn LivingEntity,
    ) {
        if Self::can_smash_attack(attacker) {
            attacker.reset_fall_distance();
        }
    }
}
