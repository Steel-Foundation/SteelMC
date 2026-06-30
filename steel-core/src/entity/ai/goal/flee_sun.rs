use glam::DVec3;
use steel_utils::BlockPos;

use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

const HIDE_POS_ATTEMPTS: usize = 10;

pub struct FleeSunGoal {
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
}

impl FleeSunGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            wanted_position: None,
            speed_modifier,
        }
    }

    fn set_wanted_pos(&mut self, mob: &dyn PathfinderMob, level: &World) -> bool {
        let Some(pos) = get_hide_pos(mob, level) else {
            return false;
        };

        self.wanted_position = Some(pos);
        true
    }
}

impl Goal for FleeSunGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if mob.target().is_some() {
            return false;
        }

        let Some(level) = mob.level() else {
            return false;
        };

        if !level.is_bright_outside() {
            return false;
        }
        if !mob.is_on_fire() {
            return false;
        }
        if !level.can_see_sky(mob.block_position()) {
            return false;
        }
        if mob.has_item_in_slot(EquipmentSlot::Head) {
            return false;
        }

        self.set_wanted_pos(mob, &level)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.mob_base().navigation().lock().is_done()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let Some(wanted_position) = self.wanted_position else {
            return;
        };

        mob.move_to_pos(wanted_position, self.speed_modifier);
    }
}

fn get_hide_pos(mob: &dyn PathfinderMob, level: &World) -> Option<DVec3> {
    get_hide_pos_with(
        mob,
        |pos| level.can_see_sky(pos),
        |pos| mob.get_walk_target_value(pos),
    )
}

fn get_hide_pos_with(
    mob: &dyn PathfinderMob,
    mut can_see_sky: impl FnMut(BlockPos) -> bool,
    mut walk_target_value: impl FnMut(BlockPos) -> f32,
) -> Option<DVec3> {
    let pos = mob.block_position();

    for _ in 0..HIDE_POS_ATTEMPTS {
        let random_pos = {
            pos.offset(
                rand::random_range(0..20) - 10,
                rand::random_range(0..6) - 3,
                rand::random_range(0..20) - 10,
            )
        };
        if !can_see_sky(random_pos) && walk_target_value(random_pos) < 0.0 {
            let (x, y, z) = random_pos.get_bottom_center();
            return Some(DVec3::new(x, y, z));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::entities::PigEntity;

    fn pig() -> PigEntity {
        PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new())
    }

    #[test]
    fn flee_sun_goal_uses_move_control() {
        let goal = FleeSunGoal::new(1.0);

        assert_eq!(goal.controls(), GoalControls::MOVE);
    }

    #[test]
    fn flee_sun_goal_requires_world() {
        init_test_registry();
        let mut goal = FleeSunGoal::new(1.0);

        assert!(!goal.can_use(&pig()));
    }

    #[test]
    fn flee_sun_hide_pos_accepts_sheltered_negative_walk_target() {
        init_test_registry();
        let mob = pig();
        let mut visited = Vec::new();

        let pos = get_hide_pos_with(
            &mob,
            |pos| {
                visited.push(pos);
                false
            },
            |_| -1.0,
        );

        assert_eq!(visited.len(), 1);
        let (x, y, z) = visited[0].get_bottom_center();
        assert_eq!(pos, Some(DVec3::new(x, y, z)));
    }

    #[test]
    fn flee_sun_hide_pos_rejects_sky_exposed_positions() {
        init_test_registry();
        let mob = pig();
        let mut visited = Vec::new();

        let pos = get_hide_pos_with(
            &mob,
            |pos| {
                visited.push(pos);
                true
            },
            |_| -1.0,
        );

        assert!(pos.is_none());
        assert_eq!(visited.len(), HIDE_POS_ATTEMPTS);
    }

    #[test]
    fn flee_sun_hide_pos_rejects_non_negative_walk_targets() {
        init_test_registry();
        let mob = pig();

        assert!(get_hide_pos_with(&mob, |_| false, |_| 0.0).is_none());
    }
}
