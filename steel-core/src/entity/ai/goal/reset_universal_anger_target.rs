//! Refreshes anger after a player hits a mob under universal anger.

use steel_registry::vanilla_game_rules::UNIVERSAL_ANGER;
use steel_utils::WorldAabb;

use super::selector::{Goal, GoalControls};
use super::target_goal::follow_distance;
use crate::entity::PathfinderMob;

const ALERT_RANGE_Y: f64 = 10.0;

pub(crate) struct ResetUniversalAngerTargetGoal {
    alert_others_of_same_type: bool,
    last_hurt_by_player_timestamp: i32,
}

impl ResetUniversalAngerTargetGoal {
    #[must_use]
    pub(crate) const fn new(alert_others_of_same_type: bool) -> Self {
        Self {
            alert_others_of_same_type,
            last_hurt_by_player_timestamp: 0,
        }
    }

    fn was_hurt_by_player(&self, mob: &dyn PathfinderMob) -> bool {
        let Some(attacker) = mob.last_hurt_by_mob() else {
            return false;
        };
        attacker.as_player().is_some()
            && mob.last_hurt_by_mob_timestamp() > self.last_hurt_by_player_timestamp
    }

    fn alert_nearby_mobs(mob: &dyn PathfinderMob) {
        let Some(world) = mob.level() else {
            return;
        };
        let follow_range = follow_distance(mob);
        let position = mob.position();
        let aabb = WorldAabb::new(
            position.x,
            position.y,
            position.z,
            position.x + 1.0,
            position.y + 1.0,
            position.z + 1.0,
        )
        .inflate_xyz(follow_range, ALERT_RANGE_Y, follow_range);

        for entity in world.get_entities_of_class_matching(&aabb, |entity| {
            entity.id() != mob.id()
                && !entity.is_spectator()
                && entity.entity_type() == mob.entity_type()
                && entity.as_neutral_mob().is_some()
        }) {
            if let Some(neutral) = entity.as_neutral_mob() {
                neutral.forget_current_target_and_refresh_universal_anger();
            }
        }
    }
}

impl Goal for ResetUniversalAngerTargetGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::TARGET
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };
        world.get_game_rule(&UNIVERSAL_ANGER) && self.was_hurt_by_player(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.last_hurt_by_player_timestamp = mob.last_hurt_by_mob_timestamp();
        if let Some(neutral) = mob.as_neutral_mob() {
            neutral.forget_current_target_and_refresh_universal_anger();
        }
        if self.alert_others_of_same_type {
            Self::alert_nearby_mobs(mob);
        }
    }
}
