use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    AttachFace, BlockStateProperties, DoubleBlockHalf, SlabType,
};
use steel_registry::{Registry, vanilla_blocks};
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockStateId, BoundingBox, Direction};

use crate::worldgen::region::WorldGenRegion;
use steel_worldgen::structure::stronghold::{StrongholdPieceData, StrongholdSmallDoorType};

use super::{StructurePiecePlacer, scattered_feature::ScatteredFeaturePlacer};

const STRONGHOLD_CORRIDOR_LOOT: &str = "minecraft:chests/stronghold_corridor";
const STRONGHOLD_CROSSING_LOOT: &str = "minecraft:chests/stronghold_crossing";
const STRONGHOLD_LIBRARY_LOOT: &str = "minecraft:chests/stronghold_library";
const SILVERFISH_ENTITY: &str = "minecraft:silverfish";

impl StructurePiecePlacer {
    pub(super) fn place_stronghold_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounding_box: BoundingBox,
        orientation: Option<Direction>,
        data: &mut StrongholdPieceData,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
    ) -> bool {
        let Some(
            orientation @ (Direction::North | Direction::South | Direction::West | Direction::East),
        ) = orientation
        else {
            return false;
        };

        let mut piece_bounding_box = bounding_box;
        let mut placer = ScatteredFeaturePlacer::new(
            region,
            registry,
            &mut piece_bounding_box,
            Some(orientation),
            clip,
        );
        match data {
            StrongholdPieceData::Straight {
                entry_door,
                left_child,
                right_child,
            } => place_straight(&mut placer, random, *entry_door, *left_child, *right_child),
            StrongholdPieceData::PrisonHall { entry_door } => {
                place_prison_hall(&mut placer, random, *entry_door);
            }
            StrongholdPieceData::LeftTurn { entry_door } => {
                place_turn(&mut placer, random, *entry_door, true, orientation);
            }
            StrongholdPieceData::RightTurn { entry_door } => {
                place_turn(&mut placer, random, *entry_door, false, orientation);
            }
            StrongholdPieceData::RoomCrossing {
                entry_door,
                crossing_type,
            } => place_room_crossing(&mut placer, random, *entry_door, *crossing_type),
            StrongholdPieceData::StraightStairsDown { entry_door } => {
                place_straight_stairs_down(&mut placer, random, *entry_door);
            }
            StrongholdPieceData::StairsDown { entry_door, .. } => {
                place_stairs_down(&mut placer, random, *entry_door);
            }
            StrongholdPieceData::FiveCrossing {
                entry_door,
                left_low,
                left_high,
                right_low,
                right_high,
            } => place_five_crossing(
                &mut placer,
                random,
                *entry_door,
                *left_low,
                *left_high,
                *right_low,
                *right_high,
            ),
            StrongholdPieceData::ChestCorridor {
                entry_door,
                has_placed_chest,
            } => place_chest_corridor(&mut placer, random, *entry_door, has_placed_chest),
            StrongholdPieceData::Library {
                entry_door,
                is_tall,
            } => place_library(&mut placer, random, *entry_door, *is_tall),
            StrongholdPieceData::PortalRoom { has_placed_spawner } => {
                place_portal_room(&mut placer, random, has_placed_spawner);
            }
            StrongholdPieceData::FillerCorridor { steps } => {
                place_filler_corridor(&mut placer, *steps);
            }
        }
        true
    }
}

fn stone_bricks() -> BlockStateId {
    vanilla_blocks::STONE_BRICKS.default_state()
}

fn cracked_stone_bricks() -> BlockStateId {
    vanilla_blocks::CRACKED_STONE_BRICKS.default_state()
}

fn mossy_stone_bricks() -> BlockStateId {
    vanilla_blocks::MOSSY_STONE_BRICKS.default_state()
}

fn infested_stone_bricks() -> BlockStateId {
    vanilla_blocks::INFESTED_STONE_BRICKS.default_state()
}

fn cave_air() -> BlockStateId {
    vanilla_blocks::CAVE_AIR.default_state()
}

fn smooth_stone_slab() -> BlockStateId {
    vanilla_blocks::SMOOTH_STONE_SLAB.default_state()
}

fn double_smooth_stone_slab() -> BlockStateId {
    vanilla_blocks::SMOOTH_STONE_SLAB
        .default_state()
        .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Double)
}

fn wall_torch(facing: Direction) -> BlockStateId {
    vanilla_blocks::WALL_TORCH
        .default_state()
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
}

fn ladder(facing: Direction) -> BlockStateId {
    vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, facing)
}

fn stone_brick_stairs(facing: Direction) -> BlockStateId {
    vanilla_blocks::STONE_BRICK_STAIRS
        .default_state()
        .set_value(&BlockStateProperties::FACING, facing)
}

fn cobblestone_stairs(facing: Direction) -> BlockStateId {
    vanilla_blocks::COBBLESTONE_STAIRS
        .default_state()
        .set_value(&BlockStateProperties::FACING, facing)
}

const CONNECT_NORTH: u8 = 1;
const CONNECT_EAST: u8 = 2;
const CONNECT_SOUTH: u8 = 4;
const CONNECT_WEST: u8 = 8;

fn iron_bars(mask: u8) -> BlockStateId {
    vanilla_blocks::IRON_BARS
        .default_state()
        .set_value(&BlockStateProperties::NORTH, mask & CONNECT_NORTH != 0)
        .set_value(&BlockStateProperties::EAST, mask & CONNECT_EAST != 0)
        .set_value(&BlockStateProperties::SOUTH, mask & CONNECT_SOUTH != 0)
        .set_value(&BlockStateProperties::WEST, mask & CONNECT_WEST != 0)
}

fn oak_fence(mask: u8) -> BlockStateId {
    vanilla_blocks::OAK_FENCE
        .default_state()
        .set_value(&BlockStateProperties::NORTH, mask & CONNECT_NORTH != 0)
        .set_value(&BlockStateProperties::EAST, mask & CONNECT_EAST != 0)
        .set_value(&BlockStateProperties::SOUTH, mask & CONNECT_SOUTH != 0)
        .set_value(&BlockStateProperties::WEST, mask & CONNECT_WEST != 0)
}

fn upper_door(state: BlockStateId) -> BlockStateId {
    state.set_value(
        &BlockStateProperties::DOUBLE_BLOCK_HALF,
        DoubleBlockHalf::Upper,
    )
}

fn stone_button(facing: Direction) -> BlockStateId {
    vanilla_blocks::STONE_BUTTON
        .default_state()
        .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Wall)
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
}

fn end_portal_frame(facing: Direction, has_eye: bool) -> BlockStateId {
    vanilla_blocks::END_PORTAL_FRAME
        .default_state()
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
        .set_value(&BlockStateProperties::EYE, has_eye)
}

fn smooth_stone_selector(
    random: &mut WorldgenRandom,
    _x: i32,
    _y: i32,
    _z: i32,
    is_edge: bool,
) -> BlockStateId {
    if !is_edge {
        return cave_air();
    }

    let selection = random.next_f32();
    if selection < 0.2 {
        cracked_stone_bricks()
    } else if selection < 0.5 {
        mossy_stone_bricks()
    } else if selection < 0.55 {
        infested_stone_bricks()
    } else {
        stone_bricks()
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla StructurePiece.generateBox selector overload"
)]
fn stronghold_box(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
    skip_air: bool,
) {
    placer.generate_box_with_selector(
        x0,
        y0,
        z0,
        x1,
        y1,
        z1,
        skip_air,
        random,
        smooth_stone_selector,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla StructurePiece.generateMaybeBox for stronghold cobwebs"
)]
fn generate_maybe_box(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    probability: f32,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
    edge: BlockStateId,
    fill: BlockStateId,
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if random.next_f32() > probability {
                    continue;
                }
                let state = if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                    fill
                } else {
                    edge
                };
                placer.place_block(state, x, y, z);
            }
        }
    }
}

fn maybe_generate_block(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    probability: f32,
    x: i32,
    y: i32,
    z: i32,
    state: BlockStateId,
) {
    if random.next_f32() < probability {
        placer.place_block(state, x, y, z);
    }
}

fn generate_small_door(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    door_type: StrongholdSmallDoorType,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
) {
    match door_type {
        StrongholdSmallDoorType::Opening => {
            placer.generate_box(
                foot_x,
                foot_y,
                foot_z,
                foot_x + 2,
                foot_y + 2,
                foot_z,
                cave_air(),
                cave_air(),
                false,
            );
        }
        StrongholdSmallDoorType::WoodDoor => {
            let stone_bricks = stone_bricks();
            placer.place_block(stone_bricks, foot_x, foot_y, foot_z);
            placer.place_block(stone_bricks, foot_x, foot_y + 1, foot_z);
            placer.place_block(stone_bricks, foot_x, foot_y + 2, foot_z);
            placer.place_block(stone_bricks, foot_x + 1, foot_y + 2, foot_z);
            placer.place_block(stone_bricks, foot_x + 2, foot_y + 2, foot_z);
            placer.place_block(stone_bricks, foot_x + 2, foot_y + 1, foot_z);
            placer.place_block(stone_bricks, foot_x + 2, foot_y, foot_z);
            let door = vanilla_blocks::OAK_DOOR.default_state();
            placer.place_block(door, foot_x + 1, foot_y, foot_z);
            placer.place_block(upper_door(door), foot_x + 1, foot_y + 1, foot_z);
        }
        StrongholdSmallDoorType::Grates => {
            let air = cave_air();
            placer.place_block(air, foot_x + 1, foot_y, foot_z);
            placer.place_block(air, foot_x + 1, foot_y + 1, foot_z);
            let west_bars = iron_bars(CONNECT_WEST);
            placer.place_block(west_bars, foot_x, foot_y, foot_z);
            placer.place_block(west_bars, foot_x, foot_y + 1, foot_z);
            let east_west_bars = iron_bars(CONNECT_EAST | CONNECT_WEST);
            placer.place_block(east_west_bars, foot_x, foot_y + 2, foot_z);
            placer.place_block(east_west_bars, foot_x + 1, foot_y + 2, foot_z);
            placer.place_block(east_west_bars, foot_x + 2, foot_y + 2, foot_z);
            let east_bars = iron_bars(CONNECT_EAST);
            placer.place_block(east_bars, foot_x + 2, foot_y + 1, foot_z);
            placer.place_block(east_bars, foot_x + 2, foot_y, foot_z);
        }
        StrongholdSmallDoorType::IronDoor => {
            let stone_bricks = stone_bricks();
            placer.place_block(stone_bricks, foot_x, foot_y, foot_z);
            placer.place_block(stone_bricks, foot_x, foot_y + 1, foot_z);
            placer.place_block(stone_bricks, foot_x, foot_y + 2, foot_z);
            placer.place_block(stone_bricks, foot_x + 1, foot_y + 2, foot_z);
            placer.place_block(stone_bricks, foot_x + 2, foot_y + 2, foot_z);
            placer.place_block(stone_bricks, foot_x + 2, foot_y + 1, foot_z);
            placer.place_block(stone_bricks, foot_x + 2, foot_y, foot_z);
            let door = vanilla_blocks::IRON_DOOR.default_state();
            placer.place_block(door, foot_x + 1, foot_y, foot_z);
            placer.place_block(upper_door(door), foot_x + 1, foot_y + 1, foot_z);
            placer.place_block(
                stone_button(Direction::North),
                foot_x + 2,
                foot_y + 1,
                foot_z + 1,
            );
            placer.place_block(
                stone_button(Direction::South),
                foot_x + 2,
                foot_y + 1,
                foot_z - 1,
            );
        }
    }
}

fn place_chest_corridor(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
    has_placed_chest: &mut bool,
) {
    stronghold_box(placer, random, 0, 0, 0, 4, 4, 6, true);
    generate_small_door(placer, entry_door, 1, 1, 0);
    generate_small_door(placer, StrongholdSmallDoorType::Opening, 1, 1, 6);
    let stone_bricks = stone_bricks();
    placer.generate_box(3, 1, 2, 3, 1, 4, stone_bricks, stone_bricks, false);
    let slab = smooth_stone_slab();
    placer.place_block(slab, 3, 1, 1);
    placer.place_block(slab, 3, 1, 5);
    placer.place_block(slab, 3, 2, 2);
    placer.place_block(slab, 3, 2, 4);

    for z in 2..=4 {
        placer.place_block(slab, 2, 1, z);
    }

    let chest_pos = placer.world_pos(3, 2, 3);
    if !*has_placed_chest && placer.clip().is_inside(chest_pos) {
        *has_placed_chest = true;
        let _ = placer.create_chest(random, 3, 2, 3, STRONGHOLD_CORRIDOR_LOOT);
    }
}

fn place_filler_corridor(placer: &mut ScatteredFeaturePlacer<'_, '_>, steps: i32) {
    let stone_bricks = stone_bricks();
    let air = cave_air();
    for i in 0..steps {
        placer.place_block(stone_bricks, 0, 0, i);
        placer.place_block(stone_bricks, 1, 0, i);
        placer.place_block(stone_bricks, 2, 0, i);
        placer.place_block(stone_bricks, 3, 0, i);
        placer.place_block(stone_bricks, 4, 0, i);

        for y in 1..=3 {
            placer.place_block(stone_bricks, 0, y, i);
            placer.place_block(air, 1, y, i);
            placer.place_block(air, 2, y, i);
            placer.place_block(air, 3, y, i);
            placer.place_block(stone_bricks, 4, y, i);
        }

        placer.place_block(stone_bricks, 0, 4, i);
        placer.place_block(stone_bricks, 1, 4, i);
        placer.place_block(stone_bricks, 2, 4, i);
        placer.place_block(stone_bricks, 3, 4, i);
        placer.place_block(stone_bricks, 4, 4, i);
    }
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "straight transcription of vanilla StrongholdPieces.FiveCrossing"
)]
fn place_five_crossing(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
    left_low: bool,
    left_high: bool,
    right_low: bool,
    right_high: bool,
) {
    stronghold_box(placer, random, 0, 0, 0, 9, 8, 10, true);
    generate_small_door(placer, entry_door, 4, 3, 0);
    let air = cave_air();
    if left_low {
        placer.generate_box(0, 3, 1, 0, 5, 3, air, air, false);
    }
    if right_low {
        placer.generate_box(9, 3, 1, 9, 5, 3, air, air, false);
    }
    if left_high {
        placer.generate_box(0, 5, 7, 0, 7, 9, air, air, false);
    }
    if right_high {
        placer.generate_box(9, 5, 7, 9, 7, 9, air, air, false);
    }

    placer.generate_box(5, 1, 10, 7, 3, 10, air, air, false);
    stronghold_box(placer, random, 1, 2, 1, 8, 2, 6, false);
    stronghold_box(placer, random, 4, 1, 5, 4, 4, 9, false);
    stronghold_box(placer, random, 8, 1, 5, 8, 4, 9, false);
    stronghold_box(placer, random, 1, 4, 7, 3, 4, 9, false);
    stronghold_box(placer, random, 1, 3, 5, 3, 3, 6, false);
    let slab = smooth_stone_slab();
    placer.generate_box(1, 3, 4, 3, 3, 4, slab, slab, false);
    placer.generate_box(1, 4, 6, 3, 4, 6, slab, slab, false);
    stronghold_box(placer, random, 5, 1, 7, 7, 1, 8, false);
    placer.generate_box(5, 1, 9, 7, 1, 9, slab, slab, false);
    placer.generate_box(5, 2, 7, 7, 2, 7, slab, slab, false);
    placer.generate_box(4, 5, 7, 4, 5, 9, slab, slab, false);
    placer.generate_box(8, 5, 7, 8, 5, 9, slab, slab, false);
    let double_slab = double_smooth_stone_slab();
    placer.generate_box(5, 5, 7, 7, 5, 9, double_slab, double_slab, false);
    placer.place_block(wall_torch(Direction::South), 6, 5, 6);
}

fn place_turn(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
    is_left_turn: bool,
    orientation: Direction,
) {
    stronghold_box(placer, random, 0, 0, 0, 4, 4, 4, true);
    generate_small_door(placer, entry_door, 1, 1, 0);
    let opens_west = if is_left_turn {
        matches!(orientation, Direction::North | Direction::East)
    } else {
        !matches!(orientation, Direction::North | Direction::East)
    };
    let air = cave_air();
    if opens_west {
        placer.generate_box(0, 1, 1, 0, 3, 3, air, air, false);
    } else {
        placer.generate_box(4, 1, 1, 4, 3, 3, air, air, false);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "straight transcription of vanilla StrongholdPieces.Library"
)]
fn place_library(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
    is_tall: bool,
) {
    let current_height = if is_tall { 11 } else { 6 };
    stronghold_box(placer, random, 0, 0, 0, 13, current_height - 1, 14, true);
    generate_small_door(placer, entry_door, 4, 1, 0);
    let cobweb = vanilla_blocks::COBWEB.default_state();
    generate_maybe_box(placer, random, 0.07, 2, 1, 1, 11, 4, 13, cobweb, cobweb);
    let oak_planks = vanilla_blocks::OAK_PLANKS.default_state();
    let bookshelf = vanilla_blocks::BOOKSHELF.default_state();

    for depth in 1..=13 {
        if (depth - 1) % 4 == 0 {
            placer.generate_box(1, 1, depth, 1, 4, depth, oak_planks, oak_planks, false);
            placer.generate_box(12, 1, depth, 12, 4, depth, oak_planks, oak_planks, false);
            placer.place_block(wall_torch(Direction::East), 2, 3, depth);
            placer.place_block(wall_torch(Direction::West), 11, 3, depth);
            if is_tall {
                placer.generate_box(1, 6, depth, 1, 9, depth, oak_planks, oak_planks, false);
                placer.generate_box(12, 6, depth, 12, 9, depth, oak_planks, oak_planks, false);
            }
        } else {
            placer.generate_box(1, 1, depth, 1, 4, depth, bookshelf, bookshelf, false);
            placer.generate_box(12, 1, depth, 12, 4, depth, bookshelf, bookshelf, false);
            if is_tall {
                placer.generate_box(1, 6, depth, 1, 9, depth, bookshelf, bookshelf, false);
                placer.generate_box(12, 6, depth, 12, 9, depth, bookshelf, bookshelf, false);
            }
        }
    }

    for dx in (3..12).step_by(2) {
        placer.generate_box(3, 1, dx, 4, 3, dx, bookshelf, bookshelf, false);
        placer.generate_box(6, 1, dx, 7, 3, dx, bookshelf, bookshelf, false);
        placer.generate_box(9, 1, dx, 10, 3, dx, bookshelf, bookshelf, false);
    }

    if is_tall {
        placer.generate_box(1, 5, 1, 3, 5, 13, oak_planks, oak_planks, false);
        placer.generate_box(10, 5, 1, 12, 5, 13, oak_planks, oak_planks, false);
        placer.generate_box(4, 5, 1, 9, 5, 2, oak_planks, oak_planks, false);
        placer.generate_box(4, 5, 12, 9, 5, 13, oak_planks, oak_planks, false);
        placer.place_block(oak_planks, 9, 5, 11);
        placer.place_block(oak_planks, 8, 5, 11);
        placer.place_block(oak_planks, 9, 5, 10);
        let west_east_fence = oak_fence(CONNECT_WEST | CONNECT_EAST);
        let north_south_fence = oak_fence(CONNECT_NORTH | CONNECT_SOUTH);
        placer.generate_box(
            3,
            6,
            3,
            3,
            6,
            11,
            north_south_fence,
            north_south_fence,
            false,
        );
        placer.generate_box(
            10,
            6,
            3,
            10,
            6,
            9,
            north_south_fence,
            north_south_fence,
            false,
        );
        placer.generate_box(4, 6, 2, 9, 6, 2, west_east_fence, west_east_fence, false);
        placer.generate_box(4, 6, 12, 7, 6, 12, west_east_fence, west_east_fence, false);
        placer.place_block(oak_fence(CONNECT_NORTH | CONNECT_EAST), 3, 6, 2);
        placer.place_block(oak_fence(CONNECT_SOUTH | CONNECT_EAST), 3, 6, 12);
        placer.place_block(oak_fence(CONNECT_NORTH | CONNECT_WEST), 10, 6, 2);

        for i in 0..=2 {
            placer.place_block(oak_fence(CONNECT_SOUTH | CONNECT_WEST), 8 + i, 6, 12 - i);
            if i != 2 {
                placer.place_block(oak_fence(CONNECT_NORTH | CONNECT_EAST), 8 + i, 6, 11 - i);
            }
        }

        let ladder = ladder(Direction::South);
        for y in 1..=7 {
            placer.place_block(ladder, 10, y, 13);
        }

        let east_fence = oak_fence(CONNECT_EAST);
        let west_fence = oak_fence(CONNECT_WEST);
        placer.place_block(east_fence, 6, 9, 7);
        placer.place_block(west_fence, 7, 9, 7);
        placer.place_block(east_fence, 6, 8, 7);
        placer.place_block(west_fence, 7, 8, 7);
        let all_sides_fence =
            oak_fence(CONNECT_NORTH | CONNECT_SOUTH | CONNECT_WEST | CONNECT_EAST);
        placer.place_block(all_sides_fence, 6, 7, 7);
        placer.place_block(all_sides_fence, 7, 7, 7);
        placer.place_block(east_fence, 5, 7, 7);
        placer.place_block(west_fence, 8, 7, 7);
        placer.place_block(oak_fence(CONNECT_EAST | CONNECT_NORTH), 6, 7, 6);
        placer.place_block(oak_fence(CONNECT_EAST | CONNECT_SOUTH), 6, 7, 8);
        placer.place_block(oak_fence(CONNECT_WEST | CONNECT_NORTH), 7, 7, 6);
        placer.place_block(oak_fence(CONNECT_WEST | CONNECT_SOUTH), 7, 7, 8);
        let torch = vanilla_blocks::TORCH.default_state();
        placer.place_block(torch, 5, 8, 7);
        placer.place_block(torch, 8, 8, 7);
        placer.place_block(torch, 6, 8, 6);
        placer.place_block(torch, 6, 8, 8);
        placer.place_block(torch, 7, 8, 6);
        placer.place_block(torch, 7, 8, 8);
    }

    let _ = placer.create_chest(random, 3, 3, 5, STRONGHOLD_LIBRARY_LOOT);
    if is_tall {
        placer.place_block(cave_air(), 12, 9, 1);
        let _ = placer.create_chest(random, 12, 8, 1, STRONGHOLD_LIBRARY_LOOT);
    }
}

fn place_portal_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    has_placed_spawner: &mut bool,
) {
    stronghold_box(placer, random, 0, 0, 0, 10, 7, 15, false);
    generate_small_door(placer, StrongholdSmallDoorType::Grates, 4, 1, 0);
    stronghold_box(placer, random, 1, 6, 1, 1, 6, 14, false);
    stronghold_box(placer, random, 9, 6, 1, 9, 6, 14, false);
    stronghold_box(placer, random, 2, 6, 1, 8, 6, 2, false);
    stronghold_box(placer, random, 2, 6, 14, 8, 6, 14, false);
    stronghold_box(placer, random, 1, 1, 1, 2, 1, 4, false);
    stronghold_box(placer, random, 8, 1, 1, 9, 1, 4, false);
    let lava = vanilla_blocks::LAVA.default_state();
    placer.generate_box(1, 1, 1, 1, 1, 3, lava, lava, false);
    placer.generate_box(9, 1, 1, 9, 1, 3, lava, lava, false);
    stronghold_box(placer, random, 3, 1, 8, 7, 1, 12, false);
    placer.generate_box(4, 1, 9, 6, 1, 11, lava, lava, false);
    let north_south_bars = iron_bars(CONNECT_NORTH | CONNECT_SOUTH);
    let west_east_bars = iron_bars(CONNECT_WEST | CONNECT_EAST);

    for z in (3..14).step_by(2) {
        placer.generate_box(0, 3, z, 0, 4, z, north_south_bars, north_south_bars, false);
        placer.generate_box(
            10,
            3,
            z,
            10,
            4,
            z,
            north_south_bars,
            north_south_bars,
            false,
        );
    }

    for x in (2..9).step_by(2) {
        placer.generate_box(x, 3, 15, x, 4, 15, west_east_bars, west_east_bars, false);
    }

    let stairs = stone_brick_stairs(Direction::North);
    stronghold_box(placer, random, 4, 1, 5, 6, 1, 7, false);
    stronghold_box(placer, random, 4, 2, 6, 6, 2, 7, false);
    stronghold_box(placer, random, 4, 3, 7, 6, 3, 7, false);

    for x in 4..=6 {
        placer.place_block(stairs, x, 1, 4);
        placer.place_block(stairs, x, 2, 5);
        placer.place_block(stairs, x, 3, 6);
    }

    let mut all_eyes = true;
    let mut eyes = [false; 12];
    for eye in &mut eyes {
        *eye = random.next_f32() > 0.9;
        all_eyes &= *eye;
    }

    placer.place_block(end_portal_frame(Direction::North, eyes[0]), 4, 3, 8);
    placer.place_block(end_portal_frame(Direction::North, eyes[1]), 5, 3, 8);
    placer.place_block(end_portal_frame(Direction::North, eyes[2]), 6, 3, 8);
    placer.place_block(end_portal_frame(Direction::South, eyes[3]), 4, 3, 12);
    placer.place_block(end_portal_frame(Direction::South, eyes[4]), 5, 3, 12);
    placer.place_block(end_portal_frame(Direction::South, eyes[5]), 6, 3, 12);
    placer.place_block(end_portal_frame(Direction::East, eyes[6]), 3, 3, 9);
    placer.place_block(end_portal_frame(Direction::East, eyes[7]), 3, 3, 10);
    placer.place_block(end_portal_frame(Direction::East, eyes[8]), 3, 3, 11);
    placer.place_block(end_portal_frame(Direction::West, eyes[9]), 7, 3, 9);
    placer.place_block(end_portal_frame(Direction::West, eyes[10]), 7, 3, 10);
    placer.place_block(end_portal_frame(Direction::West, eyes[11]), 7, 3, 11);

    if all_eyes {
        let portal = vanilla_blocks::END_PORTAL.default_state();
        placer.place_block(portal, 4, 3, 9);
        placer.place_block(portal, 5, 3, 9);
        placer.place_block(portal, 6, 3, 9);
        placer.place_block(portal, 4, 3, 10);
        placer.place_block(portal, 5, 3, 10);
        placer.place_block(portal, 6, 3, 10);
        placer.place_block(portal, 4, 3, 11);
        placer.place_block(portal, 5, 3, 11);
        placer.place_block(portal, 6, 3, 11);
    }

    let spawner_pos = placer.world_pos(5, 3, 6);
    if !*has_placed_spawner && placer.clip().is_inside(spawner_pos) {
        *has_placed_spawner = true;
        let _ = placer.create_spawner(5, 3, 6, SILVERFISH_ENTITY);
    }
}

fn place_prison_hall(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
) {
    stronghold_box(placer, random, 0, 0, 0, 8, 4, 10, true);
    generate_small_door(placer, entry_door, 1, 1, 0);
    placer.generate_box(1, 1, 10, 3, 3, 10, cave_air(), cave_air(), false);
    stronghold_box(placer, random, 4, 1, 1, 4, 3, 1, false);
    stronghold_box(placer, random, 4, 1, 3, 4, 3, 3, false);
    stronghold_box(placer, random, 4, 1, 7, 4, 3, 7, false);
    stronghold_box(placer, random, 4, 1, 9, 4, 3, 9, false);

    let north_south_bars = iron_bars(CONNECT_NORTH | CONNECT_SOUTH);
    let north_south_east_bars = iron_bars(CONNECT_NORTH | CONNECT_SOUTH | CONNECT_EAST);
    let west_east_bars = iron_bars(CONNECT_WEST | CONNECT_EAST);
    for y in 1..=3 {
        placer.place_block(north_south_bars, 4, y, 4);
        placer.place_block(north_south_east_bars, 4, y, 5);
        placer.place_block(north_south_bars, 4, y, 6);
        placer.place_block(west_east_bars, 5, y, 5);
        placer.place_block(west_east_bars, 6, y, 5);
        placer.place_block(west_east_bars, 7, y, 5);
    }

    placer.place_block(north_south_bars, 4, 3, 2);
    placer.place_block(north_south_bars, 4, 3, 8);
    let door_bottom = vanilla_blocks::IRON_DOOR
        .default_state()
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::West);
    let door_top = upper_door(door_bottom);
    placer.place_block(door_bottom, 4, 1, 2);
    placer.place_block(door_top, 4, 2, 2);
    placer.place_block(door_bottom, 4, 1, 8);
    placer.place_block(door_top, 4, 2, 8);
}

fn place_room_crossing(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
    crossing_type: i32,
) {
    stronghold_box(placer, random, 0, 0, 0, 10, 6, 10, true);
    generate_small_door(placer, entry_door, 4, 1, 0);
    let air = cave_air();
    placer.generate_box(4, 1, 10, 6, 3, 10, air, air, false);
    placer.generate_box(0, 1, 4, 0, 3, 6, air, air, false);
    placer.generate_box(10, 1, 4, 10, 3, 6, air, air, false);
    match crossing_type {
        0 => {
            let stone_bricks = stone_bricks();
            placer.place_block(stone_bricks, 5, 1, 5);
            placer.place_block(stone_bricks, 5, 2, 5);
            placer.place_block(stone_bricks, 5, 3, 5);
            placer.place_block(wall_torch(Direction::West), 4, 3, 5);
            placer.place_block(wall_torch(Direction::East), 6, 3, 5);
            placer.place_block(wall_torch(Direction::South), 5, 3, 4);
            placer.place_block(wall_torch(Direction::North), 5, 3, 6);
            let slab = smooth_stone_slab();
            placer.place_block(slab, 4, 1, 4);
            placer.place_block(slab, 4, 1, 5);
            placer.place_block(slab, 4, 1, 6);
            placer.place_block(slab, 6, 1, 4);
            placer.place_block(slab, 6, 1, 5);
            placer.place_block(slab, 6, 1, 6);
            placer.place_block(slab, 5, 1, 4);
            placer.place_block(slab, 5, 1, 6);
        }
        1 => {
            let stone_bricks = stone_bricks();
            for i in 0..5 {
                placer.place_block(stone_bricks, 3, 1, 3 + i);
                placer.place_block(stone_bricks, 7, 1, 3 + i);
                placer.place_block(stone_bricks, 3 + i, 1, 3);
                placer.place_block(stone_bricks, 3 + i, 1, 7);
            }
            placer.place_block(stone_bricks, 5, 1, 5);
            placer.place_block(stone_bricks, 5, 2, 5);
            placer.place_block(stone_bricks, 5, 3, 5);
            placer.place_block(vanilla_blocks::WATER.default_state(), 5, 4, 5);
        }
        2 => {
            let cobblestone = vanilla_blocks::COBBLESTONE.default_state();
            for z in 1..=9 {
                placer.place_block(cobblestone, 1, 3, z);
                placer.place_block(cobblestone, 9, 3, z);
            }
            for x in 1..=9 {
                placer.place_block(cobblestone, x, 3, 1);
                placer.place_block(cobblestone, x, 3, 9);
            }
            placer.place_block(cobblestone, 5, 1, 4);
            placer.place_block(cobblestone, 5, 1, 6);
            placer.place_block(cobblestone, 5, 3, 4);
            placer.place_block(cobblestone, 5, 3, 6);
            placer.place_block(cobblestone, 4, 1, 5);
            placer.place_block(cobblestone, 6, 1, 5);
            placer.place_block(cobblestone, 4, 3, 5);
            placer.place_block(cobblestone, 6, 3, 5);
            for y in 1..=3 {
                placer.place_block(cobblestone, 4, y, 4);
                placer.place_block(cobblestone, 6, y, 4);
                placer.place_block(cobblestone, 4, y, 6);
                placer.place_block(cobblestone, 6, y, 6);
            }
            placer.place_block(vanilla_blocks::WALL_TORCH.default_state(), 5, 3, 5);
            let oak_planks = vanilla_blocks::OAK_PLANKS.default_state();
            for z in 2..=8 {
                placer.place_block(oak_planks, 2, 3, z);
                placer.place_block(oak_planks, 3, 3, z);
                if z <= 3 || z >= 7 {
                    placer.place_block(oak_planks, 4, 3, z);
                    placer.place_block(oak_planks, 5, 3, z);
                    placer.place_block(oak_planks, 6, 3, z);
                }
                placer.place_block(oak_planks, 7, 3, z);
                placer.place_block(oak_planks, 8, 3, z);
            }
            let ladder = ladder(Direction::West);
            placer.place_block(ladder, 9, 1, 3);
            placer.place_block(ladder, 9, 2, 3);
            placer.place_block(ladder, 9, 3, 3);
            let _ = placer.create_chest(random, 3, 4, 8, STRONGHOLD_CROSSING_LOOT);
        }
        _ => {}
    }
}

fn place_stairs_down(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
) {
    stronghold_box(placer, random, 0, 0, 0, 4, 10, 4, true);
    generate_small_door(placer, entry_door, 1, 7, 0);
    generate_small_door(placer, StrongholdSmallDoorType::Opening, 1, 1, 4);
    let stone_bricks = stone_bricks();
    let slab = smooth_stone_slab();
    placer.place_block(stone_bricks, 2, 6, 1);
    placer.place_block(stone_bricks, 1, 5, 1);
    placer.place_block(slab, 1, 6, 1);
    placer.place_block(stone_bricks, 1, 5, 2);
    placer.place_block(stone_bricks, 1, 4, 3);
    placer.place_block(slab, 1, 5, 3);
    placer.place_block(stone_bricks, 2, 4, 3);
    placer.place_block(stone_bricks, 3, 3, 3);
    placer.place_block(slab, 3, 4, 3);
    placer.place_block(stone_bricks, 3, 3, 2);
    placer.place_block(stone_bricks, 3, 2, 1);
    placer.place_block(slab, 3, 3, 1);
    placer.place_block(stone_bricks, 2, 2, 1);
    placer.place_block(stone_bricks, 1, 1, 1);
    placer.place_block(slab, 1, 2, 1);
    placer.place_block(stone_bricks, 1, 1, 2);
    placer.place_block(slab, 1, 1, 3);
}

fn place_straight(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
    left_child: bool,
    right_child: bool,
) {
    stronghold_box(placer, random, 0, 0, 0, 4, 4, 6, true);
    generate_small_door(placer, entry_door, 1, 1, 0);
    generate_small_door(placer, StrongholdSmallDoorType::Opening, 1, 1, 6);
    maybe_generate_block(placer, random, 0.1, 1, 2, 1, wall_torch(Direction::East));
    maybe_generate_block(placer, random, 0.1, 3, 2, 1, wall_torch(Direction::West));
    maybe_generate_block(placer, random, 0.1, 1, 2, 5, wall_torch(Direction::East));
    maybe_generate_block(placer, random, 0.1, 3, 2, 5, wall_torch(Direction::West));
    let air = cave_air();
    if left_child {
        placer.generate_box(0, 1, 2, 0, 3, 4, air, air, false);
    }
    if right_child {
        placer.generate_box(4, 1, 2, 4, 3, 4, air, air, false);
    }
}

fn place_straight_stairs_down(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    entry_door: StrongholdSmallDoorType,
) {
    stronghold_box(placer, random, 0, 0, 0, 4, 10, 7, true);
    generate_small_door(placer, entry_door, 1, 7, 0);
    generate_small_door(placer, StrongholdSmallDoorType::Opening, 1, 1, 7);
    let stairs = cobblestone_stairs(Direction::South);
    let stone_bricks = stone_bricks();
    for i in 0..6 {
        placer.place_block(stairs, 1, 6 - i, 1 + i);
        placer.place_block(stairs, 2, 6 - i, 1 + i);
        placer.place_block(stairs, 3, 6 - i, 1 + i);
        if i < 5 {
            placer.place_block(stone_bricks, 1, 5 - i, 1 + i);
            placer.place_block(stone_bricks, 2, 5 - i, 1 + i);
            placer.place_block(stone_bricks, 3, 5 - i, 1 + i);
        }
    }
}
