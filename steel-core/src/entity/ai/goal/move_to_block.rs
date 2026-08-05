use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;
use crate::world::LevelReader;
use glam::DVec3;
use steel_utils::BlockPos;

const GIVE_UP_TICKS: i32 = 1200;
const STAY_TICKS: i32 = 1200;
const INTERVAL_TICKS: i32 = 200;
const DEFAULT_VERTICAL_SEARCH_RANGE: i32 = 1;
const DEFAULT_RECALCULATE_PATH_INTERVAL: i32 = 40;
const DEFAULT_ACCEPTED_DISTANCE: f64 = 1.0;

type ValidTargetPredicate = Box<dyn Fn(&dyn LevelReader, BlockPos) -> bool + Send + Sync>;
type MoveToTarget = Box<dyn Fn(BlockPos) -> BlockPos + Send + Sync>;

pub struct MoveToBlockGoal {
    block_pos: BlockPos,
    target_predicate: ValidTargetPredicate,
    move_to_target: MoveToTarget,
    speed_modifier: f64,
    next_start_tick: i32,
    try_ticks: i32,
    max_stay_ticks: i32,
    reached_target: bool,
    search_range: i32,
    vertical_search_range: i32,
    vertical_search_start: i32,
    accepted_distance: f64,
    recalculate_path_interval: i32,
}

impl MoveToBlockGoal {
    #[must_use]
    pub(crate) fn new(
        speed_modifier: f64,
        search_range: i32,
        target_predicate: impl Fn(&dyn LevelReader, BlockPos) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::with_vertical_search_range(
            speed_modifier,
            search_range,
            DEFAULT_VERTICAL_SEARCH_RANGE,
            target_predicate,
        )
    }

    #[must_use]
    pub(crate) fn with_vertical_search_range(
        speed_modifier: f64,
        search_range: i32,
        vertical_search_range: i32,
        target_predicate: impl Fn(&dyn LevelReader, BlockPos) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            block_pos: BlockPos::ZERO,
            target_predicate: Box::new(target_predicate),
            move_to_target: Box::new(|pos| pos.above()),
            speed_modifier,
            next_start_tick: 0,
            try_ticks: 0,
            max_stay_ticks: 0,
            reached_target: false,
            search_range,
            vertical_search_range,
            vertical_search_start: 0,
            accepted_distance: DEFAULT_ACCEPTED_DISTANCE,
            recalculate_path_interval: DEFAULT_RECALCULATE_PATH_INTERVAL,
        }
    }

    #[must_use]
    pub(crate) const fn with_vertical_search_start(mut self, vertical_search_start: i32) -> Self {
        self.vertical_search_start = vertical_search_start;
        self
    }

    #[must_use]
    pub(crate) const fn with_accepted_distance(mut self, accepted_distance: f64) -> Self {
        self.accepted_distance = accepted_distance;
        self
    }

    #[must_use]
    pub(crate) const fn with_recalculate_path_interval(
        mut self,
        recalculate_path_interval: i32,
    ) -> Self {
        self.recalculate_path_interval = recalculate_path_interval;
        self
    }

    #[must_use]
    pub(crate) fn with_move_to_target(
        mut self,
        move_to_target: impl Fn(BlockPos) -> BlockPos + Send + Sync + 'static,
    ) -> Self {
        self.move_to_target = Box::new(move_to_target);
        self
    }

    #[must_use]
    pub(crate) const fn block_pos(&self) -> BlockPos {
        self.block_pos
    }

    #[must_use]
    pub(crate) const fn is_reached_target(&self) -> bool {
        self.reached_target
    }

    fn next_start_tick(_mob: &dyn PathfinderMob) -> i32 {
        reduced_tick_delay(INTERVAL_TICKS + rand::random_range(0..200))
    }

    fn move_mob_to_block(&self, mob: &dyn PathfinderMob) {
        mob.move_to_pos(
            block_center_with_y(self.block_pos, self.block_pos.y() + 1),
            self.speed_modifier,
        );
    }

    fn move_to_target(&self) -> BlockPos {
        (self.move_to_target)(self.block_pos)
    }

    const fn should_recalculate_path(&self) -> bool {
        self.try_ticks % self.recalculate_path_interval == 0
    }

    fn find_nearest_block(&mut self, mob: &dyn PathfinderMob, level: &dyn LevelReader) -> bool {
        let Some(block_pos) = find_nearest_block_from(
            mob.block_position(),
            self.search_range,
            self.vertical_search_start,
            self.vertical_search_range,
            |pos| mob.is_within_home_pos(pos) && (self.target_predicate)(level, pos),
        ) else {
            return false;
        };

        self.block_pos = block_pos;
        true
    }
}

impl Goal for MoveToBlockGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::JUMP
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if self.next_start_tick > 0 {
            self.next_start_tick -= 1;
            return false;
        }

        self.next_start_tick = Self::next_start_tick(mob);
        let Some(world) = mob.level() else {
            return false;
        };
        self.find_nearest_block(mob, world.as_ref())
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };

        self.try_ticks >= -self.max_stay_ticks
            && self.try_ticks <= GIVE_UP_TICKS
            && (self.target_predicate)(world.as_ref(), self.block_pos)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.move_mob_to_block(mob);
        self.try_ticks = 0;
        let inner_bound = rand::random_range(0..STAY_TICKS) + STAY_TICKS;
        self.max_stay_ticks = rand::random_range(0..inner_bound) + STAY_TICKS;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let move_to_target = self.move_to_target();
        if block_pos_closer_to_center_than(move_to_target, mob.position(), self.accepted_distance) {
            self.reached_target = true;
            self.try_ticks -= 1;
        } else {
            self.reached_target = false;
            self.try_ticks += 1;
            if self.should_recalculate_path() {
                mob.move_to_pos(
                    block_center_with_y(move_to_target, move_to_target.y()),
                    self.speed_modifier,
                );
            }
        }
    }
}

fn block_center_with_y(pos: BlockPos, y: i32) -> DVec3 {
    DVec3::new(
        f64::from(pos.x()) + 0.5,
        f64::from(y),
        f64::from(pos.z()) + 0.5,
    )
}

fn block_pos_closer_to_center_than(pos: BlockPos, position: DVec3, distance: f64) -> bool {
    let (x, y, z) = pos.get_center();
    DVec3::new(x, y, z).distance_squared(position) < distance * distance
}

fn find_nearest_block_from(
    mob_pos: BlockPos,
    search_range: i32,
    vertical_search_start: i32,
    vertical_search_range: i32,
    mut is_valid_target: impl FnMut(BlockPos) -> bool,
) -> Option<BlockPos> {
    let mut y = vertical_search_start;
    while y <= vertical_search_range {
        for r in 0..search_range {
            let mut x = 0;
            while x <= r {
                let mut z = if x < r && x > -r { r } else { 0 };
                while z <= r {
                    let pos = mob_pos.offset(x, y - 1, z);
                    if is_valid_target(pos) {
                        return Some(pos);
                    }
                    z = next_mirrored_offset(z);
                }
                x = next_mirrored_offset(x);
            }
        }
        y = next_mirrored_offset(y);
    }

    None
}

const fn next_mirrored_offset(value: i32) -> i32 {
    if value > 0 { -value } else { 1 - value }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::entities::PigEntity;

    #[test]
    fn move_to_block_goal_uses_move_and_jump_controls() {
        let goal = MoveToBlockGoal::new(1.0, 8, |_, _| false);

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::JUMP);
        assert!(goal.requires_update_every_tick());
    }

    #[test]
    fn move_to_block_goal_requires_world_after_start_delay() {
        init_test_registry();
        let mut goal = MoveToBlockGoal::new(1.0, 8, |_, _| true);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn move_to_block_goal_counts_down_next_start_tick_before_world_lookup() {
        init_test_registry();
        let mut goal = MoveToBlockGoal::new(1.0, 8, |_, _| true);
        goal.next_start_tick = 2;
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!goal.can_use(&mob));

        assert_eq!(goal.next_start_tick, 1);
    }

    #[test]
    fn move_to_block_search_uses_vanilla_below_first_order() {
        let mob_pos = BlockPos::new(10, 64, 10);
        let expected = BlockPos::new(10, 63, 10);
        let later = BlockPos::new(11, 63, 10);

        let found =
            find_nearest_block_from(mob_pos, 8, 0, 1, |pos| pos == later || pos == expected);

        assert_eq!(found, Some(expected));
    }

    #[test]
    fn move_to_block_search_respects_vertical_search_start() {
        let mob_pos = BlockPos::new(10, 64, 10);
        let expected = BlockPos::new(10, 61, 10);
        let skipped = BlockPos::new(10, 63, 10);

        let found =
            find_nearest_block_from(mob_pos, 8, -2, 6, |pos| pos == skipped || pos == expected);

        assert_eq!(found, Some(expected));
    }

    #[test]
    fn move_to_block_goal_tracks_reached_target() {
        init_test_registry();
        let mut goal = MoveToBlockGoal::new(1.0, 8, |_, _| false);
        goal.block_pos = BlockPos::new(0, -1, 0);
        let mob = PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::new(0.5, 0.5, 0.5),
            Weak::new(),
        );

        goal.tick(&mob);

        assert!(goal.is_reached_target());
        assert_eq!(goal.try_ticks, -1);
    }
}
