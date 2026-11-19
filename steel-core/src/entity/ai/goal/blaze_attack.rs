//! Vanilla `Blaze.BlazeAttackGoal`: melee at close range, charged fireball
//! volleys at range.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::{level_events, vanilla_attributes, vanilla_entities};
use steel_utils::Downcast as _;

use super::selector::{Goal, GoalControls};
use crate::entity::entities::{BlazeEntity, SmallFireballEntity};
use crate::entity::projectile::triangle_random;
use crate::entity::{
    Entity, INITIAL_ACCELERATION_POWER, LivingEntity, Mob, PathfinderMob, Projectile, SharedEntity,
    next_entity_id,
};

/// Squared distance below which the blaze attacks with melee.
const MELEE_DISTANCE_SQR: f64 = 4.0;
const MELEE_ATTACK_INTERVAL: i32 = 20;
const CHARGE_DELAY: i32 = 60;
const SHOT_DELAY: i32 = 6;
const COOLDOWN_DELAY: i32 = 100;
/// Vanilla fires on `attack_step` 2 through 4, then cools down at 5.
const SHOT_STEP_LIMIT: i32 = 4;
/// Vanilla's fireball direction spread scale.
const SHOT_SPREAD_SCALE: f64 = 2.297;
const LOST_SIGHT_MOVE_TICKS: i32 = 5;
const MOVE_SPEED: f64 = 1.0;
const LOOK_SPEED: f32 = 10.0;

pub(crate) struct BlazeAttackGoal {
    attack_step: i32,
    attack_time: i32,
    last_seen: i32,
}

impl BlazeAttackGoal {
    pub(crate) const fn new() -> Self {
        Self {
            attack_step: 0,
            attack_time: 0,
            last_seen: 0,
        }
    }

    fn follow_distance(mob: &dyn PathfinderMob) -> f64 {
        mob.attributes()
            .lock()
            .get_value(vanilla_attributes::FOLLOW_RANGE)
            .unwrap_or(0.0)
    }

    fn spawn_fireball(mob: &dyn PathfinderMob, entity: &SharedEntity, target: &SharedEntity) {
        let Some(world) = mob.level() else {
            return;
        };

        let mob_position = mob.position();
        let mob_height = mob.bounding_box().height();
        let target_height = target.bounding_box().height();
        let direction = DVec3::new(
            target.position().x - mob_position.x,
            (target.position().y + target_height * 0.5) - (mob_position.y + mob_height * 0.5),
            target.position().z - mob_position.z,
        );
        let distance_sqr = mob_position.distance_squared(target.position());
        let spread = SHOT_SPREAD_SCALE * (distance_sqr.sqrt().sqrt() * 0.5);

        let shot_direction = DVec3::new(
            triangle_random(direction.x, spread),
            direction.y,
            triangle_random(direction.z, spread),
        );

        let spawn_position = DVec3::new(
            mob_position.x,
            mob_position.y + mob_height * 0.5 + 0.5,
            mob_position.z,
        );
        let fireball = SmallFireballEntity::new(
            &vanilla_entities::SMALL_FIREBALL,
            next_entity_id(),
            spawn_position,
            Arc::downgrade(&world),
        );
        fireball.set_owner_entity(Some(entity));
        fireball.set_rotation(mob.rotation());
        fireball.set_velocity(shot_direction.normalize_or_zero() * INITIAL_ACCELERATION_POWER);
        fireball.mark_velocity_sync();

        if let Err(error) = world.try_add_entity(Arc::new(fireball) as SharedEntity) {
            log::debug!("failed to spawn blaze fireball: {error}");
        }
    }
}

impl Goal for BlazeAttackGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(target) = mob.target() else {
            return false;
        };
        let Some(target_living) = target.as_living_entity() else {
            return false;
        };
        LivingEntity::is_alive(target_living) && Mob::can_attack(mob, target_living)
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.attack_step = 0;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        set_charged(mob, false);
        self.last_seen = 0;
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, entity: &SharedEntity) {
        self.attack_time -= 1;
        let Some(target) = mob.target() else {
            return;
        };
        let target_position = target.position();

        let has_line_of_sight = mob.has_line_of_sight_cached(target.as_ref());
        if has_line_of_sight {
            self.last_seen = 0;
        } else {
            self.last_seen += 1;
        }

        let distance_sqr = mob.position().distance_squared(target_position);
        if distance_sqr < MELEE_DISTANCE_SQR {
            if !has_line_of_sight {
                return;
            }

            if self.attack_time <= 0 {
                self.attack_time = MELEE_ATTACK_INTERVAL;
                if let Some(world) = mob.level() {
                    let _ = mob.do_hurt_target(entity, &world, &target);
                }
            }

            mob.set_wanted_position(target_position, MOVE_SPEED);
        } else if distance_sqr < Self::follow_distance(mob).powi(2) && has_line_of_sight {
            if self.attack_time <= 0 {
                self.attack_step += 1;
                if self.attack_step == 1 {
                    self.attack_time = CHARGE_DELAY;
                    set_charged(mob, true);
                } else if self.attack_step <= SHOT_STEP_LIMIT {
                    self.attack_time = SHOT_DELAY;
                } else {
                    self.attack_time = COOLDOWN_DELAY;
                    self.attack_step = 0;
                    set_charged(mob, false);
                }

                if self.attack_step > 1 {
                    if !mob.is_silent()
                        && let Some(world) = mob.level()
                    {
                        world.level_event(
                            level_events::SOUND_BLAZE_FIREBALL,
                            mob.block_position(),
                            0,
                            None,
                        );
                    }
                    Self::spawn_fireball(mob, entity, &target);
                }
            }

            mob.mob_base().controls().lock().look_control.set_look_at(
                DVec3::new(target_position.x, target.get_eye_y(), target_position.z),
                LOOK_SPEED,
                LOOK_SPEED,
            );
        } else if self.last_seen < LOST_SIGHT_MOVE_TICKS {
            mob.set_wanted_position(target_position, MOVE_SPEED);
        }
    }
}

/// The charged flag lives in the blaze's synced data, so the goal reaches the
/// concrete entity through Steel's keyed downcasting.
fn set_charged(mob: &dyn PathfinderMob, charged: bool) {
    if let Some(blaze) = mob.downcast_ref::<BlazeEntity>() {
        blaze.set_charged(charged);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::entity::entities::PigEntity;
    use crate::entity::{Entity, Mob, SharedEntity};
    use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;

    fn fixture(name: &'static str) -> (TestWorld, Arc<BlazeEntity>, SharedEntity, SharedEntity) {
        init_vanilla_registry();
        let fixture = fresh_test_world(name);
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let blaze = Arc::new(BlazeEntity::new(
            &vanilla_entities::BLAZE,
            next_entity_id(),
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(world),
        ));
        let target = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(13.0, 65.0, 8.0),
            Arc::downgrade(world),
        ));

        let blaze_entity: SharedEntity = Arc::<BlazeEntity>::clone(&blaze);
        let target_entity: SharedEntity = Arc::<PigEntity>::clone(&target);
        world
            .try_add_entity(Arc::clone(&blaze_entity))
            .expect("blaze should attach to the loaded chunk");
        world
            .try_add_entity(Arc::clone(&target_entity))
            .expect("target should attach to the loaded chunk");
        assert!(blaze.set_target(Some(&target_entity)));

        (fixture, blaze, blaze_entity, target_entity)
    }

    fn fireballs(world: &World, blaze: &BlazeEntity) -> Vec<SharedEntity> {
        let search_box = blaze.bounding_box().inflate_xyz(4.0, 4.0, 4.0);
        world
            .get_entities_in_aabb(&search_box)
            .into_iter()
            .filter(|entity| entity.entity_type() == &vanilla_entities::SMALL_FIREBALL)
            .collect()
    }

    #[test]
    fn charged_volley_fires_three_owned_fireballs_then_cools_down() {
        let (fixture, blaze, blaze_entity, _target) = fixture("blaze_attack_volley");
        let mut goal = BlazeAttackGoal::new();
        assert!(goal.can_use(blaze.as_ref()));
        goal.start(blaze.as_ref());

        goal.tick(blaze.as_ref(), &blaze_entity);
        assert!(blaze.is_charged());
        assert_eq!(goal.attack_step, 1);
        assert_eq!(goal.attack_time, CHARGE_DELAY);
        assert!(fireballs(&fixture.world, &blaze).is_empty());

        for _ in 0..CHARGE_DELAY - 1 {
            goal.tick(blaze.as_ref(), &blaze_entity);
        }
        assert!(
            fireballs(&fixture.world, &blaze).is_empty(),
            "the blaze must not fire on the tick the charge timer reaches zero"
        );

        goal.tick(blaze.as_ref(), &blaze_entity);
        assert_eq!(goal.attack_step, 2);
        assert_eq!(goal.attack_time, SHOT_DELAY);
        let first_shot = fireballs(&fixture.world, &blaze);
        assert_eq!(first_shot.len(), 1);
        let projectile = first_shot[0]
            .as_projectile()
            .expect("small fireball should be a projectile");
        assert_eq!(projectile.owner_uuid(), Some(blaze.uuid()));
        assert!(
            (projectile.velocity().length() - INITIAL_ACCELERATION_POWER).abs() < f64::EPSILON,
            "vanilla normalizes the shot direction before scaling it by 0.1"
        );

        for expected_shots in 2..=3 {
            for _ in 0..SHOT_DELAY {
                goal.tick(blaze.as_ref(), &blaze_entity);
            }
            assert_eq!(fireballs(&fixture.world, &blaze).len(), expected_shots);
        }

        for _ in 0..SHOT_DELAY {
            goal.tick(blaze.as_ref(), &blaze_entity);
        }
        assert_eq!(goal.attack_step, 0);
        assert_eq!(goal.attack_time, COOLDOWN_DELAY);
        assert!(!blaze.is_charged());
        assert_eq!(
            fireballs(&fixture.world, &blaze).len(),
            3,
            "the cooldown tick must not spawn an extra fireball"
        );
    }
}
