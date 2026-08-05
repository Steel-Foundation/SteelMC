use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::BlockPos;

use super::selector::{Goal, GoalControls};
use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::PathfinderMob;
use crate::entity::ai::path::Path;

const DOOR_REACH_DISTANCE_SQR: f64 = 2.25;
const PATH_NODE_SCAN_AHEAD: usize = 2;

pub(super) struct DoorInteractGoal {
    door_pos: BlockPos,
    has_door: bool,
    passed: bool,
    door_open_dir_x: f32,
    door_open_dir_z: f32,
}

impl DoorInteractGoal {
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            door_pos: BlockPos::ZERO,
            has_door: false,
            passed: false,
            door_open_dir_x: 0.0,
            door_open_dir_z: 0.0,
        }
    }

    #[must_use]
    pub(super) const fn door_pos(&self) -> BlockPos {
        self.door_pos
    }

    #[must_use]
    pub(super) const fn has_door(&self) -> bool {
        self.has_door
    }

    pub(super) fn is_open(&mut self, mob: &dyn PathfinderMob) -> bool {
        if !self.has_door {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };
        let state = world.get_block_state(self.door_pos);
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        if !behavior.is_wooden_door(state) {
            self.has_door = false;
            return false;
        }

        state.get_value(&BlockStateProperties::OPEN)
    }

    pub(super) fn set_open(&self, mob: &dyn PathfinderMob, open: bool) {
        if !self.has_door {
            return;
        }

        let Some(world) = mob.level() else {
            return;
        };
        let state = world.get_block_state(self.door_pos);
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        behavior.set_door_open(
            state,
            &world,
            self.door_pos,
            Some(mob.as_entity_event_source()),
            open,
        );
    }

    pub(super) fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if !mob.horizontal_collision() {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };

        let navigation = mob.mob_base().navigation().lock();
        let Some(path) = navigation.path() else {
            return false;
        };
        if path.is_done() {
            return false;
        }

        self.find_door_in_path(mob, path, |pos| {
            let state = world.get_block_state(pos);
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .is_wooden_door(state)
        })
    }

    pub(super) const fn can_continue_to_use(&self) -> bool {
        !self.passed
    }

    pub(super) fn start(&mut self, mob: &dyn PathfinderMob) {
        self.passed = false;
        self.door_open_dir_x = (f64::from(self.door_pos.x()) + 0.5 - mob.position().x) as f32;
        self.door_open_dir_z = (f64::from(self.door_pos.z()) + 0.5 - mob.position().z) as f32;
    }

    pub(super) fn tick(&mut self, mob: &dyn PathfinderMob) {
        let new_door_dir_x = (f64::from(self.door_pos.x()) + 0.5 - mob.position().x) as f32;
        let new_door_dir_z = (f64::from(self.door_pos.z()) + 0.5 - mob.position().z) as f32;
        let dot = self
            .door_open_dir_x
            .mul_add(new_door_dir_x, self.door_open_dir_z * new_door_dir_z);
        if dot < 0.0 {
            self.passed = true;
        }
    }

    fn find_door_in_path(
        &mut self,
        mob: &dyn PathfinderMob,
        path: &Path,
        mut is_wooden_door: impl FnMut(BlockPos) -> bool,
    ) -> bool {
        let limit = path
            .next_node_index()
            .saturating_add(PATH_NODE_SCAN_AHEAD)
            .min(path.node_count());
        for index in 0..limit {
            let Some(node) = path.node(index) else {
                continue;
            };
            let door_pos = BlockPos::new(node.x, node.y + 1, node.z);
            if distance_to_door_sqr(mob, door_pos) > DOOR_REACH_DISTANCE_SQR {
                continue;
            }

            self.door_pos = door_pos;
            self.has_door = is_wooden_door(door_pos);
            if self.has_door {
                return true;
            }
        }

        self.door_pos = mob.block_position().above();
        self.has_door = is_wooden_door(self.door_pos);
        self.has_door
    }
}

impl Goal for DoorInteractGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        Self::can_use(self, mob)
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        Self::can_continue_to_use(self)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        Self::start(self, mob);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        Self::tick(self, mob);
    }
}

fn distance_to_door_sqr(mob: &dyn PathfinderMob, door_pos: BlockPos) -> f64 {
    let position = mob.position();
    let dx = f64::from(door_pos.x()) - position.x;
    let dz = f64::from(door_pos.z()) - position.z;
    dx.mul_add(dx, dz * dz)
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::ai::node::Node;
    use crate::entity::entities::PigEntity;
    use crate::entity::{Entity, EntityGroundContact, EntityMovementFlags};

    fn pig(position: DVec3) -> PigEntity {
        PigEntity::new(&vanilla_entities::PIG, 1, position, Weak::new())
    }

    fn set_horizontal_collision(mob: &PigEntity) {
        mob.base().set_movement_flags(
            EntityMovementFlags::new().with_horizontal_collision(true),
            EntityGroundContact::airborne(),
        );
    }

    #[test]
    fn door_interact_goal_claims_no_controls_like_vanilla() {
        let goal = DoorInteractGoal::new();

        assert_eq!(goal.controls(), GoalControls::EMPTY);
        assert!(goal.requires_update_every_tick());
    }

    #[test]
    fn door_interact_goal_requires_horizontal_collision() {
        init_test_registry();
        let mut goal = DoorInteractGoal::new();
        let mob = pig(DVec3::ZERO);

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn door_interact_path_scan_uses_current_node_and_one_ahead() {
        init_test_registry();
        let mut goal = DoorInteractGoal::new();
        let mob = pig(DVec3::ZERO);
        let path = Path::new(
            vec![Node::new(8, 0, 0), Node::new(1, 0, 0), Node::new(0, 0, 1)],
            BlockPos::new(0, 0, 1),
            true,
        );

        assert!(goal.find_door_in_path(&mob, &path, |pos| pos == BlockPos::new(1, 1, 0)));
        assert_eq!(goal.door_pos(), BlockPos::new(1, 1, 0));
        assert!(goal.has_door());
    }

    #[test]
    fn door_interact_path_scan_uses_above_mob_as_fallback() {
        init_test_registry();
        let mut goal = DoorInteractGoal::new();
        let mob = pig(DVec3::ZERO);
        let path = Path::new(vec![Node::new(8, 0, 0)], BlockPos::new(8, 0, 0), true);

        assert!(goal.find_door_in_path(&mob, &path, |pos| pos == BlockPos::new(0, 1, 0)));
        assert_eq!(goal.door_pos(), BlockPos::new(0, 1, 0));
    }

    #[test]
    fn door_interact_tick_marks_passed_after_crossing_door_plane() {
        init_test_registry();
        let mut goal = DoorInteractGoal::new();
        let mob = pig(DVec3::ZERO);
        goal.door_pos = BlockPos::new(1, 1, 0);
        goal.start(&mob);

        mob.base().set_position_local(DVec3::new(2.0, 0.0, 0.0));
        goal.tick(&mob);

        assert!(!goal.can_continue_to_use());
    }

    #[test]
    fn door_interact_can_use_requires_world_after_collision() {
        init_test_registry();
        let mut goal = DoorInteractGoal::new();
        let mob = pig(DVec3::ZERO);
        set_horizontal_collision(&mob);

        assert!(!goal.can_use(&mob));
    }
}
