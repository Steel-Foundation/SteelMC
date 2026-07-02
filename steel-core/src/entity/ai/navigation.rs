//! Path navigation state shell.

use glam::DVec3;
use steel_math::fast_floor;
use steel_registry::REGISTRY;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::BlockPos;

use crate::entity::ai::path::{Path, PathType, PathTypeCache, PathfindingContext};
use crate::entity::ai::pathfinder::{PathFinder, PathRequest};
use crate::entity::ai::walk::{WalkNodeCollision, WalkNodeEvaluator};
use crate::world::LevelReader;

const DIRECT_TARGET_REACHED_DISTANCE_SQR: f64 = 2.500_000_3e-7;
const DEFAULT_REQUIRED_PATH_LENGTH: f32 = 16.0;
const MAX_TIME_RECOMPUTE: i64 = 20;
const MAX_VISITED_NODES_SCALE: f32 = 16.0;
const STUCK_CHECK_INTERVAL: i32 = 100;
const STUCK_THRESHOLD_DISTANCE_FACTOR: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavigationPathRequest<'a> {
    pub mob_position: BlockPos,
    pub targets: &'a [BlockPos],
    pub max_path_length: f32,
    pub reach_range: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavigationTickContext {
    pub mob_position: DVec3,
    pub mob_bounding_box_width: f64,
    pub mob_speed: f32,
    pub game_time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationRecomputeRequest {
    pub target_pos: BlockPos,
    pub reach_range: i32,
    pub game_time: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathNavigation {
    path: Option<Path>,
    path_finder: PathFinder,
    path_type_cache: PathTypeCache,
    direct_target: Option<DVec3>,
    target_pos: Option<BlockPos>,
    speed_modifier: f64,
    max_visited_nodes_multiplier: f32,
    required_path_length: f32,
    reach_range: i32,
    tick: i32,
    last_stuck_check: i32,
    last_stuck_check_pos: DVec3,
    timeout_cached_node: BlockPos,
    timeout_timer: i64,
    last_timeout_check: i64,
    timeout_limit: f64,
    has_delayed_recomputation: bool,
    time_last_recompute: i64,
    stuck: bool,
    done: bool,
    can_float: bool,
    can_open_doors: bool,
    can_walk_over_fences: bool,
    avoid_sun: bool,
    can_path_to_targets_below_surface: bool,
}

impl PathNavigation {
    #[must_use]
    pub fn new() -> Self {
        Self {
            path: None,
            path_finder: PathFinder::new(0),
            path_type_cache: PathTypeCache::new(),
            direct_target: None,
            target_pos: None,
            speed_modifier: 0.0,
            max_visited_nodes_multiplier: 1.0,
            required_path_length: DEFAULT_REQUIRED_PATH_LENGTH,
            reach_range: 0,
            tick: 0,
            last_stuck_check: 0,
            last_stuck_check_pos: DVec3::ZERO,
            timeout_cached_node: BlockPos::ZERO,
            timeout_timer: 0,
            last_timeout_check: 0,
            timeout_limit: 0.0,
            has_delayed_recomputation: false,
            time_last_recompute: 0,
            stuck: false,
            done: true,
            can_float: false,
            can_open_doors: false,
            can_walk_over_fences: false,
            avoid_sun: false,
            can_path_to_targets_below_surface: false,
        }
    }

    #[must_use]
    pub const fn path(&self) -> Option<&Path> {
        self.path.as_ref()
    }

    #[must_use]
    pub const fn target_pos(&self) -> Option<BlockPos> {
        self.target_pos
    }

    #[must_use]
    pub const fn speed_modifier(&self) -> f64 {
        self.speed_modifier
    }

    pub const fn set_speed_modifier(&mut self, speed_modifier: f64) {
        self.speed_modifier = speed_modifier;
    }

    #[must_use]
    pub const fn reach_range(&self) -> i32 {
        self.reach_range
    }

    #[must_use]
    pub const fn required_path_length(&self) -> f32 {
        self.required_path_length
    }

    #[must_use]
    pub const fn max_visited_nodes_multiplier(&self) -> f32 {
        self.max_visited_nodes_multiplier
    }

    pub fn set_required_path_length(&mut self, required_path_length: f32, follow_range: f64) {
        self.required_path_length = required_path_length;
        self.update_pathfinder_max_visited_nodes(follow_range);
    }

    pub const fn reset_max_visited_nodes_multiplier(&mut self) {
        self.max_visited_nodes_multiplier = 1.0;
    }

    pub const fn set_max_visited_nodes_multiplier(&mut self, max_visited_nodes_multiplier: f32) {
        self.max_visited_nodes_multiplier = max_visited_nodes_multiplier;
    }

    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.done
    }

    #[must_use]
    pub const fn tick_count(&self) -> i32 {
        self.tick
    }

    #[must_use]
    pub const fn is_stuck(&self) -> bool {
        self.stuck
    }

    #[must_use]
    pub const fn has_delayed_recomputation(&self) -> bool {
        self.has_delayed_recomputation
    }

    #[must_use]
    pub const fn can_float(&self) -> bool {
        self.can_float
    }

    pub const fn set_can_float(&mut self, can_float: bool) {
        self.can_float = can_float;
    }

    #[must_use]
    pub const fn can_open_doors(&self) -> bool {
        self.can_open_doors
    }

    pub const fn set_can_open_doors(&mut self, can_open_doors: bool) {
        self.can_open_doors = can_open_doors;
    }

    #[must_use]
    pub const fn can_walk_over_fences(&self) -> bool {
        self.can_walk_over_fences
    }

    pub const fn set_can_walk_over_fences(&mut self, can_walk_over_fences: bool) {
        self.can_walk_over_fences = can_walk_over_fences;
    }

    #[must_use]
    pub const fn avoid_sun(&self) -> bool {
        self.avoid_sun
    }

    pub const fn set_avoid_sun(&mut self, avoid_sun: bool) {
        self.avoid_sun = avoid_sun;
    }

    #[must_use]
    pub const fn can_path_to_targets_below_surface(&self) -> bool {
        self.can_path_to_targets_below_surface
    }

    pub const fn set_can_path_to_targets_below_surface(
        &mut self,
        can_path_to_targets_below_surface: bool,
    ) {
        self.can_path_to_targets_below_surface = can_path_to_targets_below_surface;
    }

    pub const fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn invalidate_path_type(&mut self, pos: BlockPos) {
        self.path_type_cache.invalidate(pos);
    }

    pub fn stop(&mut self) {
        self.path = None;
        self.done = true;
    }

    #[must_use]
    pub const fn max_path_length(&self, follow_range: f64) -> f32 {
        (follow_range as f32).max(self.required_path_length)
    }

    pub fn update_pathfinder_max_visited_nodes(&mut self, follow_range: f64) {
        let max_visited_nodes = fast_floor(f64::from(
            self.max_path_length(follow_range) * MAX_VISITED_NODES_SCALE,
        ));
        self.path_finder.set_max_visited_nodes(max_visited_nodes);
    }

    pub fn create_path(
        &mut self,
        evaluator: &mut WalkNodeEvaluator,
        level: &dyn LevelReader,
        collision: &mut impl WalkNodeCollision,
        request: NavigationPathRequest<'_>,
    ) -> Option<Path> {
        if let Some(path) = self.reusable_current_path(request.targets) {
            // Vanilla returns the current `Path` instance here. Steel's
            // navigation API returns owned paths, so reuse copies the path
            // state instead of running the pathfinder again.
            return Some(path.clone());
        }

        let mut context =
            PathfindingContext::with_cache(level, request.mob_position, &mut self.path_type_cache);
        let path = self.path_finder.find_path(
            evaluator,
            &mut context,
            collision,
            PathRequest {
                targets: request.targets,
                max_path_length: request.max_path_length,
                reach_range: request.reach_range,
                max_visited_nodes_multiplier: self.max_visited_nodes_multiplier,
            },
        )?;
        self.target_pos = Some(path.target());
        self.reach_range = request.reach_range;
        self.reset_stuck_timeout();
        Some(path)
    }

    fn reusable_current_path(&self, targets: &[BlockPos]) -> Option<&Path> {
        let path = self.path.as_ref()?;
        if path.is_done() {
            return None;
        }

        let target_pos = self.target_pos?;
        targets.contains(&target_pos).then_some(path)
    }

    fn trim_path_for_avoid_sun(
        &self,
        level: &dyn LevelReader,
        mob_position: DVec3,
        path: &mut Path,
    ) {
        if !self.avoid_sun
            || level.can_see_sky(BlockPos::containing(
                mob_position.x,
                mob_position.y + 0.5,
                mob_position.z,
            ))
        {
            return;
        }

        for index in 0..path.node_count() {
            let Some(pos) = path.node_pos(index) else {
                continue;
            };
            if level.can_see_sky(pos) {
                path.truncate_nodes(index);
                return;
            }
        }
    }

    fn trim_path(&self, level: &dyn LevelReader, mob_position: DVec3, path: &mut Path) {
        Self::trim_path_for_cauldrons(level, path);
        self.trim_path_for_avoid_sun(level, mob_position, path);
    }

    fn trim_path_for_cauldrons(level: &dyn LevelReader, path: &mut Path) {
        for index in 0..path.node_count() {
            let Some(node) = path.node(index).cloned() else {
                continue;
            };
            let Some(block) = REGISTRY
                .blocks
                .by_state_id(level.get_block_state(node.as_block_pos()))
            else {
                continue;
            };
            if !block.has_tag(&BlockTag::CAULDRONS) {
                continue;
            }

            let _ = path.replace_node(index, node.clone_and_move(node.x, node.y + 1, node.z));
            let Some(next_node) = path.node(index + 1).cloned() else {
                continue;
            };
            if node.y >= next_node.y {
                let _ = path.replace_node(
                    index + 1,
                    node.clone_and_move(next_node.x, node.y + 1, next_node.z),
                );
            }
        }
    }

    pub fn move_to(
        &mut self,
        level: &dyn LevelReader,
        mut path: Path,
        speed_modifier: f64,
        mob_position: DVec3,
    ) -> bool {
        self.direct_target = None;
        if path.node_count() == 0 {
            self.path = None;
            self.done = true;
            return false;
        }

        let same_as_current = self
            .path
            .as_ref()
            .is_some_and(|current| path.same_as(current));
        if !same_as_current {
            self.trim_path(level, mob_position, &mut path);
            self.path = Some(path);
        } else if let Some(mut path) = self.path.take() {
            self.trim_path(level, mob_position, &mut path);
            self.path = Some(path);
        }
        if self.path.as_ref().is_none_or(Path::is_done) {
            self.done = true;
            return false;
        }

        self.target_pos = self.path.as_ref().map(Path::target);
        self.speed_modifier = speed_modifier;
        self.last_stuck_check = self.tick;
        self.last_stuck_check_pos = mob_position;
        self.done = false;
        true
    }

    pub fn reuse_current_path_to_targets(
        &mut self,
        level: &dyn LevelReader,
        targets: &[BlockPos],
        speed_modifier: f64,
        mob_position: DVec3,
    ) -> bool {
        if targets.is_empty() {
            return false;
        }
        if self.path.as_ref().is_none_or(Path::is_done) {
            return false;
        }

        let Some(target_pos) = self.target_pos else {
            return false;
        };
        if !targets.contains(&target_pos) {
            return false;
        }

        if let Some(mut path) = self.path.take() {
            self.trim_path(level, mob_position, &mut path);
            self.path = Some(path);
        }
        if self.path.as_ref().is_none_or(Path::is_done) {
            self.done = true;
            return false;
        }

        self.direct_target = None;
        self.speed_modifier = speed_modifier;
        self.last_stuck_check = self.tick;
        self.last_stuck_check_pos = mob_position;
        self.done = false;
        true
    }

    pub fn set_direct_target(&mut self, target: DVec3, speed_modifier: f64) {
        self.path = None;
        self.direct_target = Some(target);
        self.target_pos = Some(BlockPos::new(
            target.x.floor() as i32,
            target.y.floor() as i32,
            target.z.floor() as i32,
        ));
        self.speed_modifier = speed_modifier;
        self.done = false;
    }

    pub fn next_move_target(&mut self, context: NavigationTickContext) -> Option<(DVec3, f64)> {
        if self.done {
            return None;
        }

        if self.path.is_some() {
            return self.next_path_move_target(context);
        }

        let target = self.direct_target?;
        if target.distance_squared(context.mob_position) < DIRECT_TARGET_REACHED_DISTANCE_SQR {
            self.stop();
            return None;
        }

        Some((target, self.speed_modifier))
    }

    pub fn next_move_target_without_path_update(
        &mut self,
        context: NavigationTickContext,
        on_ground: bool,
    ) -> Option<(DVec3, f64)> {
        if self.done {
            return None;
        }

        let path = self.path.as_mut()?;
        let Some(target) = path_move_target(path, context.mob_bounding_box_width) else {
            self.stop();
            return None;
        };

        if context.mob_position.y > target.y
            && !on_ground
            && fast_floor(context.mob_position.x) == fast_floor(target.x)
            && fast_floor(context.mob_position.z) == fast_floor(target.z)
        {
            path.advance();
        }

        if path.is_done() {
            self.stop();
            return None;
        }

        path_move_target(path, context.mob_bounding_box_width)
            .map(|target| (target, self.speed_modifier))
    }

    pub fn request_recompute_path(
        &mut self,
        game_time: i64,
        can_update_path: bool,
    ) -> Option<NavigationRecomputeRequest> {
        if game_time - self.time_last_recompute <= MAX_TIME_RECOMPUTE || !can_update_path {
            self.has_delayed_recomputation = true;
            return None;
        }

        let target_pos = self.target_pos?;
        self.path = None;
        Some(NavigationRecomputeRequest {
            target_pos,
            reach_range: self.reach_range,
            game_time,
        })
    }

    pub fn take_delayed_recompute_request(
        &mut self,
        game_time: i64,
        can_update_path: bool,
    ) -> Option<NavigationRecomputeRequest> {
        if !self.has_delayed_recomputation {
            return None;
        }

        self.request_recompute_path(game_time, can_update_path)
    }

    pub fn complete_recompute_path(&mut self, path: Option<Path>, game_time: i64) {
        self.direct_target = None;
        self.path = path;
        self.done = self.path.as_ref().is_none_or(Path::is_done);
        self.time_last_recompute = game_time;
        self.has_delayed_recomputation = false;
    }

    #[must_use]
    pub fn should_recompute_path(&self, pos: BlockPos, mob_position: DVec3) -> bool {
        if self.has_delayed_recomputation {
            return false;
        }

        let Some(path) = self.path.as_ref() else {
            return false;
        };
        if path.is_done() || path.node_count() == 0 {
            return false;
        }

        let Some(target) = path.end_node() else {
            return false;
        };
        let middle_pos = DVec3::new(
            f64::midpoint(f64::from(target.x), mob_position.x),
            f64::midpoint(f64::from(target.y), mob_position.y),
            f64::midpoint(f64::from(target.z), mob_position.z),
        );
        let distance = (path.node_count() - path.next_node_index()) as f64;
        block_center(pos).distance_squared(middle_pos) < distance * distance
    }

    fn next_path_move_target(&mut self, context: NavigationTickContext) -> Option<(DVec3, f64)> {
        {
            let Some(path) = self.path.as_mut() else {
                self.done = true;
                return None;
            };

            let Some(current_node_pos) = path.next_node_pos() else {
                self.stop();
                return None;
            };

            let max_distance_to_waypoint = if context.mob_bounding_box_width > 0.75 {
                context.mob_bounding_box_width / 2.0
            } else {
                0.75 - context.mob_bounding_box_width / 2.0
            };
            let x_distance =
                (context.mob_position.x - (f64::from(current_node_pos.x()) + 0.5)).abs();
            let y_distance = (context.mob_position.y - f64::from(current_node_pos.y())).abs();
            let z_distance =
                (context.mob_position.z - (f64::from(current_node_pos.z()) + 0.5)).abs();
            let is_close_enough_to_current_node = x_distance < max_distance_to_waypoint
                && z_distance < max_distance_to_waypoint
                && y_distance < 1.0;
            let should_cut_corner = path
                .next_node()
                .is_some_and(|node| can_cut_corner(node.path_type))
                && should_target_next_node_in_direction(path, context.mob_position);
            if is_close_enough_to_current_node || should_cut_corner {
                path.advance();
            }

            if path.is_done() {
                self.stop();
                return None;
            }
        }

        self.do_stuck_detection(context.mob_position, context.mob_speed, context.game_time);
        if self.done {
            return None;
        }

        let Some(path) = self.path.as_ref() else {
            self.stop();
            return None;
        };

        let target = path_move_target(path, context.mob_bounding_box_width)?;
        Some((target, self.speed_modifier))
    }

    fn do_stuck_detection(&mut self, mob_position: DVec3, mob_speed: f32, game_time: i64) {
        if self.tick - self.last_stuck_check > STUCK_CHECK_INTERVAL {
            let effective_speed = if mob_speed >= 1.0 {
                mob_speed
            } else {
                mob_speed * mob_speed
            };
            let threshold_distance =
                effective_speed * STUCK_CHECK_INTERVAL as f32 * STUCK_THRESHOLD_DISTANCE_FACTOR;
            if mob_position.distance_squared(self.last_stuck_check_pos)
                < f64::from(threshold_distance * threshold_distance)
            {
                self.stuck = true;
                self.stop();
            } else {
                self.stuck = false;
            }

            self.last_stuck_check = self.tick;
            self.last_stuck_check_pos = mob_position;
        }

        if self.is_done() {
            return;
        }

        let Some(current_node_pos) = self.path.as_ref().and_then(Path::next_node_pos) else {
            return;
        };
        if current_node_pos == self.timeout_cached_node {
            self.timeout_timer += game_time - self.last_timeout_check;
        } else {
            self.timeout_cached_node = current_node_pos;
            let dist_to_node = mob_position.distance(block_bottom_center(current_node_pos));
            self.timeout_limit = if mob_speed > 0.0 {
                dist_to_node / f64::from(mob_speed) * 20.0
            } else {
                0.0
            };
        }

        if self.timeout_limit > 0.0 && self.timeout_timer as f64 > self.timeout_limit * 3.0 {
            self.timeout_path();
        }

        self.last_timeout_check = game_time;
    }

    fn timeout_path(&mut self) {
        self.reset_stuck_timeout();
        self.stop();
    }

    const fn reset_stuck_timeout(&mut self) {
        self.timeout_cached_node = BlockPos::ZERO;
        self.timeout_timer = 0;
        self.timeout_limit = 0.0;
        self.stuck = false;
    }
}

fn should_target_next_node_in_direction(path: &Path, mob_position: DVec3) -> bool {
    let next_node_index = path.next_node_index();
    if next_node_index + 1 >= path.node_count() {
        return false;
    }

    let Some(current_node_pos) = path.next_node_pos() else {
        return false;
    };
    let current_node = block_bottom_center(current_node_pos);
    if mob_position.distance_squared(current_node) >= 4.0 {
        return false;
    }

    let Some(next_node_pos) = path.node_pos(next_node_index + 1) else {
        return false;
    };
    let next_node = block_bottom_center(next_node_pos);
    let mob_to_current = current_node - mob_position;
    let mob_to_next = next_node - mob_position;
    let mob_to_current_sqr = mob_to_current.length_squared();
    let mob_to_next_sqr = mob_to_next.length_squared();
    let closer_to_next_than_current = mob_to_next_sqr < mob_to_current_sqr;
    let within_current_block = mob_to_current_sqr < 0.5;
    if !closer_to_next_than_current && !within_current_block {
        return false;
    }

    mob_to_next.dot(mob_to_current) < 0.0
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_bottom_center();
    DVec3::new(x, y, z)
}

fn block_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_center();
    DVec3::new(x, y, z)
}

fn path_move_target(path: &Path, mob_bounding_box_width: f64) -> Option<DVec3> {
    path.next_node().map(|node| {
        let offset = f64::from(fast_floor(mob_bounding_box_width + 1.0)) * 0.5;
        DVec3::new(
            f64::from(node.x) + offset,
            f64::from(node.y),
            f64::from(node.z) + offset,
        )
    })
}

const fn can_cut_corner(path_type: PathType) -> bool {
    !matches!(
        path_type,
        PathType::FireInNeighbor | PathType::DamagingInNeighbor | PathType::WalkableDoor
    )
}

impl Default for PathNavigation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::{REGISTRY, test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{BlockPos, BlockStateId, WorldAabb};

    use super::{NavigationPathRequest, NavigationTickContext, PathNavigation};
    use crate::behavior::init_behaviors;
    use crate::entity::ai::node::Node;
    use crate::entity::ai::path::{Path, PathType, PathfindingMalus};
    use crate::entity::ai::walk::{MobPathSettings, WalkNodeEvaluator};
    use crate::world::LevelReader;

    struct GridLevel {
        default_state: BlockStateId,
        states: Vec<(BlockPos, BlockStateId)>,
        sky_positions: Vec<BlockPos>,
    }

    impl GridLevel {
        fn new(default_state: BlockStateId) -> Self {
            Self {
                default_state,
                states: Vec::new(),
                sky_positions: Vec::new(),
            }
        }

        fn with(mut self, pos: BlockPos, state: BlockStateId) -> Self {
            self.states.push((pos, state));
            self
        }

        fn with_sky(mut self, pos: BlockPos) -> Self {
            self.sky_positions.push(pos);
            self
        }
    }

    impl LevelReader for GridLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            self.states
                .iter()
                .find_map(|(state_pos, state)| (*state_pos == pos).then_some(*state))
                .unwrap_or(self.default_state)
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn can_see_sky(&self, pos: BlockPos) -> bool {
            self.sky_positions.contains(&pos)
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    fn node_with_path_type(x: i32, y: i32, z: i32, path_type: PathType) -> Node {
        let mut node = Node::new(x, y, z);
        node.path_type = path_type;
        node
    }

    fn tick_context(mob_position: DVec3) -> NavigationTickContext {
        NavigationTickContext {
            mob_position,
            mob_bounding_box_width: 0.9,
            mob_speed: 0.25,
            game_time: 0,
        }
    }

    fn tick_context_with_time(
        mob_position: DVec3,
        mob_speed: f32,
        game_time: i64,
    ) -> NavigationTickContext {
        NavigationTickContext {
            mob_position,
            mob_bounding_box_width: 0.9,
            mob_speed,
            game_time,
        }
    }

    fn empty_level() -> GridLevel {
        init_test_registry();
        GridLevel::new(REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR))
    }

    fn move_to(
        navigation: &mut PathNavigation,
        path: Path,
        speed_modifier: f64,
        mob_position: DVec3,
    ) -> bool {
        let level = empty_level();
        navigation.move_to(&level, path, speed_modifier, mob_position)
    }

    #[test]
    fn path_navigation_tracks_can_float_flag() {
        let mut navigation = PathNavigation::new();

        assert!(!navigation.can_float());

        navigation.set_can_float(true);

        assert!(navigation.can_float());
    }

    #[test]
    fn path_navigation_tracks_can_open_doors_flag() {
        let mut navigation = PathNavigation::new();

        assert!(!navigation.can_open_doors());

        navigation.set_can_open_doors(true);

        assert!(navigation.can_open_doors());
    }

    #[test]
    fn path_navigation_tracks_can_walk_over_fences_flag() {
        let mut navigation = PathNavigation::new();

        assert!(!navigation.can_walk_over_fences());

        navigation.set_can_walk_over_fences(true);

        assert!(navigation.can_walk_over_fences());
    }

    #[test]
    fn path_navigation_tracks_avoid_sun_flag() {
        let mut navigation = PathNavigation::new();

        assert!(!navigation.avoid_sun());

        navigation.set_avoid_sun(true);

        assert!(navigation.avoid_sun());
    }

    #[test]
    fn path_navigation_tracks_can_path_to_targets_below_surface_flag() {
        let mut navigation = PathNavigation::new();

        assert!(!navigation.can_path_to_targets_below_surface());

        navigation.set_can_path_to_targets_below_surface(true);

        assert!(navigation.can_path_to_targets_below_surface());
    }

    #[test]
    fn avoid_sun_trims_path_before_first_sky_node() {
        let level = GridLevel::new(BlockStateId(0)).with_sky(BlockPos::new(2, 64, 0));
        let mut path = Path::new(
            vec![
                Node::new(0, 64, 0),
                Node::new(1, 64, 0),
                Node::new(2, 64, 0),
                Node::new(3, 64, 0),
            ],
            BlockPos::new(3, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();
        navigation.set_avoid_sun(true);

        navigation.trim_path_for_avoid_sun(&level, DVec3::new(0.5, 64.0, 0.5), &mut path);

        assert_eq!(path.node_count(), 2);
        assert_eq!(path.node_pos(1), Some(BlockPos::new(1, 64, 0)));
    }

    #[test]
    fn avoid_sun_keeps_path_when_mob_is_already_under_sky() {
        let level = GridLevel::new(BlockStateId(0))
            .with_sky(BlockPos::new(0, 64, 0))
            .with_sky(BlockPos::new(1, 64, 0));
        let mut path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(1, 64, 0)],
            BlockPos::new(1, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();
        navigation.set_avoid_sun(true);

        navigation.trim_path_for_avoid_sun(&level, DVec3::new(0.5, 64.0, 0.5), &mut path);

        assert_eq!(path.node_count(), 2);
    }

    #[test]
    fn move_to_trims_path_over_cauldrons() {
        init_test_registry();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let cauldron = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::CAULDRON);
        let level = GridLevel::new(air).with(BlockPos::new(0, 64, 0), cauldron);
        let path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(1, 64, 0)],
            BlockPos::new(1, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(navigation.move_to(&level, path, 1.0, DVec3::new(0.5, 64.0, 0.5)));

        assert_eq!(
            navigation.path().and_then(|path| path.node_pos(0)),
            Some(BlockPos::new(0, 65, 0))
        );
        assert_eq!(
            navigation.path().and_then(|path| path.node_pos(1)),
            Some(BlockPos::new(1, 65, 0))
        );
    }

    #[test]
    fn move_to_trims_avoid_sun_path() {
        let level = empty_level().with_sky(BlockPos::new(2, 64, 0));
        let path = Path::new(
            vec![
                Node::new(0, 64, 0),
                Node::new(1, 64, 0),
                Node::new(2, 64, 0),
            ],
            BlockPos::new(2, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();
        navigation.set_avoid_sun(true);

        assert!(navigation.move_to(&level, path, 1.0, DVec3::new(0.5, 64.0, 0.5)));

        assert_eq!(navigation.path().map(Path::node_count), Some(2));
    }

    #[test]
    fn move_to_path_targets_next_node_after_current_node_is_reached() {
        let path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(1, 64, 0)],
            BlockPos::new(1, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.25,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        let target = navigation.next_move_target(tick_context(DVec3::new(0.5, 64.0, 0.5)));

        let Some((target, speed)) = target else {
            panic!("navigation should target the next path node");
        };
        assert_eq!(target, DVec3::new(1.5, 64.0, 0.5));
        assert_eq!(speed.to_bits(), 1.25_f64.to_bits());
        assert!(!navigation.is_done());
    }

    #[test]
    fn path_navigation_stops_after_final_node_is_reached() {
        let path = Path::new(vec![Node::new(0, 64, 0)], BlockPos::new(0, 64, 0), true);
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        assert!(
            navigation
                .next_move_target(tick_context(DVec3::new(0.5, 64.0, 0.5)))
                .is_none()
        );
        assert!(navigation.is_done());
        assert!(navigation.path().is_none());
        assert_eq!(navigation.target_pos(), Some(BlockPos::new(0, 64, 0)));
    }

    #[test]
    fn path_navigation_targets_next_node_when_mob_passed_current_node_directionally() {
        let path = Path::new(
            vec![
                node_with_path_type(0, 64, 0, PathType::Walkable),
                Node::new(1, 64, 0),
                Node::new(2, 64, 0),
            ],
            BlockPos::new(2, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.0, 64.0, 0.5)
        ));

        let target = navigation.next_move_target(tick_context(DVec3::new(1.1, 64.0, 0.5)));

        let Some((target, _speed)) = target else {
            panic!("navigation should target a path node");
        };
        assert_eq!(target, DVec3::new(1.5, 64.0, 0.5));
        assert_eq!(navigation.path().map(Path::next_node_index), Some(1));
    }

    #[test]
    fn path_navigation_does_not_cut_corner_through_walkable_door() {
        let path = Path::new(
            vec![
                node_with_path_type(0, 64, 0, PathType::WalkableDoor),
                Node::new(1, 64, 0),
            ],
            BlockPos::new(1, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.0, 64.0, 0.5)
        ));

        let target = navigation.next_move_target(tick_context(DVec3::new(1.1, 64.0, 0.5)));

        let Some((target, _speed)) = target else {
            panic!("navigation should target the current path node");
        };
        assert_eq!(target, DVec3::new(0.5, 64.0, 0.5));
        assert_eq!(navigation.path().map(Path::next_node_index), Some(0));
    }

    #[test]
    fn path_navigation_without_update_does_not_advance_normally() {
        let path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(1, 64, 0)],
            BlockPos::new(1, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        let target = navigation
            .next_move_target_without_path_update(tick_context(DVec3::new(0.5, 64.0, 0.5)), false);

        assert_eq!(
            target.map(|(target, _)| target),
            Some(DVec3::new(0.5, 64.0, 0.5))
        );
        assert_eq!(navigation.path().map(Path::next_node_index), Some(0));
    }

    #[test]
    fn path_navigation_without_update_advances_when_airborne_over_node() {
        let path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(1, 64, 0)],
            BlockPos::new(1, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 65.0, 0.5)
        ));

        let target = navigation
            .next_move_target_without_path_update(tick_context(DVec3::new(0.5, 65.0, 0.5)), false);

        assert_eq!(
            target.map(|(target, _)| target),
            Some(DVec3::new(1.5, 64.0, 0.5))
        );
        assert_eq!(navigation.path().map(Path::next_node_index), Some(1));
    }

    #[test]
    fn path_navigation_stops_when_stationary_past_stuck_interval() {
        let path = Path::new(vec![Node::new(1, 64, 0)], BlockPos::new(1, 64, 0), true);
        let mut navigation = PathNavigation::new();
        let mob_position = DVec3::new(0.0, 64.0, 0.5);

        assert!(move_to(&mut navigation, path, 1.0, mob_position));
        for game_time in 1..=101 {
            navigation.tick();
            let _ =
                navigation.next_move_target(tick_context_with_time(mob_position, 0.25, game_time));
        }

        assert!(navigation.is_stuck());
        assert!(navigation.is_done());
    }

    #[test]
    fn path_navigation_times_out_when_same_node_takes_too_long() {
        let path = Path::new(vec![Node::new(2, 64, 0)], BlockPos::new(2, 64, 0), true);
        let mut navigation = PathNavigation::new();
        let mob_position = DVec3::new(1.0, 64.0, 0.5);

        assert!(move_to(&mut navigation, path, 1.0, mob_position));
        for game_time in 1..=92 {
            navigation.tick();
            let _ =
                navigation.next_move_target(tick_context_with_time(mob_position, 1.0, game_time));
        }

        assert!(!navigation.is_stuck());
        assert!(navigation.is_done());
    }

    #[test]
    fn path_recompute_request_delays_during_vanilla_cooldown() {
        let path = Path::new(vec![Node::new(2, 64, 0)], BlockPos::new(2, 64, 0), true);
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        assert_eq!(navigation.request_recompute_path(20, true), None);
        assert!(navigation.has_delayed_recomputation());
        assert!(navigation.path().is_some());

        let Some(request) = navigation.take_delayed_recompute_request(21, true) else {
            panic!("recompute should be allowed after vanilla cooldown");
        };
        assert_eq!(request.target_pos, BlockPos::new(2, 64, 0));
        assert_eq!(request.reach_range, 0);
        assert_eq!(request.game_time, 21);
        assert!(navigation.path().is_none());

        navigation.complete_recompute_path(
            Some(Path::new(
                vec![Node::new(1, 64, 0), Node::new(2, 64, 0)],
                BlockPos::new(2, 64, 0),
                true,
            )),
            request.game_time,
        );
        assert!(!navigation.has_delayed_recomputation());
        assert!(!navigation.is_done());
    }

    #[test]
    fn path_recompute_request_waits_until_path_can_update() {
        let path = Path::new(vec![Node::new(2, 64, 0)], BlockPos::new(2, 64, 0), true);
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        assert_eq!(navigation.request_recompute_path(30, false), None);
        assert!(navigation.has_delayed_recomputation());
        assert_eq!(navigation.take_delayed_recompute_request(40, false), None);
        assert!(navigation.has_delayed_recomputation());

        let Some(request) = navigation.take_delayed_recompute_request(40, true) else {
            panic!("recompute should run once path updates are allowed");
        };
        assert_eq!(request.target_pos, BlockPos::new(2, 64, 0));
    }

    #[test]
    fn stop_keeps_delayed_recompute_state() {
        let path = Path::new(vec![Node::new(2, 64, 0)], BlockPos::new(2, 64, 0), true);
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        assert_eq!(navigation.request_recompute_path(20, true), None);
        assert!(navigation.has_delayed_recomputation());

        navigation.stop();

        assert!(navigation.has_delayed_recomputation());
        assert_eq!(navigation.target_pos(), Some(BlockPos::new(2, 64, 0)));
        assert_eq!(navigation.speed_modifier().to_bits(), 1.0_f64.to_bits());
        let Some(request) = navigation.take_delayed_recompute_request(21, true) else {
            panic!("stopped navigation should keep enough state to recompute the target");
        };
        assert_eq!(request.target_pos, BlockPos::new(2, 64, 0));

        navigation.complete_recompute_path(
            Some(Path::new(
                vec![Node::new(1, 64, 0), Node::new(2, 64, 0)],
                BlockPos::new(2, 64, 0),
                true,
            )),
            request.game_time,
        );
        assert!(!navigation.is_done());
        assert!(!navigation.has_delayed_recomputation());
        assert!(navigation.path().is_some());
    }

    #[test]
    fn path_should_recompute_uses_vanilla_midpoint_window() {
        let path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(4, 64, 0)],
            BlockPos::new(4, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();
        let mob_position = DVec3::new(0.5, 64.0, 0.5);

        assert!(move_to(&mut navigation, path, 1.0, mob_position));
        assert!(navigation.should_recompute_path(BlockPos::new(2, 64, 0), mob_position));
        assert!(!navigation.should_recompute_path(BlockPos::new(20, 64, 0), mob_position));

        assert_eq!(navigation.request_recompute_path(1, true), None);
        assert!(!navigation.should_recompute_path(BlockPos::new(2, 64, 0), mob_position));
    }

    #[test]
    fn move_to_same_path_keeps_current_progress() {
        let path = Path::new(
            vec![
                Node::new(0, 64, 0),
                Node::new(1, 64, 0),
                Node::new(2, 64, 0),
            ],
            BlockPos::new(2, 64, 0),
            true,
        );
        let same_path = Path::new(
            vec![
                Node::new(0, 64, 0),
                Node::new(1, 64, 0),
                Node::new(2, 64, 0),
            ],
            BlockPos::new(2, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));
        assert!(
            navigation
                .next_move_target(tick_context(DVec3::new(0.5, 64.0, 0.5)))
                .is_some()
        );
        assert_eq!(navigation.path().map(Path::next_node_index), Some(1));

        assert!(move_to(
            &mut navigation,
            same_path,
            1.5,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        assert_eq!(navigation.path().map(Path::next_node_index), Some(1));
        assert_eq!(navigation.speed_modifier().to_bits(), 1.5_f64.to_bits());
    }

    #[test]
    fn path_navigation_reuses_current_path_for_matching_target() {
        let path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(4, 64, 0)],
            BlockPos::new(4, 64, 0),
            true,
        );
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));
        let level = empty_level();
        assert!(navigation.reuse_current_path_to_targets(
            &level,
            &[BlockPos::new(4, 64, 0)],
            1.25,
            DVec3::new(1.5, 64.0, 0.5),
        ));

        assert!(!navigation.is_done());
        assert_eq!(navigation.speed_modifier().to_bits(), 1.25_f64.to_bits());
        assert_eq!(
            navigation.path().map(Path::target),
            Some(BlockPos::new(4, 64, 0))
        );
        assert_eq!(navigation.path().map(Path::next_node_index), Some(0));
    }

    #[test]
    fn create_path_reuses_current_path_for_matching_target() {
        let mut path = Path::new(
            vec![
                Node::new(0, 64, 0),
                Node::new(1, 64, 0),
                Node::new(2, 64, 0),
            ],
            BlockPos::new(2, 64, 0),
            true,
        );
        path.set_next_node_index(1);
        let mut navigation = PathNavigation::new();
        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        let level = GridLevel::new(BlockStateId(0));
        let malus = PathfindingMalus::new();
        let mut evaluator = WalkNodeEvaluator::new(MobPathSettings::new(
            1,
            1,
            1,
            BlockPos::new(0, 64, 0),
            &malus,
        ));
        let mut no_collision = |_aabb: WorldAabb| false;

        let reused = navigation.create_path(
            &mut evaluator,
            &level,
            &mut no_collision,
            NavigationPathRequest {
                mob_position: BlockPos::new(0, 64, 0),
                targets: &[BlockPos::new(2, 64, 0)],
                max_path_length: 16.0,
                reach_range: 0,
            },
        );

        let Some(reused) = reused else {
            panic!("matching active path should be reused");
        };
        assert_eq!(reused.target(), BlockPos::new(2, 64, 0));
        assert_eq!(reused.next_node_index(), 1);
        assert_eq!(navigation.path().map(Path::next_node_index), Some(1));
    }

    #[test]
    fn path_navigation_does_not_reuse_current_path_for_different_target() {
        let path = Path::new(vec![Node::new(4, 64, 0)], BlockPos::new(4, 64, 0), true);
        let mut navigation = PathNavigation::new();

        assert!(move_to(
            &mut navigation,
            path,
            1.0,
            DVec3::new(0.5, 64.0, 0.5)
        ));

        let level = empty_level();
        assert!(!navigation.reuse_current_path_to_targets(
            &level,
            &[BlockPos::new(5, 64, 0)],
            1.25,
            DVec3::new(0.5, 64.0, 0.5),
        ));
        assert_eq!(navigation.speed_modifier().to_bits(), 1.0_f64.to_bits());
    }

    #[test]
    fn path_navigation_updates_current_speed_modifier() {
        let mut navigation = PathNavigation::new();

        navigation.set_speed_modifier(1.75);

        assert_eq!(navigation.speed_modifier().to_bits(), 1.75_f64.to_bits());
    }

    #[test]
    fn direct_target_stops_when_reached() {
        let mut navigation = PathNavigation::new();
        navigation.set_direct_target(DVec3::new(1.0, 64.0, 1.0), 0.5);

        assert!(
            navigation
                .next_move_target(tick_context(DVec3::new(1.0, 64.0, 1.0)))
                .is_none()
        );
        assert!(navigation.is_done());
    }

    #[test]
    fn create_path_finds_walkable_target_with_cached_navigation_state() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air)
            .with(BlockPos::new(0, 63, 0), stone)
            .with(BlockPos::new(1, 63, 0), stone)
            .with(BlockPos::new(2, 63, 0), stone);
        let malus = PathfindingMalus::new();
        let mut evaluator = WalkNodeEvaluator::new(MobPathSettings::new(
            1,
            1,
            1,
            BlockPos::new(0, 64, 0),
            &malus,
        ));
        let mut navigation = PathNavigation::new();
        navigation.update_pathfinder_max_visited_nodes(16.0);
        let mut no_collision = |_aabb: WorldAabb| false;

        let path = navigation.create_path(
            &mut evaluator,
            &level,
            &mut no_collision,
            NavigationPathRequest {
                mob_position: BlockPos::new(0, 64, 0),
                targets: &[BlockPos::new(2, 64, 0)],
                max_path_length: 16.0,
                reach_range: 0,
            },
        );

        let Some(path) = path else {
            panic!("path should be found");
        };
        assert!(path.can_reach());
        assert_eq!(navigation.target_pos(), Some(BlockPos::new(2, 64, 0)));
        assert_eq!(navigation.reach_range(), 0);
    }
}
