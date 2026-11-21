//! Vanilla `BreakDoorGoal`: a mob attacks a wooden door until it breaks.

use steel_registry::level_events;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::types::{Difficulty, InteractionHand};

use super::door_interact::DoorInteractGoal;
use super::selector::{Goal, GoalControls};
use crate::entity::{PathfinderMob, SharedEntity};

const DOOR_BREAK_TIME: i32 = 240;
const DOOR_BREAK_SOUND_INTERVAL: i32 = 20;
const DOOR_BREAK_PROGRESS_STEPS: f32 = 10.0;
const DOOR_REACH_DISTANCE: f64 = 2.0;

/// A goal that breaks the wooden door blocking the mob's path.
pub struct BreakDoorGoal {
    door_interact: DoorInteractGoal,
    valid_difficulties: fn(Difficulty) -> bool,
    break_time: i32,
    last_break_progress: i32,
}

impl BreakDoorGoal {
    #[must_use]
    pub(crate) const fn new(valid_difficulties: fn(Difficulty) -> bool) -> Self {
        Self {
            door_interact: DoorInteractGoal::new(),
            valid_difficulties,
            break_time: 0,
            last_break_progress: -1,
        }
    }

    fn is_valid_difficulty(&self, difficulty: Difficulty) -> bool {
        (self.valid_difficulties)(difficulty)
    }
}

impl Goal for BreakDoorGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if !self.door_interact.can_use(mob) {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };
        world.get_game_rule(&MOB_GRIEFING)
            && self.is_valid_difficulty(world.difficulty())
            && !self.door_interact.is_open(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };

        self.break_time <= DOOR_BREAK_TIME
            && !self.door_interact.is_open(mob)
            && self
                .door_interact
                .door_pos()
                .closer_to_center_than(mob.position(), DOOR_REACH_DISTANCE)
            && self.is_valid_difficulty(world.difficulty())
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.door_interact.start(mob);
        self.break_time = 0;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        let Some(world) = mob.level() else {
            return;
        };
        world.broadcast_block_destruction(mob.id(), self.door_interact.door_pos(), -1);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, _entity: &SharedEntity) {
        self.door_interact.tick(mob);

        let Some(world) = mob.level() else {
            return;
        };
        let door_pos = self.door_interact.door_pos();

        if rand::random_range(0..DOOR_BREAK_SOUND_INTERVAL) == 0 {
            world.level_event(level_events::SOUND_ZOMBIE_WOODEN_DOOR, door_pos, 0, None);
            if !mob.living_swing_state().swinging() {
                mob.swing_for_attack(InteractionHand::MainHand);
            }
        }

        self.break_time += 1;
        let progress =
            (self.break_time as f32 / DOOR_BREAK_TIME as f32 * DOOR_BREAK_PROGRESS_STEPS) as i32;
        if progress != self.last_break_progress {
            world.broadcast_block_destruction(mob.id(), door_pos, progress);
            self.last_break_progress = progress;
        }

        if self.break_time == DOOR_BREAK_TIME && self.is_valid_difficulty(world.difficulty()) {
            world.remove_block(door_pos, false);
            world.level_event(level_events::SOUND_ZOMBIE_DOOR_CRASH, door_pos, 0, None);
            let state = world.get_block_state(door_pos);
            world.level_event(
                level_events::PARTICLES_AND_SOUND_DESTROY_BLOCK,
                door_pos,
                level_events::encode_block_state_data(u32::from(state.0)),
                None,
            );
        }
    }
}
