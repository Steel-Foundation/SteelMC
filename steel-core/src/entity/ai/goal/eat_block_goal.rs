//! Vanilla `EatBlockGoal`: a mob stops moving and chews for a fixed duration before
//! eating an edible block and running `Mob.ate`.

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::level_events;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::entity_events::EntityStatus;
use steel_utils::types::UpdateFlags;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::{AgeableMob, Mob, PathfinderMob};
use crate::world::LevelAccessor;

/// Constant mirroring vanilla `EatBlockGoal.EAT_ANIMATION_TICKS`, the full animation
/// length before the goal's tick reaches the block-eating step.
const EAT_ANIMATION_TICKS: i32 = 40;
/// Constant mirroring vanilla `EatBlockGoal`'s check point (`adjustedTickDelay(4)`).
const EAT_BLOCK_TICK: i32 = 4;
/// Vanilla `EatBlockGoal.canUse` rolls `nextInt(adjustedTickDelay(baby ? 50 : 1000))`;
/// babies check far more often so they regrow wool quickly.
const BABY_EAT_CHECK_TICKS: i32 = 50;
const ADULT_EAT_CHECK_TICKS: i32 = 1000;

/// A goal where a mob stands still and chews before eating an edible block.
///
/// Ports vanilla 26.2 `EatBlockGoal`. The eat animation only turns grass into dirt
/// when the `mobGriefing` game rule is enabled, matching vanilla.
pub struct EatBlockGoal {
    eat_animation_tick: i32,
}

impl EatBlockGoal {
    /// Creates the goal with no running animation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            eat_animation_tick: 0,
        }
    }

    /// Returns the current eat-animation tick (vanilla `EatBlockGoal.getEatAnimationTick`).
    #[must_use]
    pub const fn get_eat_animation_tick(&self) -> i32 {
        self.eat_animation_tick
    }
}

impl Goal for EatBlockGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK | GoalControls::JUMP
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let interval = if mob.as_animal().is_some_and(AgeableMob::is_baby) {
            BABY_EAT_CHECK_TICKS
        } else {
            ADULT_EAT_CHECK_TICKS
        };
        let adjusted = reduced_tick_delay(interval);
        if rand::random_range(0..adjusted) != 0 {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };
        let pos = mob.block_position();
        let state_at_feet = world.get_block_state(pos);
        state_at_feet
            .get_block()
            .has_tag(&BlockTag::EDIBLE_FOR_SHEEP)
            || world.get_block_state(pos.below()).get_block() == &vanilla_blocks::GRASS_BLOCK
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        self.eat_animation_tick > 0
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.eat_animation_tick = reduced_tick_delay(EAT_ANIMATION_TICKS);
        mob.broadcast_entity_event(EntityStatus::EatGrass);
        mob.mob_base().navigation().lock().stop();
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.eat_animation_tick = 0;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.eat_animation_tick = (self.eat_animation_tick - 1).max(0);
        if self.eat_animation_tick != reduced_tick_delay(EAT_BLOCK_TICK) {
            return;
        }

        let Some(world) = mob.level() else {
            return;
        };
        let pos = mob.block_position();
        let below = pos.below();
        let at_feet = world.get_block_state(pos);
        if at_feet.get_block().has_tag(&BlockTag::EDIBLE_FOR_SHEEP) {
            if world.get_game_rule(&MOB_GRIEFING) {
                world.destroy_block(pos, false);
            }
            Mob::ate(mob);
        } else if world.get_block_state(below).get_block() == &vanilla_blocks::GRASS_BLOCK {
            if world.get_game_rule(&MOB_GRIEFING) {
                world.level_event(
                    level_events::PARTICLES_DESTROY_BLOCK,
                    below,
                    level_events::encode_block_state_data(u32::from(
                        vanilla_blocks::GRASS_BLOCK.default_state().0,
                    )),
                    None,
                );
                world.set_block_state(
                    below,
                    vanilla_blocks::DIRT.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                );
            }
            Mob::ate(mob);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::init_vanilla_registry;
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::SharedEntity;
    use crate::entity::entities::{PigEntity, SheepEntity};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;
    use steel_utils::Downcast as _;

    #[test]
    fn eat_block_goal_mirrors_fixed_animation_duration() {
        use steel_registry::vanilla_entities;

        init_vanilla_registry();
        let mut goal = EatBlockGoal::new();
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        goal.start(&mob);

        assert_eq!(goal.get_eat_animation_tick(), 20);
    }

    fn sheep_on_grass_world(name: &'static str) -> (Arc<World>, SharedEntity) {
        use steel_registry::vanilla_blocks;
        use steel_registry::vanilla_entities;
        use steel_utils::ChunkPos;

        init_behaviors();
        let world = fresh_test_world(name);
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let sheep = SheepEntity::new(
            &vanilla_entities::SHEEP,
            1,
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(&world),
        );
        sheep.set_sheared(true);
        let shared: SharedEntity = Arc::new(sheep);
        world
            .try_add_entity(Arc::clone(&shared))
            .expect("sheep should attach to the loaded test chunk");
        world.set_block_state(
            shared.block_position().below(),
            vanilla_blocks::GRASS_BLOCK.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        );
        (world, shared)
    }

    #[test]
    fn eat_block_goal_turns_grass_into_dirt_and_runs_mob_ate() {
        use steel_registry::vanilla_blocks;

        init_vanilla_registry();
        let (world, shared) = sheep_on_grass_world("eat_grass");
        let mob = shared
            .as_pathfinder_mob()
            .expect("sheep should be a pathfinder mob");

        let mut goal = EatBlockGoal::new();
        goal.start(mob);
        for _ in 0..18 {
            goal.tick(mob);
        }

        let sheep = shared
            .downcast_ref::<SheepEntity>()
            .expect("shared entity should be a sheep");
        assert!(!sheep.is_sheared(), "eating grass should run Mob.ate");
        assert_eq!(
            world
                .get_block_state(shared.block_position().below())
                .get_block(),
            &vanilla_blocks::DIRT,
            "eating grass should turn it into dirt with mobGriefing enabled"
        );
    }

    #[test]
    fn eat_block_goal_does_not_grief_grass_with_mob_griefing_disabled() {
        use steel_registry::vanilla_blocks;

        init_vanilla_registry();
        let (world, shared) = sheep_on_grass_world("eat_grass_no_grief");
        world.set_game_rule(&MOB_GRIEFING, false);
        let mob = shared
            .as_pathfinder_mob()
            .expect("sheep should be a pathfinder mob");

        let mut goal = EatBlockGoal::new();
        goal.start(mob);
        for _ in 0..18 {
            goal.tick(mob);
        }

        let sheep = shared
            .downcast_ref::<SheepEntity>()
            .expect("shared entity should be a sheep");
        assert!(
            !sheep.is_sheared(),
            "Mob.ate should still run without mobGriefing"
        );
        assert_eq!(
            world
                .get_block_state(shared.block_position().below())
                .get_block(),
            &vanilla_blocks::GRASS_BLOCK,
            "grass should survive without mobGriefing"
        );
    }
}
