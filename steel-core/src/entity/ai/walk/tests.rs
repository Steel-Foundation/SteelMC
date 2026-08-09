use std::sync::Weak;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, SlabType};
use steel_registry::{REGISTRY, init_vanilla_registry, vanilla_blocks, vanilla_entities};
use steel_utils::{BlockPos, BlockStateId, Direction, WorldAabb};

use super::{
    MobPathSettings, WalkNodeEvaluator, WalkPathEvaluator,
    node_evaluator::{AcceptedNodeRequest, WalkNeighbors},
};
use crate::behavior::{BlockStateBehaviorExt as _, init_behaviors};
use crate::entity::Mob as _;
use crate::entity::ai::path::{
    PathComputationType, PathType, PathfindingContext, PathfindingMalus,
};
use crate::entity::entities::PigEntity;
use crate::world::LevelReader;

struct GridLevel {
    default_state: BlockStateId,
    states: Vec<(BlockPos, BlockStateId)>,
}

impl GridLevel {
    fn new(default_state: BlockStateId) -> Self {
        Self {
            default_state,
            states: Vec::new(),
        }
    }

    fn with(mut self, pos: BlockPos, state: BlockStateId) -> Self {
        self.states.push((pos, state));
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

    fn min_y(&self) -> i32 {
        -64
    }

    fn height(&self) -> i32 {
        384
    }
}

#[test]
fn mob_path_settings_reads_can_open_doors_from_navigation() {
    init_vanilla_registry();
    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.mob_base().navigation().lock().set_can_open_doors(true);

    let settings = MobPathSettings::from_mob(&pig);

    assert!(settings.can_open_doors());
}

#[test]
fn mob_path_settings_reads_can_walk_over_fences_from_navigation() {
    init_vanilla_registry();
    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.mob_base()
        .navigation()
        .lock()
        .set_can_walk_over_fences(true);

    let settings = MobPathSettings::from_mob(&pig);

    assert!(settings.can_walk_over_fences());
}

#[test]
fn path_type_from_state_matches_core_vanilla_special_cases() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
    let lava = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::LAVA);
    let cactus = REGISTRY
        .blocks
        .get_default_state_id(&vanilla_blocks::CACTUS);
    let honey = REGISTRY
        .blocks
        .get_default_state_id(&vanilla_blocks::HONEY_BLOCK);
    let pointed_dripstone = REGISTRY
        .blocks
        .get_default_state_id(&vanilla_blocks::POINTED_DRIPSTONE);
    let sulfur_spike = REGISTRY
        .blocks
        .get_default_state_id(&vanilla_blocks::SULFUR_SPIKE);

    assert_eq!(classify(air), PathType::Open);
    assert_eq!(classify(water), PathType::Water);
    assert_eq!(classify(lava), PathType::Lava);
    assert_eq!(classify(cactus), PathType::Damaging);
    assert_eq!(classify(honey), PathType::StickyHoney);
    assert_eq!(classify(pointed_dripstone), PathType::DamageCautious);
    assert_eq!(classify(sulfur_spike), PathType::DamageCautious);
}

#[test]
fn doors_use_vanilla_mob_interactable_door_tag() {
    init_vanilla_registry();
    init_behaviors();

    let oak_closed = vanilla_blocks::OAK_DOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false);
    let iron_closed = vanilla_blocks::IRON_DOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false);
    let copper_closed = vanilla_blocks::COPPER_DOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false);
    let oak_open = oak_closed.set_value(&BlockStateProperties::OPEN, true);

    assert_eq!(classify(oak_closed), PathType::DoorWoodClosed);
    assert_eq!(classify(copper_closed), PathType::DoorWoodClosed);
    assert_eq!(classify(iron_closed), PathType::DoorIronClosed);
    assert_eq!(classify(oak_open), PathType::DoorOpen);
}

#[test]
fn block_state_pathfindable_uses_behavior_overrides() {
    init_vanilla_registry();
    init_behaviors();

    let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
    let lava = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::LAVA);
    let cactus = REGISTRY
        .blocks
        .get_default_state_id(&vanilla_blocks::CACTUS);
    let cocoa = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::COCOA);
    let powder_snow = REGISTRY
        .blocks
        .get_default_state_id(&vanilla_blocks::POWDER_SNOW);
    let shallow_snow = vanilla_blocks::SNOW
        .default_state()
        .set_value(&BlockStateProperties::LAYERS, 4);
    let deep_snow = shallow_snow.set_value(&BlockStateProperties::LAYERS, 5);
    let oak_closed = vanilla_blocks::OAK_DOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false);
    let oak_open = oak_closed.set_value(&BlockStateProperties::OPEN, true);

    assert!(water.is_pathfindable(PathComputationType::Land));
    assert!(!lava.is_pathfindable(PathComputationType::Land));
    assert!(!cactus.is_pathfindable(PathComputationType::Land));
    assert!(!cocoa.is_pathfindable(PathComputationType::Land));
    assert!(!cocoa.is_pathfindable(PathComputationType::Air));
    assert!(!cocoa.is_pathfindable(PathComputationType::Water));
    assert!(powder_snow.is_pathfindable(PathComputationType::Land));
    assert!(shallow_snow.is_pathfindable(PathComputationType::Land));
    assert!(!deep_snow.is_pathfindable(PathComputationType::Land));
    assert!(!oak_closed.is_pathfindable(PathComputationType::Land));
    assert!(oak_open.is_pathfindable(PathComputationType::Air));
    assert!(!oak_open.is_pathfindable(PathComputationType::Water));
}

#[test]
fn walk_node_evaluator_applies_vanilla_door_adjustments_for_mobs() {
    init_vanilla_registry();
    init_behaviors();

    let oak_closed = vanilla_blocks::OAK_DOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false);
    let oak_open = oak_closed.set_value(&BlockStateProperties::OPEN, true);
    let closed_level = GridLevel::new(oak_closed);
    let open_level = GridLevel::new(oak_open);
    let mut closed_context = PathfindingContext::new(&closed_level, BlockPos::ZERO);
    let mut open_context = PathfindingContext::new(&open_level, BlockPos::ZERO);

    let opener = WalkNodeEvaluator::new(
        test_settings(1, 1, 1)
            .with_can_open_doors(true)
            .with_can_pass_doors(true),
    );
    let blocker = WalkNodeEvaluator::new(test_settings(1, 1, 1).with_can_pass_doors(false));

    assert_eq!(
        opener.get_path_type_of_mob(&mut closed_context, 0, 64, 0),
        PathType::WalkableDoor
    );
    assert_eq!(
        blocker.get_path_type_of_mob(&mut open_context, 0, 64, 0),
        PathType::Blocked
    );
}

#[test]
fn walk_node_evaluator_marks_rails_unpassable_when_mob_is_not_on_rails() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let rail = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::RAIL);
    let level = GridLevel::new(air).with(BlockPos::new(1, 64, 0), rail);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

    assert_eq!(
        evaluator.get_path_type_of_mob(&mut context, 1, 64, 0),
        PathType::UnpassableRail
    );
}

#[test]
fn large_walk_node_evaluator_caps_nearby_danger_cost_like_vanilla() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
    let level = GridLevel::new(air)
        .with(BlockPos::new(0, 63, 0), stone)
        .with(BlockPos::new(3, 64, 0), water);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let evaluator = WalkNodeEvaluator::new(test_settings(4, 1, 1));

    assert_eq!(
        evaluator.get_path_type_of_mob(&mut context, 0, 64, 0),
        PathType::BigMobsCloseToDanger
    );
}

#[test]
fn walk_node_evaluator_floor_level_uses_collision_shape_below_node() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let bottom_slab = vanilla_blocks::SMOOTH_STONE_SLAB
        .default_state()
        .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Bottom);
    let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), bottom_slab);
    let context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

    assert_eq!(
        evaluator
            .get_floor_level(&context, BlockPos::new(0, 64, 0))
            .to_bits(),
        63.5_f64.to_bits()
    );
}

#[test]
fn get_start_uses_grounded_mob_block_position() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

    let start = evaluator.get_start(&mut context);

    let Some(node) = evaluator.node(start) else {
        panic!("start node should exist");
    };
    assert_eq!((node.x, node.y, node.z), (0, 64, 0));
    assert_eq!(node.path_type, PathType::Walkable);
    assert_eq!(node.cost_malus.to_bits(), 0.0_f32.to_bits());
}

#[test]
fn get_start_floats_to_top_water_node_when_mob_can_float() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
    let level = GridLevel::new(air)
        .with(BlockPos::new(0, 64, 0), water)
        .with(BlockPos::new(0, 65, 0), water);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(
        test_settings(1, 1, 1)
            .with_can_float(true)
            .with_in_water(true)
            .with_on_ground(false),
    );

    let start = evaluator.get_start(&mut context);

    let Some(node) = evaluator.node(start) else {
        panic!("start node should exist");
    };
    assert_eq!((node.x, node.y, node.z), (0, 65, 0));
    assert_eq!(node.path_type, PathType::Water);
}

#[test]
fn get_start_scans_down_to_first_ground_when_airborne() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 62, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1).with_on_ground(false));

    let start = evaluator.get_start(&mut context);

    let Some(node) = evaluator.node(start) else {
        panic!("start node should exist");
    };
    assert_eq!((node.x, node.y, node.z), (0, 63, 0));
    assert_eq!(node.path_type, PathType::Walkable);
}

#[test]
fn get_start_uses_first_startable_bounding_box_corner() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 63, 1), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

    let start = evaluator.get_start(&mut context);

    let Some(node) = evaluator.node(start) else {
        panic!("start node should exist");
    };
    assert_eq!((node.x, node.y, node.z), (0, 64, 1));
    assert_eq!(node.path_type, PathType::Walkable);
}

#[test]
fn find_accepted_node_records_walkable_node_cost() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
    let mut no_collision = |_aabb: WorldAabb| false;

    let accepted = evaluator.find_accepted_node(
        &mut context,
        &mut no_collision,
        accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Walkable),
    );

    let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
        panic!("walkable node should be accepted");
    };
    assert_eq!(node.path_type, PathType::Walkable);
    assert_eq!(node.cost_malus.to_bits(), 0.0_f32.to_bits());
}

#[test]
fn find_accepted_node_falls_to_ground_when_within_max_fall_distance() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 64, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 67, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1).with_max_fall_distance(3));
    let mut no_collision = |_aabb: WorldAabb| false;

    let accepted = evaluator.find_accepted_node(
        &mut context,
        &mut no_collision,
        accepted_request(BlockPos::new(0, 67, 0), 0, 66.0, PathType::Open),
    );

    let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
        panic!("open node should fall to ground");
    };
    assert_eq!((node.x, node.y, node.z), (0, 65, 0));
    assert_eq!(node.path_type, PathType::Walkable);
}

#[test]
fn find_accepted_node_blocks_falls_past_max_fall_distance() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 64, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 67, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1).with_max_fall_distance(1));
    let mut no_collision = |_aabb: WorldAabb| false;

    let accepted = evaluator.find_accepted_node(
        &mut context,
        &mut no_collision,
        accepted_request(BlockPos::new(0, 67, 0), 0, 66.0, PathType::Open),
    );

    let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
        panic!("excessive fall should produce a blocked node");
    };
    assert_eq!((node.x, node.y, node.z), (0, 65, 0));
    assert_eq!(node.path_type, PathType::Blocked);
    assert_eq!(node.cost_malus.to_bits(), (-1.0_f32).to_bits());
}

#[test]
fn find_accepted_node_keeps_last_water_node_before_non_water() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air)
        .with(BlockPos::new(0, 64, 0), water)
        .with(BlockPos::new(0, 63, 0), water)
        .with(BlockPos::new(0, 62, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
    let mut no_collision = |_aabb: WorldAabb| false;

    let accepted = evaluator.find_accepted_node(
        &mut context,
        &mut no_collision,
        accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Water),
    );

    let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
        panic!("water scan should keep the deepest water node");
    };
    assert_eq!((node.x, node.y, node.z), (0, 63, 0));
    assert_eq!(node.path_type, PathType::Water);
}

#[test]
fn find_accepted_node_rejects_partial_collision_when_reach_is_blocked() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
    let mut collision_checked = false;
    let mut blocked = |_aabb: WorldAabb| {
        collision_checked = true;
        true
    };

    let accepted = evaluator.find_accepted_node(
        &mut context,
        &mut blocked,
        accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::DoorWoodClosed),
    );

    assert!(accepted.is_none());
    assert!(collision_checked);
}

#[test]
fn get_neighbors_expands_all_cardinal_and_diagonal_nodes_on_flat_ground() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let mut level = GridLevel::new(air);
    for x in -1..=1 {
        for z in -1..=1 {
            level = level.with(BlockPos::new(x, 63, z), stone);
        }
    }
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
    let mut no_collision = |_aabb: WorldAabb| false;
    let Some(current) = evaluator.find_accepted_node(
        &mut context,
        &mut no_collision,
        accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Walkable),
    ) else {
        panic!("current walkable node should be accepted");
    };

    let neighbors = evaluator.get_neighbors(&mut context, &mut no_collision, current);

    assert_eq!(neighbors.len(), 8);
    let positions = neighbor_positions(&evaluator, &neighbors);
    assert!(positions.contains(&(0, 64, -1)));
    assert!(positions.contains(&(1, 64, 0)));
    assert!(positions.contains(&(0, 64, 1)));
    assert!(positions.contains(&(-1, 64, 0)));
    assert!(positions.contains(&(1, 64, -1)));
    assert!(positions.contains(&(1, 64, 1)));
    assert!(positions.contains(&(-1, 64, 1)));
    assert!(positions.contains(&(-1, 64, -1)));
}

#[test]
fn get_neighbors_rejects_diagonals_through_walkable_doors() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let oak_closed = vanilla_blocks::OAK_DOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false);
    let mut level = GridLevel::new(air).with(BlockPos::new(0, 64, -1), oak_closed);
    for x in -1..=1 {
        for z in -1..=1 {
            level = level.with(BlockPos::new(x, 63, z), stone);
        }
    }
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
    let mut evaluator = WalkNodeEvaluator::new(
        test_settings(1, 1, 1)
            .with_can_open_doors(true)
            .with_can_pass_doors(true),
    );
    let mut no_collision = |_aabb: WorldAabb| false;
    let Some(current) = evaluator.find_accepted_node(
        &mut context,
        &mut no_collision,
        accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Walkable),
    ) else {
        panic!("current walkable node should be accepted");
    };

    let neighbors = evaluator.get_neighbors(&mut context, &mut no_collision, current);

    let positions = neighbor_positions(&evaluator, &neighbors);
    assert!(positions.contains(&(0, 64, -1)));
    assert!(positions.contains(&(1, 64, 0)));
    assert!(positions.contains(&(-1, 64, 0)));
    assert!(!positions.contains(&(1, 64, -1)));
    assert!(!positions.contains(&(-1, 64, -1)));
}

#[test]
fn open_air_above_solid_ground_becomes_walkable() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));

    assert_eq!(
        WalkPathEvaluator::path_type_static(&mut context, BlockPos::new(0, 64, 0)),
        PathType::Walkable
    );
}

#[test]
fn walkable_ground_adjacent_to_water_becomes_water_border() {
    init_vanilla_registry();
    init_behaviors();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
    let level = GridLevel::new(air)
        .with(BlockPos::new(0, 63, 0), stone)
        .with(BlockPos::new(1, 64, 0), water);
    let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));

    assert_eq!(
        WalkPathEvaluator::path_type_static(&mut context, BlockPos::new(0, 64, 0)),
        PathType::WaterBorder
    );
}

fn classify(state: BlockStateId) -> PathType {
    let level = GridLevel::new(state);
    WalkPathEvaluator::path_type_from_state(&level, BlockPos::ZERO)
}

fn test_settings(entity_width: i32, entity_height: i32, entity_depth: i32) -> MobPathSettings {
    MobPathSettings::new(
        entity_width,
        entity_height,
        entity_depth,
        BlockPos::new(0, 64, 0),
        &PathfindingMalus::new(),
    )
}

const fn accepted_request(
    pos: BlockPos,
    jump_size: i32,
    node_height: f64,
    current_path_type: PathType,
) -> AcceptedNodeRequest {
    AcceptedNodeRequest {
        pos,
        jump_size,
        node_height,
        travel_direction: Direction::North,
        current_path_type,
    }
}

fn neighbor_positions(
    evaluator: &WalkNodeEvaluator,
    neighbors: &WalkNeighbors,
) -> Vec<(i32, i32, i32)> {
    neighbors
        .iter()
        .filter_map(|hash| evaluator.node(hash).map(|node| (node.x, node.y, node.z)))
        .collect()
}
