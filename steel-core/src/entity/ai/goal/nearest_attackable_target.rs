use glam::DVec3;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use super::target_goal::{TargetGoalBase, follow_distance};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{LivingEntity, PathfinderMob, SharedEntity};
use crate::world::World;

const DEFAULT_RANDOM_INTERVAL: i32 = 10;

enum TargetSearch {
    Players,
    Entities,
}

pub(crate) struct NearestAttackableTargetGoal {
    target_goal: TargetGoalBase,
    random_interval: i32,
    target: Option<SharedEntity>,
    target_conditions: TargetingConditions,
    search: TargetSearch,
}

impl NearestAttackableTargetGoal {
    #[must_use]
    pub(crate) fn new(
        must_see: bool,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::new_with_interval(DEFAULT_RANDOM_INTERVAL, must_see, false, selector)
    }

    #[must_use]
    pub(crate) fn new_with_interval(
        random_interval: i32,
        must_see: bool,
        must_reach: bool,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::new_with_options(
            random_interval,
            must_see,
            must_reach,
            selector,
            TargetSearch::Entities,
        )
    }

    #[must_use]
    pub(crate) fn new_for_players(
        must_see: bool,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::new_for_players_with_interval(DEFAULT_RANDOM_INTERVAL, must_see, false, selector)
    }

    #[must_use]
    pub(crate) fn new_for_players_with_interval(
        random_interval: i32,
        must_see: bool,
        must_reach: bool,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::new_with_options(
            random_interval,
            must_see,
            must_reach,
            selector,
            TargetSearch::Players,
        )
    }

    fn new_with_options(
        random_interval: i32,
        must_see: bool,
        must_reach: bool,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
        search: TargetSearch,
    ) -> Self {
        Self {
            target_goal: TargetGoalBase::new(must_see, must_reach),
            random_interval: reduced_tick_delay(random_interval),
            target: None,
            target_conditions: TargetingConditions::for_combat().selector(selector),
            search,
        }
    }

    fn find_target(&mut self, mob: &dyn PathfinderMob) {
        let Some(world) = mob.level() else {
            self.target = None;
            return;
        };

        let follow = follow_distance(mob);
        let position = mob.position();
        let origin = DVec3::new(position.x, mob.get_eye_y(), position.z);
        let target_conditions = self.target_conditions.clone().range(follow);
        let level = world.as_ref();

        self.target = match self.search {
            TargetSearch::Players => level
                .nearest_player(origin, follow, |player| {
                    target_conditions.test(level, Some(mob), player)
                })
                .map(|player| -> SharedEntity { player }),
            TargetSearch::Entities => {
                let search_box = mob.bounding_box().inflate_xyz(follow, follow, follow);
                level.nearest_entity_in_aabb_matching(&search_box, origin, |entity| {
                    entity
                        .as_living_entity()
                        .is_some_and(|target| target_conditions.test(level, Some(mob), target))
                })
            }
        };
    }

    pub(crate) fn set_target(&mut self, target: Option<SharedEntity>) {
        self.target = target;
    }
}

impl Goal for NearestAttackableTargetGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::TARGET
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if self.random_interval > 0 && rand::random_range(0..self.random_interval) != 0 {
            return false;
        }

        self.find_target(mob);
        self.target.is_some()
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.target_goal.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let _ = mob.set_target(self.target.as_ref());
        self.target_goal.start();
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.target_goal.stop(mob);
        self.target = None;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::entities::{CowEntity, PigEntity};
    use crate::entity::{Entity, Mob};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn animal_fixture(
        name: &'static str,
    ) -> (Arc<World>, Arc<PigEntity>, Arc<PigEntity>, Arc<CowEntity>) {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world(name);
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let hunter = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(&world),
        ));
        let nearer_pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(9.0, 65.0, 8.0),
            Arc::downgrade(&world),
        ));
        let farther_cow = Arc::new(CowEntity::new(
            &vanilla_entities::COW,
            3,
            DVec3::new(10.0, 65.0, 8.0),
            Arc::downgrade(&world),
        ));

        for entity in [
            Arc::clone(&hunter) as SharedEntity,
            Arc::clone(&nearer_pig) as SharedEntity,
            Arc::clone(&farther_cow) as SharedEntity,
        ] {
            world
                .try_add_entity(entity)
                .expect("test entity should attach to the loaded chunk");
        }

        (world, hunter, nearer_pig, farther_cow)
    }

    #[test]
    fn selects_nearest_living_entity_matching_selector() {
        let (_world, hunter, nearer_pig, _farther_cow) =
            animal_fixture("nearest_attackable_selector");
        let mut goal =
            NearestAttackableTargetGoal::new_with_interval(0, false, false, |target, _| {
                target.as_animal().is_some()
            });

        assert!(goal.can_use(hunter.as_ref()));
        goal.start(hunter.as_ref());

        let Some(target) = hunter.target() else {
            panic!("selector goal should assign a target");
        };
        assert_eq!(target.uuid(), nearer_pig.uuid());
    }

    #[test]
    fn selector_can_reject_all_candidates() {
        let (_world, hunter, _nearer_pig, _farther_cow) =
            animal_fixture("nearest_attackable_selector_rejects");
        let mut goal = NearestAttackableTargetGoal::new(false, |_, _| false);

        assert!(!goal.can_use(hunter.as_ref()));
    }
}
