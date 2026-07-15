use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_damage_type_tags;
use steel_utils::BlockPos;

use super::random_pos::default_random_pos;
use super::selector::{Goal, GoalControls};
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, BlockStateBehaviorExt as _};
use crate::entity::PathfinderMob;
use crate::fluid::FluidStateExt as _;

const WATER_CHECK_DISTANCE_VERTICAL: i32 = 1;

pub struct PanicGoal {
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
    is_running: bool,
}

impl PanicGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            wanted_position: None,
            speed_modifier,
            is_running: false,
        }
    }

    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.is_running
    }

    fn should_panic(mob: &dyn PathfinderMob) -> bool {
        mob.last_damage_source()
            .is_some_and(|source| source.is(&vanilla_damage_type_tags::DamageTypeTag::PANIC_CAUSES))
    }

    fn find_random_position(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(position) = default_random_pos(mob, 5, 4) else {
            return false;
        };

        self.wanted_position = Some(position);
        true
    }
}

impl Goal for PanicGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn is_panic_goal(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if !Self::should_panic(mob) {
            return false;
        }

        if mob.is_on_fire()
            && let Some(water_pos) = look_for_water(mob, 5)
        {
            self.wanted_position = Some(block_pos_corner(water_pos));
            return true;
        }

        self.find_random_position(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.mob_base().navigation().lock().is_done()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(wanted_position) = self.wanted_position {
            mob.move_to_pos(wanted_position, self.speed_modifier);
        }
        self.is_running = true;
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.is_running = false;
    }
}

fn look_for_water(mob: &dyn PathfinderMob, xz_dist: i32) -> Option<BlockPos> {
    let world = mob.level()?;
    let mob_position = mob.block_position();
    let block_state = world.get_block_state(mob_position);
    let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
    if !behavior
        .get_collision_shape(
            block_state,
            world.as_ref(),
            mob_position,
            BlockCollisionContext::empty(),
        )
        .is_empty()
    {
        return None;
    }

    mob_position.find_closest_match(xz_dist, WATER_CHECK_DISTANCE_VERTICAL, |pos| {
        world.get_block_state(pos).get_fluid_state().is_water()
    })
}

fn block_pos_corner(pos: BlockPos) -> DVec3 {
    DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()))
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::{
        test_support::init_test_registry, vanilla_damage_types, vanilla_entities,
    };

    use super::*;
    use crate::entity::LivingEntity;
    use crate::entity::damage::DamageSource;
    use crate::entity::entities::PigEntity;
    use crate::test_support::test_world;

    #[test]
    fn panic_goal_uses_move_control() {
        let goal = PanicGoal::new(1.25);

        assert_eq!(goal.controls(), GoalControls::MOVE);
        assert!(!goal.is_running());
    }

    #[test]
    fn panic_goal_uses_vanilla_panic_damage_tag() {
        init_test_registry();
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!PanicGoal::should_panic(&pig));

        assert!(pig.hurt_server(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            1.0
        ));
        assert!(!PanicGoal::should_panic(&pig));

        assert!(pig.hurt_server(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
            2.0
        ));
        assert!(PanicGoal::should_panic(&pig));
    }
}
