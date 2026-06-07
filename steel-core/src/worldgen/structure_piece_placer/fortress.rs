use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{Registry, vanilla_blocks};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockStateId, BoundingBox, Direction};

use crate::worldgen::region::WorldGenRegion;
use steel_worldgen::structure::fortress::FortressPieceData;

use super::{StructurePiecePlacer, scattered_feature::ScatteredFeaturePlacer};

const NETHER_BRIDGE_LOOT: &str = "minecraft:chests/nether_bridge";
const BLAZE_ENTITY: &str = "minecraft:blaze";

impl StructurePiecePlacer {
    pub(super) fn place_nether_fortress_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounding_box: BoundingBox,
        orientation: Option<Direction>,
        data: &mut FortressPieceData,
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
            FortressPieceData::BridgeCrossing => place_bridge_crossing(&mut placer),
            FortressPieceData::BridgeEndFiller { self_seed } => {
                place_bridge_end_filler(&mut placer, *self_seed);
            }
            FortressPieceData::BridgeStraight => place_bridge_straight(&mut placer),
            FortressPieceData::CastleCorridorStairs => place_castle_corridor_stairs(&mut placer),
            FortressPieceData::CastleCorridorTBalcony => {
                place_castle_corridor_t_balcony(&mut placer);
            }
            FortressPieceData::CastleEntrance => place_castle_entrance(&mut placer),
            FortressPieceData::CastleSmallCorridorCrossing => {
                place_castle_small_corridor_crossing(&mut placer);
            }
            FortressPieceData::CastleSmallCorridorLeftTurn { is_needing_chest } => {
                place_castle_small_corridor_left_turn(&mut placer, random, is_needing_chest);
            }
            FortressPieceData::CastleSmallCorridor => place_castle_small_corridor(&mut placer),
            FortressPieceData::CastleSmallCorridorRightTurn { is_needing_chest } => {
                place_castle_small_corridor_right_turn(&mut placer, random, is_needing_chest);
            }
            FortressPieceData::CastleStalkRoom => place_castle_stalk_room(&mut placer),
            FortressPieceData::MonsterThrone { has_placed_spawner } => {
                place_monster_throne(&mut placer, has_placed_spawner);
            }
            FortressPieceData::RoomCrossing => place_room_crossing(&mut placer),
            FortressPieceData::StairsRoom => place_stairs_room(&mut placer),
        }
        true
    }
}

fn nether_bricks() -> BlockStateId {
    vanilla_blocks::NETHER_BRICKS.default_state()
}

fn air() -> BlockStateId {
    vanilla_blocks::AIR.default_state()
}

fn lava() -> BlockStateId {
    vanilla_blocks::LAVA.default_state()
}

fn soul_sand() -> BlockStateId {
    vanilla_blocks::SOUL_SAND.default_state()
}

fn nether_wart() -> BlockStateId {
    vanilla_blocks::NETHER_WART.default_state()
}

fn stairs(facing: Direction) -> BlockStateId {
    vanilla_blocks::NETHER_BRICK_STAIRS
        .default_state()
        .set_value(&BlockStateProperties::FACING, facing)
}

const FENCE_NORTH: u8 = 1;
const FENCE_EAST: u8 = 2;
const FENCE_SOUTH: u8 = 4;
const FENCE_WEST: u8 = 8;

fn fence(mask: u8) -> BlockStateId {
    vanilla_blocks::NETHER_BRICK_FENCE
        .default_state()
        .set_value(&BlockStateProperties::NORTH, mask & FENCE_NORTH != 0)
        .set_value(&BlockStateProperties::EAST, mask & FENCE_EAST != 0)
        .set_value(&BlockStateProperties::SOUTH, mask & FENCE_SOUTH != 0)
        .set_value(&BlockStateProperties::WEST, mask & FENCE_WEST != 0)
}

fn fence_ns() -> BlockStateId {
    fence(FENCE_NORTH | FENCE_SOUTH)
}

fn fence_we() -> BlockStateId {
    fence(FENCE_EAST | FENCE_WEST)
}

fn place_bridge_crossing(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    placer.generate_box(7, 3, 0, 11, 4, 18, bricks, bricks, false);
    placer.generate_box(0, 3, 7, 18, 4, 11, bricks, bricks, false);
    placer.generate_box(8, 5, 0, 10, 7, 18, air, air, false);
    placer.generate_box(0, 5, 8, 18, 7, 10, air, air, false);
    placer.generate_box(7, 5, 0, 7, 5, 7, bricks, bricks, false);
    placer.generate_box(7, 5, 11, 7, 5, 18, bricks, bricks, false);
    placer.generate_box(11, 5, 0, 11, 5, 7, bricks, bricks, false);
    placer.generate_box(11, 5, 11, 11, 5, 18, bricks, bricks, false);
    placer.generate_box(0, 5, 7, 7, 5, 7, bricks, bricks, false);
    placer.generate_box(11, 5, 7, 18, 5, 7, bricks, bricks, false);
    placer.generate_box(0, 5, 11, 7, 5, 11, bricks, bricks, false);
    placer.generate_box(11, 5, 11, 18, 5, 11, bricks, bricks, false);
    placer.generate_box(7, 2, 0, 11, 2, 5, bricks, bricks, false);
    placer.generate_box(7, 2, 13, 11, 2, 18, bricks, bricks, false);
    placer.generate_box(7, 0, 0, 11, 1, 3, bricks, bricks, false);
    placer.generate_box(7, 0, 15, 11, 1, 18, bricks, bricks, false);

    for x in 7..=11 {
        for z in 0..=2 {
            placer.fill_column_down(bricks, x, -1, z);
            placer.fill_column_down(bricks, x, -1, 18 - z);
        }
    }

    placer.generate_box(0, 2, 7, 5, 2, 11, bricks, bricks, false);
    placer.generate_box(13, 2, 7, 18, 2, 11, bricks, bricks, false);
    placer.generate_box(0, 0, 7, 3, 1, 11, bricks, bricks, false);
    placer.generate_box(15, 0, 7, 18, 1, 11, bricks, bricks, false);

    for x in 0..=2 {
        for z in 7..=11 {
            placer.fill_column_down(bricks, x, -1, z);
            placer.fill_column_down(bricks, 18 - x, -1, z);
        }
    }
}

fn place_bridge_end_filler(placer: &mut ScatteredFeaturePlacer<'_, '_>, self_seed: i32) {
    let mut self_random = LegacyRandom::from_seed(i64::from(self_seed) as u64);
    let bricks = nether_bricks();

    for x in 0..=4 {
        for y in 3..=4 {
            let z = self_random.next_i32_bounded(8);
            placer.generate_box(x, y, 0, x, y, z, bricks, bricks, false);
        }
    }

    let z = self_random.next_i32_bounded(8);
    placer.generate_box(0, 5, 0, 0, 5, z, bricks, bricks, false);
    let z = self_random.next_i32_bounded(8);
    placer.generate_box(4, 5, 0, 4, 5, z, bricks, bricks, false);

    for x in 0..=4 {
        let z = self_random.next_i32_bounded(5);
        placer.generate_box(x, 2, 0, x, 2, z, bricks, bricks, false);
    }

    for x in 0..=4 {
        for y in 0..=1 {
            let z = self_random.next_i32_bounded(3);
            placer.generate_box(x, y, 0, x, y, z, bricks, bricks, false);
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "straight transcription of vanilla NetherFortressPieces.BridgeStraight"
)]
fn place_bridge_straight(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    placer.generate_box(0, 3, 0, 4, 4, 18, bricks, bricks, false);
    placer.generate_box(1, 5, 0, 3, 7, 18, air, air, false);
    placer.generate_box(0, 5, 0, 0, 5, 18, bricks, bricks, false);
    placer.generate_box(4, 5, 0, 4, 5, 18, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 4, 2, 5, bricks, bricks, false);
    placer.generate_box(0, 2, 13, 4, 2, 18, bricks, bricks, false);
    placer.generate_box(0, 0, 0, 4, 1, 3, bricks, bricks, false);
    placer.generate_box(0, 0, 15, 4, 1, 18, bricks, bricks, false);

    for x in 0..=4 {
        for z in 0..=2 {
            placer.fill_column_down(bricks, x, -1, z);
            placer.fill_column_down(bricks, x, -1, 18 - z);
        }
    }

    let north_south_east_fence = fence(FENCE_NORTH | FENCE_SOUTH | FENCE_EAST);
    let north_south_west_fence = fence(FENCE_NORTH | FENCE_SOUTH | FENCE_WEST);
    placer.generate_box(
        0,
        1,
        1,
        0,
        4,
        1,
        north_south_east_fence,
        north_south_east_fence,
        false,
    );
    placer.generate_box(
        0,
        3,
        4,
        0,
        4,
        4,
        north_south_east_fence,
        north_south_east_fence,
        false,
    );
    placer.generate_box(
        0,
        3,
        14,
        0,
        4,
        14,
        north_south_east_fence,
        north_south_east_fence,
        false,
    );
    placer.generate_box(
        0,
        1,
        17,
        0,
        4,
        17,
        north_south_east_fence,
        north_south_east_fence,
        false,
    );
    placer.generate_box(
        4,
        1,
        1,
        4,
        4,
        1,
        north_south_west_fence,
        north_south_west_fence,
        false,
    );
    placer.generate_box(
        4,
        3,
        4,
        4,
        4,
        4,
        north_south_west_fence,
        north_south_west_fence,
        false,
    );
    placer.generate_box(
        4,
        3,
        14,
        4,
        4,
        14,
        north_south_west_fence,
        north_south_west_fence,
        false,
    );
    placer.generate_box(
        4,
        1,
        17,
        4,
        4,
        17,
        north_south_west_fence,
        north_south_west_fence,
        false,
    );
}

fn place_castle_corridor_stairs(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    let stairs = stairs(Direction::South);
    let ns_fence = fence_ns();

    for step in 0..=9 {
        let floor = 1.max(7 - step);
        let roof = (floor + 5).max(14 - step).min(13);
        placer.generate_box(0, 0, step, 4, floor, step, bricks, bricks, false);
        placer.generate_box(1, floor + 1, step, 3, roof - 1, step, air, air, false);
        if step <= 6 {
            placer.place_block(stairs, 1, floor + 1, step);
            placer.place_block(stairs, 2, floor + 1, step);
            placer.place_block(stairs, 3, floor + 1, step);
        }

        placer.generate_box(0, roof, step, 4, roof, step, bricks, bricks, false);
        placer.generate_box(0, floor + 1, step, 0, roof - 1, step, bricks, bricks, false);
        placer.generate_box(4, floor + 1, step, 4, roof - 1, step, bricks, bricks, false);
        if (step & 1) == 0 {
            placer.generate_box(
                0,
                floor + 2,
                step,
                0,
                floor + 3,
                step,
                ns_fence,
                ns_fence,
                false,
            );
            placer.generate_box(
                4,
                floor + 2,
                step,
                4,
                floor + 3,
                step,
                ns_fence,
                ns_fence,
                false,
            );
        }

        for x in 0..=4 {
            placer.fill_column_down(bricks, x, -1, step);
        }
    }
}

fn place_castle_corridor_t_balcony(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    let ns_fence = fence_ns();
    let we_fence = fence_we();
    placer.generate_box(0, 0, 0, 8, 1, 8, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 8, 5, 8, air, air, false);
    placer.generate_box(0, 6, 0, 8, 6, 5, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 2, 5, 0, bricks, bricks, false);
    placer.generate_box(6, 2, 0, 8, 5, 0, bricks, bricks, false);
    placer.generate_box(1, 3, 0, 1, 4, 0, we_fence, we_fence, false);
    placer.generate_box(7, 3, 0, 7, 4, 0, we_fence, we_fence, false);
    placer.generate_box(0, 2, 4, 8, 2, 8, bricks, bricks, false);
    placer.generate_box(1, 1, 4, 2, 2, 4, air, air, false);
    placer.generate_box(6, 1, 4, 7, 2, 4, air, air, false);
    placer.generate_box(1, 3, 8, 7, 3, 8, we_fence, we_fence, false);
    placer.place_block(fence(FENCE_EAST | FENCE_SOUTH), 0, 3, 8);
    placer.place_block(fence(FENCE_SOUTH | FENCE_WEST), 8, 3, 8);
    placer.generate_box(0, 3, 6, 0, 3, 7, ns_fence, ns_fence, false);
    placer.generate_box(8, 3, 6, 8, 3, 7, ns_fence, ns_fence, false);
    placer.generate_box(0, 3, 4, 0, 5, 5, bricks, bricks, false);
    placer.generate_box(8, 3, 4, 8, 5, 5, bricks, bricks, false);
    placer.generate_box(1, 3, 5, 2, 5, 5, bricks, bricks, false);
    placer.generate_box(6, 3, 5, 7, 5, 5, bricks, bricks, false);
    placer.generate_box(1, 4, 5, 1, 5, 5, we_fence, we_fence, false);
    placer.generate_box(7, 4, 5, 7, 5, 5, we_fence, we_fence, false);

    for z in 0..=5 {
        for x in 0..=8 {
            placer.fill_column_down(bricks, x, -1, z);
        }
    }
}

fn place_castle_entrance(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    let we_fence = fence_we();
    let ns_fence = fence_ns();
    placer.generate_box(0, 3, 0, 12, 4, 12, bricks, bricks, false);
    placer.generate_box(0, 5, 0, 12, 13, 12, air, air, false);
    placer.generate_box(0, 5, 0, 1, 12, 12, bricks, bricks, false);
    placer.generate_box(11, 5, 0, 12, 12, 12, bricks, bricks, false);
    placer.generate_box(2, 5, 11, 4, 12, 12, bricks, bricks, false);
    placer.generate_box(8, 5, 11, 10, 12, 12, bricks, bricks, false);
    placer.generate_box(5, 9, 11, 7, 12, 12, bricks, bricks, false);
    placer.generate_box(2, 5, 0, 4, 12, 1, bricks, bricks, false);
    placer.generate_box(8, 5, 0, 10, 12, 1, bricks, bricks, false);
    placer.generate_box(5, 9, 0, 7, 12, 1, bricks, bricks, false);
    placer.generate_box(2, 11, 2, 10, 12, 10, bricks, bricks, false);
    placer.generate_box(5, 8, 0, 7, 8, 0, fence(0), fence(0), false);

    for i in (1..=11).step_by(2) {
        placer.generate_box(i, 10, 0, i, 11, 0, we_fence, we_fence, false);
        placer.generate_box(i, 10, 12, i, 11, 12, we_fence, we_fence, false);
        placer.generate_box(0, 10, i, 0, 11, i, ns_fence, ns_fence, false);
        placer.generate_box(12, 10, i, 12, 11, i, ns_fence, ns_fence, false);
        placer.place_block(bricks, i, 13, 0);
        placer.place_block(bricks, i, 13, 12);
        placer.place_block(bricks, 0, 13, i);
        placer.place_block(bricks, 12, 13, i);
        if i != 11 {
            placer.place_block(we_fence, i + 1, 13, 0);
            placer.place_block(we_fence, i + 1, 13, 12);
            placer.place_block(ns_fence, 0, 13, i + 1);
            placer.place_block(ns_fence, 12, 13, i + 1);
        }
    }

    placer.place_block(fence(FENCE_NORTH | FENCE_EAST), 0, 13, 0);
    placer.place_block(fence(FENCE_EAST | FENCE_SOUTH), 0, 13, 12);
    placer.place_block(fence(FENCE_SOUTH | FENCE_WEST), 12, 13, 12);
    placer.place_block(fence(FENCE_NORTH | FENCE_WEST), 12, 13, 0);

    for z in (3..=9).step_by(2) {
        placer.generate_box(
            1,
            7,
            z,
            1,
            8,
            z,
            fence(FENCE_NORTH | FENCE_SOUTH | FENCE_WEST),
            fence(FENCE_NORTH | FENCE_SOUTH | FENCE_WEST),
            false,
        );
        placer.generate_box(
            11,
            7,
            z,
            11,
            8,
            z,
            fence(FENCE_NORTH | FENCE_EAST | FENCE_SOUTH),
            fence(FENCE_NORTH | FENCE_EAST | FENCE_SOUTH),
            false,
        );
    }

    place_castle_cross_foundation(placer);
    placer.generate_box(5, 5, 5, 7, 5, 7, bricks, bricks, false);
    placer.generate_box(6, 1, 6, 6, 4, 6, air, air, false);
    placer.place_block(bricks, 6, 0, 6);
    placer.place_block(lava(), 6, 5, 6);
}

fn place_castle_small_corridor_crossing(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    placer.generate_box(0, 0, 0, 4, 1, 4, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 4, 5, 4, air, air, false);
    placer.generate_box(0, 2, 0, 0, 5, 0, bricks, bricks, false);
    placer.generate_box(4, 2, 0, 4, 5, 0, bricks, bricks, false);
    placer.generate_box(0, 2, 4, 0, 5, 4, bricks, bricks, false);
    placer.generate_box(4, 2, 4, 4, 5, 4, bricks, bricks, false);
    placer.generate_box(0, 6, 0, 4, 6, 4, bricks, bricks, false);
    fill_down_square(placer, 4, 4);
}

fn place_castle_small_corridor_left_turn(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    is_needing_chest: &mut bool,
) {
    let bricks = nether_bricks();
    let air = air();
    let we_fence = fence_we();
    let ns_fence = fence_ns();
    placer.generate_box(0, 0, 0, 4, 1, 4, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 4, 5, 4, air, air, false);
    placer.generate_box(4, 2, 0, 4, 5, 4, bricks, bricks, false);
    placer.generate_box(4, 3, 1, 4, 4, 1, ns_fence, ns_fence, false);
    placer.generate_box(4, 3, 3, 4, 4, 3, ns_fence, ns_fence, false);
    placer.generate_box(0, 2, 0, 0, 5, 0, bricks, bricks, false);
    placer.generate_box(0, 2, 4, 3, 5, 4, bricks, bricks, false);
    placer.generate_box(1, 3, 4, 1, 4, 4, we_fence, we_fence, false);
    placer.generate_box(3, 3, 4, 3, 4, 4, we_fence, we_fence, false);
    if *is_needing_chest && placer.clip().is_inside(placer.world_pos(3, 2, 3)) {
        *is_needing_chest = false;
        let _ = placer.create_chest(random, 3, 2, 3, NETHER_BRIDGE_LOOT);
    }
    placer.generate_box(0, 6, 0, 4, 6, 4, bricks, bricks, false);
    fill_down_square(placer, 4, 4);
}

fn place_castle_small_corridor(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    let ns_fence = fence_ns();
    placer.generate_box(0, 0, 0, 4, 1, 4, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 4, 5, 4, air, air, false);
    placer.generate_box(0, 2, 0, 0, 5, 4, bricks, bricks, false);
    placer.generate_box(4, 2, 0, 4, 5, 4, bricks, bricks, false);
    placer.generate_box(0, 3, 1, 0, 4, 1, ns_fence, ns_fence, false);
    placer.generate_box(0, 3, 3, 0, 4, 3, ns_fence, ns_fence, false);
    placer.generate_box(4, 3, 1, 4, 4, 1, ns_fence, ns_fence, false);
    placer.generate_box(4, 3, 3, 4, 4, 3, ns_fence, ns_fence, false);
    placer.generate_box(0, 6, 0, 4, 6, 4, bricks, bricks, false);
    fill_down_square(placer, 4, 4);
}

fn place_castle_small_corridor_right_turn(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    is_needing_chest: &mut bool,
) {
    let bricks = nether_bricks();
    let air = air();
    let we_fence = fence_we();
    let ns_fence = fence_ns();
    placer.generate_box(0, 0, 0, 4, 1, 4, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 4, 5, 4, air, air, false);
    placer.generate_box(0, 2, 0, 0, 5, 4, bricks, bricks, false);
    placer.generate_box(0, 3, 1, 0, 4, 1, ns_fence, ns_fence, false);
    placer.generate_box(0, 3, 3, 0, 4, 3, ns_fence, ns_fence, false);
    placer.generate_box(4, 2, 0, 4, 5, 0, bricks, bricks, false);
    placer.generate_box(1, 2, 4, 4, 5, 4, bricks, bricks, false);
    placer.generate_box(1, 3, 4, 1, 4, 4, we_fence, we_fence, false);
    placer.generate_box(3, 3, 4, 3, 4, 4, we_fence, we_fence, false);
    if *is_needing_chest && placer.clip().is_inside(placer.world_pos(1, 2, 3)) {
        *is_needing_chest = false;
        let _ = placer.create_chest(random, 1, 2, 3, NETHER_BRIDGE_LOOT);
    }
    placer.generate_box(0, 6, 0, 4, 6, 4, bricks, bricks, false);
    fill_down_square(placer, 4, 4);
}

#[expect(
    clippy::too_many_lines,
    reason = "straight transcription of vanilla NetherFortressPieces.CastleStalkRoom"
)]
fn place_castle_stalk_room(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    let we_fence = fence_we();
    let ns_fence = fence_ns();
    let north_south_west_fence = fence(FENCE_NORTH | FENCE_SOUTH | FENCE_WEST);
    let north_south_east_fence = fence(FENCE_NORTH | FENCE_SOUTH | FENCE_EAST);
    placer.generate_box(0, 3, 0, 12, 4, 12, bricks, bricks, false);
    placer.generate_box(0, 5, 0, 12, 13, 12, air, air, false);
    placer.generate_box(0, 5, 0, 1, 12, 12, bricks, bricks, false);
    placer.generate_box(11, 5, 0, 12, 12, 12, bricks, bricks, false);
    placer.generate_box(2, 5, 11, 4, 12, 12, bricks, bricks, false);
    placer.generate_box(8, 5, 11, 10, 12, 12, bricks, bricks, false);
    placer.generate_box(5, 9, 11, 7, 12, 12, bricks, bricks, false);
    placer.generate_box(2, 5, 0, 4, 12, 1, bricks, bricks, false);
    placer.generate_box(8, 5, 0, 10, 12, 1, bricks, bricks, false);
    placer.generate_box(5, 9, 0, 7, 12, 1, bricks, bricks, false);
    placer.generate_box(2, 11, 2, 10, 12, 10, bricks, bricks, false);

    for i in (1..=11).step_by(2) {
        placer.generate_box(i, 10, 0, i, 11, 0, we_fence, we_fence, false);
        placer.generate_box(i, 10, 12, i, 11, 12, we_fence, we_fence, false);
        placer.generate_box(0, 10, i, 0, 11, i, ns_fence, ns_fence, false);
        placer.generate_box(12, 10, i, 12, 11, i, ns_fence, ns_fence, false);
        placer.place_block(bricks, i, 13, 0);
        placer.place_block(bricks, i, 13, 12);
        placer.place_block(bricks, 0, 13, i);
        placer.place_block(bricks, 12, 13, i);
        if i != 11 {
            placer.place_block(we_fence, i + 1, 13, 0);
            placer.place_block(we_fence, i + 1, 13, 12);
            placer.place_block(ns_fence, 0, 13, i + 1);
            placer.place_block(ns_fence, 12, 13, i + 1);
        }
    }

    placer.place_block(fence(FENCE_NORTH | FENCE_EAST), 0, 13, 0);
    placer.place_block(fence(FENCE_EAST | FENCE_SOUTH), 0, 13, 12);
    placer.place_block(fence(FENCE_SOUTH | FENCE_WEST), 12, 13, 12);
    placer.place_block(fence(FENCE_NORTH | FENCE_WEST), 12, 13, 0);

    for z in (3..=9).step_by(2) {
        placer.generate_box(
            1,
            7,
            z,
            1,
            8,
            z,
            north_south_west_fence,
            north_south_west_fence,
            false,
        );
        placer.generate_box(
            11,
            7,
            z,
            11,
            8,
            z,
            north_south_east_fence,
            north_south_east_fence,
            false,
        );
    }

    let north_stairs = stairs(Direction::North);
    for ix in 0..=6 {
        let z = ix + 4;
        for x in 5..=7 {
            placer.place_block(north_stairs, x, 5 + ix, z);
        }

        if (5..=8).contains(&z) {
            placer.generate_box(5, 5, z, 7, ix + 4, z, bricks, bricks, false);
        } else if (9..=10).contains(&z) {
            placer.generate_box(5, 8, z, 7, ix + 4, z, bricks, bricks, false);
        }

        if ix >= 1 {
            placer.generate_box(5, 6 + ix, z, 7, 9 + ix, z, air, air, false);
        }
    }

    for x in 5..=7 {
        placer.place_block(north_stairs, x, 12, 11);
    }

    placer.generate_box(
        5,
        6,
        7,
        5,
        7,
        7,
        north_south_east_fence,
        north_south_east_fence,
        false,
    );
    placer.generate_box(
        7,
        6,
        7,
        7,
        7,
        7,
        north_south_west_fence,
        north_south_west_fence,
        false,
    );
    placer.generate_box(5, 13, 12, 7, 13, 12, air, air, false);
    placer.generate_box(2, 5, 2, 3, 5, 3, bricks, bricks, false);
    placer.generate_box(2, 5, 9, 3, 5, 10, bricks, bricks, false);
    placer.generate_box(2, 5, 4, 2, 5, 8, bricks, bricks, false);
    placer.generate_box(9, 5, 2, 10, 5, 3, bricks, bricks, false);
    placer.generate_box(9, 5, 9, 10, 5, 10, bricks, bricks, false);
    placer.generate_box(10, 5, 4, 10, 5, 8, bricks, bricks, false);
    let east_stairs = stairs(Direction::East);
    let west_stairs = stairs(Direction::West);
    placer.place_block(west_stairs, 4, 5, 2);
    placer.place_block(west_stairs, 4, 5, 3);
    placer.place_block(west_stairs, 4, 5, 9);
    placer.place_block(west_stairs, 4, 5, 10);
    placer.place_block(east_stairs, 8, 5, 2);
    placer.place_block(east_stairs, 8, 5, 3);
    placer.place_block(east_stairs, 8, 5, 9);
    placer.place_block(east_stairs, 8, 5, 10);
    placer.generate_box(3, 4, 4, 4, 4, 8, soul_sand(), soul_sand(), false);
    placer.generate_box(8, 4, 4, 9, 4, 8, soul_sand(), soul_sand(), false);
    placer.generate_box(3, 5, 4, 4, 5, 8, nether_wart(), nether_wart(), false);
    placer.generate_box(8, 5, 4, 9, 5, 8, nether_wart(), nether_wart(), false);
    place_castle_cross_foundation(placer);
}

fn place_monster_throne(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    has_placed_spawner: &mut bool,
) {
    let bricks = nether_bricks();
    let air = air();
    let we_fence = fence_we();
    let ns_fence = fence_ns();
    placer.generate_box(0, 2, 0, 6, 7, 7, air, air, false);
    placer.generate_box(1, 0, 0, 5, 1, 7, bricks, bricks, false);
    placer.generate_box(1, 2, 1, 5, 2, 7, bricks, bricks, false);
    placer.generate_box(1, 3, 2, 5, 3, 7, bricks, bricks, false);
    placer.generate_box(1, 4, 3, 5, 4, 7, bricks, bricks, false);
    placer.generate_box(1, 2, 0, 1, 4, 2, bricks, bricks, false);
    placer.generate_box(5, 2, 0, 5, 4, 2, bricks, bricks, false);
    placer.generate_box(1, 5, 2, 1, 5, 3, bricks, bricks, false);
    placer.generate_box(5, 5, 2, 5, 5, 3, bricks, bricks, false);
    placer.generate_box(0, 5, 3, 0, 5, 8, bricks, bricks, false);
    placer.generate_box(6, 5, 3, 6, 5, 8, bricks, bricks, false);
    placer.generate_box(1, 5, 8, 5, 5, 8, bricks, bricks, false);
    placer.place_block(fence(FENCE_WEST), 1, 6, 3);
    placer.place_block(fence(FENCE_EAST), 5, 6, 3);
    placer.place_block(fence(FENCE_NORTH | FENCE_EAST), 0, 6, 3);
    placer.place_block(fence(FENCE_NORTH | FENCE_WEST), 6, 6, 3);
    placer.generate_box(0, 6, 4, 0, 6, 7, ns_fence, ns_fence, false);
    placer.generate_box(6, 6, 4, 6, 6, 7, ns_fence, ns_fence, false);
    placer.place_block(fence(FENCE_EAST | FENCE_SOUTH), 0, 6, 8);
    placer.place_block(fence(FENCE_SOUTH | FENCE_WEST), 6, 6, 8);
    placer.generate_box(1, 6, 8, 5, 6, 8, we_fence, we_fence, false);
    placer.place_block(fence(FENCE_EAST), 1, 7, 8);
    placer.generate_box(2, 7, 8, 4, 7, 8, we_fence, we_fence, false);
    placer.place_block(fence(FENCE_WEST), 5, 7, 8);
    placer.place_block(fence(FENCE_EAST), 2, 8, 8);
    placer.place_block(we_fence, 3, 8, 8);
    placer.place_block(fence(FENCE_WEST), 4, 8, 8);
    if !*has_placed_spawner && placer.clip().is_inside(placer.world_pos(3, 5, 5)) {
        *has_placed_spawner = true;
        let _ = placer.create_spawner(3, 5, 5, BLAZE_ENTITY);
    }

    for x in 0..=6 {
        for z in 0..=6 {
            placer.fill_column_down(bricks, x, -1, z);
        }
    }
}

fn place_room_crossing(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    let we_fence = fence_we();
    let ns_fence = fence_ns();
    placer.generate_box(0, 0, 0, 6, 1, 6, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 6, 7, 6, air, air, false);
    placer.generate_box(0, 2, 0, 1, 6, 0, bricks, bricks, false);
    placer.generate_box(0, 2, 6, 1, 6, 6, bricks, bricks, false);
    placer.generate_box(5, 2, 0, 6, 6, 0, bricks, bricks, false);
    placer.generate_box(5, 2, 6, 6, 6, 6, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 0, 6, 1, bricks, bricks, false);
    placer.generate_box(0, 2, 5, 0, 6, 6, bricks, bricks, false);
    placer.generate_box(6, 2, 0, 6, 6, 1, bricks, bricks, false);
    placer.generate_box(6, 2, 5, 6, 6, 6, bricks, bricks, false);
    placer.generate_box(2, 6, 0, 4, 6, 0, bricks, bricks, false);
    placer.generate_box(2, 5, 0, 4, 5, 0, we_fence, we_fence, false);
    placer.generate_box(2, 6, 6, 4, 6, 6, bricks, bricks, false);
    placer.generate_box(2, 5, 6, 4, 5, 6, we_fence, we_fence, false);
    placer.generate_box(0, 6, 2, 0, 6, 4, bricks, bricks, false);
    placer.generate_box(0, 5, 2, 0, 5, 4, ns_fence, ns_fence, false);
    placer.generate_box(6, 6, 2, 6, 6, 4, bricks, bricks, false);
    placer.generate_box(6, 5, 2, 6, 5, 4, ns_fence, ns_fence, false);
    fill_down_square(placer, 6, 6);
}

fn place_stairs_room(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    let air = air();
    let we_fence = fence_we();
    let ns_fence = fence_ns();
    placer.generate_box(0, 0, 0, 6, 1, 6, bricks, bricks, false);
    placer.generate_box(0, 2, 0, 6, 10, 6, air, air, false);
    placer.generate_box(0, 2, 0, 1, 8, 0, bricks, bricks, false);
    placer.generate_box(5, 2, 0, 6, 8, 0, bricks, bricks, false);
    placer.generate_box(0, 2, 1, 0, 8, 6, bricks, bricks, false);
    placer.generate_box(6, 2, 1, 6, 8, 6, bricks, bricks, false);
    placer.generate_box(1, 2, 6, 5, 8, 6, bricks, bricks, false);
    placer.generate_box(0, 3, 2, 0, 5, 4, ns_fence, ns_fence, false);
    placer.generate_box(6, 3, 2, 6, 5, 2, ns_fence, ns_fence, false);
    placer.generate_box(6, 3, 4, 6, 5, 4, ns_fence, ns_fence, false);
    placer.place_block(bricks, 5, 2, 5);
    placer.generate_box(4, 2, 5, 4, 3, 5, bricks, bricks, false);
    placer.generate_box(3, 2, 5, 3, 4, 5, bricks, bricks, false);
    placer.generate_box(2, 2, 5, 2, 5, 5, bricks, bricks, false);
    placer.generate_box(1, 2, 5, 1, 6, 5, bricks, bricks, false);
    placer.generate_box(1, 7, 1, 5, 7, 4, bricks, bricks, false);
    placer.generate_box(6, 8, 2, 6, 8, 4, air, air, false);
    placer.generate_box(2, 6, 0, 4, 8, 0, bricks, bricks, false);
    placer.generate_box(2, 5, 0, 4, 5, 0, we_fence, we_fence, false);
    fill_down_square(placer, 6, 6);
}

fn place_castle_cross_foundation(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let bricks = nether_bricks();
    placer.generate_box(4, 2, 0, 8, 2, 12, bricks, bricks, false);
    placer.generate_box(0, 2, 4, 12, 2, 8, bricks, bricks, false);
    placer.generate_box(4, 0, 0, 8, 1, 3, bricks, bricks, false);
    placer.generate_box(4, 0, 9, 8, 1, 12, bricks, bricks, false);
    placer.generate_box(0, 0, 4, 3, 1, 8, bricks, bricks, false);
    placer.generate_box(9, 0, 4, 12, 1, 8, bricks, bricks, false);

    for x in 4..=8 {
        for z in 0..=2 {
            placer.fill_column_down(bricks, x, -1, z);
            placer.fill_column_down(bricks, x, -1, 12 - z);
        }
    }

    for x in 0..=2 {
        for z in 4..=8 {
            placer.fill_column_down(bricks, x, -1, z);
            placer.fill_column_down(bricks, 12 - x, -1, z);
        }
    }
}

fn fill_down_square(placer: &mut ScatteredFeaturePlacer<'_, '_>, max_x: i32, max_z: i32) {
    let bricks = nether_bricks();
    for x in 0..=max_x {
        for z in 0..=max_z {
            placer.fill_column_down(bricks, x, -1, z);
        }
    }
}
