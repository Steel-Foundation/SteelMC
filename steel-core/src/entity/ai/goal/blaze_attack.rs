use glam::DVec3;
use steel_registry::{level_events, vanilla_attributes};
use steel_utils::{Downcast, random::triangle_random};

use crate::entity::{
    Entity, EntityAnchor, LivingEntity, Mob, PathfinderMob,
    ai::goal::{
        GoalControl,
        selector::{Goal, GoalControls},
    },
    entities::BlazeEntity,
};

pub struct BlazeAttackGoal {
    attack_step: i32,
    attack_time: i32,
    last_seen: i32,
}

impl BlazeAttackGoal {
    pub fn new() -> Self {
        Self {
            attack_step: 0,
            attack_time: 0,
            last_seen: 0,
        }
    }

    fn blaze<'a>(&self, mob: &'a dyn PathfinderMob) -> &'a BlazeEntity {
        mob.downcast_ref::<BlazeEntity>()
            .expect("This goal isn't supposed to be used by mobs other than the Blaze")
    }

    fn get_follow_distance(&self, mob: &dyn PathfinderMob) -> f64 {
        mob.attributes()
            .lock()
            .get_value(vanilla_attributes::FOLLOW_RANGE)
            .unwrap_or_default()
    }
}

impl Goal for BlazeAttackGoal {
    fn controls(&self) -> GoalControls {
        let mut controls = GoalControls::MOVE;
        controls.insert(GoalControl::Look);
        controls
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let target = mob.target();
        if let Some(target) = target.as_deref().and_then(Entity::as_living_entity)
            && LivingEntity::is_alive(target)
            && Mob::can_attack(mob, target)
        {
            return true;
        }
        false
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.attack_step = 0;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.blaze(mob).set_charged(false);
        self.last_seen = 0;
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.attack_time -= 1;
        let blaze = self.blaze(mob);
        if let Some(target) = blaze.target() {
            let has_line_of_sight = blaze.has_line_of_sight(&*target);
            if has_line_of_sight {
                self.last_seen = 0
            } else {
                self.last_seen += 1;
            }

            let distance = blaze.distance_to_sqr(&*target);
            if distance < 4.0 {
                if !has_line_of_sight {
                    return;
                }

                if let Some(world) = blaze.level()
                    && self.attack_time <= 0
                {
                    self.attack_time = 20;
                    let _ = blaze.do_hurt_target(&world, &target);
                }

                blaze.set_wanted_position(target.position(), 1.0);
            } else if distance < self.get_follow_distance(mob) * self.get_follow_distance(mob)
                && has_line_of_sight
            {
                if self.attack_time <= 0 {
                    self.attack_step += 1;
                    if self.attack_step == 1 {
                        self.attack_time = 60;
                        blaze.set_charged(true);
                    } else if self.attack_step <= 4 {
                        self.attack_time = 6;
                    } else {
                        self.attack_time = 100;
                        self.attack_step = 0;
                        blaze.set_charged(false);
                    }

                    if self.attack_step > 1 {
                        if let Some(level) = blaze.level()
                            && !blaze.is_silent()
                        {
                            level.level_event(
                                level_events::SOUND_BLAZE_FIREBALL,
                                blaze.block_position(),
                                0,
                                None,
                            );
                        }

                        let xd = target.position().x - blaze.position().x;
                        let yd = (target.position().y + target.bounding_box().height() * 0.5)
                            - (blaze.position().y + blaze.bounding_box().height() * 0.5);
                        let zd = target.position().z - blaze.position().z;
                        let sqr_dist = distance.sqrt().sqrt() * 0.5;
                        let direction = DVec3::new(
                            triangle_random(xd, 2.297 * sqr_dist),
                            yd,
                            triangle_random(zd, 2.297 * sqr_dist),
                        );
                        // TODO: Spawn small fireball
                        // SmallFireball entity = new SmallFireball(this.blaze.level(), this.blaze, direction.normalize());
                        // entity.setPos(entity.getX(), this.blaze.getY(0.5) + 0.5, entity.getZ());
                        // this.blaze.level().addFreshEntity(entity);
                    }
                }

                blaze.look_at_entity(EntityAnchor::Eyes, &*target, EntityAnchor::Eyes);
            } else if self.last_seen < 5 {
                blaze.set_wanted_position(target.position(), 1.0);
            }
        }
    }
}
