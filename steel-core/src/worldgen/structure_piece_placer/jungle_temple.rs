use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{AttachFace, BlockStateProperties, RedstoneSide};
use steel_registry::{Registry, vanilla_blocks};
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockStateId, BoundingBox, Direction};

use crate::world::structure::jungle_temple::JungleTemplePieceData;
use crate::worldgen::region::WorldGenRegion;

use super::StructurePiecePlacer;
use super::scattered_feature::ScatteredFeaturePlacer;

const JUNGLE_TEMPLE_LOOT: &str = "minecraft:chests/jungle_temple";
const JUNGLE_TEMPLE_DISPENSER_LOOT: &str = "minecraft:chests/jungle_temple_dispenser";

impl StructurePiecePlacer {
    pub(super) fn place_jungle_temple_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounding_box: &mut BoundingBox,
        orientation: Option<Direction>,
        data: &mut JungleTemplePieceData,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
    ) -> bool {
        let mut placer =
            ScatteredFeaturePlacer::new(region, registry, bounding_box, orientation, clip);
        if !placer.update_average_ground_height(&mut data.height_position, 0) {
            return false;
        }

        place_jungle_temple(&mut placer, data, random);
        true
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "jungle temple placement is a direct port of vanilla's linear postProcess"
)]
fn place_jungle_temple(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    data: &mut JungleTemplePieceData,
    random: &mut WorldgenRandom,
) {
    let air = vanilla_blocks::AIR.default_state();
    let mossy_cobblestone = vanilla_blocks::MOSSY_COBBLESTONE.default_state();
    let chiseled_stone_bricks = vanilla_blocks::CHISELED_STONE_BRICKS.default_state();

    moss_box(placer, 0, -4, 0, 11, 0, 14, false, random);
    moss_box(placer, 2, 1, 2, 9, 2, 2, false, random);
    moss_box(placer, 2, 1, 12, 9, 2, 12, false, random);
    moss_box(placer, 2, 1, 3, 2, 2, 11, false, random);
    moss_box(placer, 9, 1, 3, 9, 2, 11, false, random);
    moss_box(placer, 1, 3, 1, 10, 6, 1, false, random);
    moss_box(placer, 1, 3, 13, 10, 6, 13, false, random);
    moss_box(placer, 1, 3, 2, 1, 6, 12, false, random);
    moss_box(placer, 10, 3, 2, 10, 6, 12, false, random);
    moss_box(placer, 2, 3, 2, 9, 3, 12, false, random);
    moss_box(placer, 2, 6, 2, 9, 6, 12, false, random);
    moss_box(placer, 3, 7, 3, 8, 7, 11, false, random);
    moss_box(placer, 4, 8, 4, 7, 8, 10, false, random);
    placer.generate_air_box(3, 1, 3, 8, 2, 11);
    placer.generate_air_box(4, 3, 6, 7, 3, 9);
    placer.generate_air_box(2, 4, 2, 9, 5, 12);
    placer.generate_air_box(4, 6, 5, 7, 6, 9);
    placer.generate_air_box(5, 7, 6, 6, 7, 8);
    placer.generate_air_box(5, 1, 2, 6, 2, 2);
    placer.generate_air_box(5, 2, 12, 6, 2, 12);
    placer.generate_air_box(5, 5, 1, 6, 5, 1);
    placer.generate_air_box(5, 5, 13, 6, 5, 13);
    placer.place_block(air, 1, 5, 5);
    placer.place_block(air, 10, 5, 5);
    placer.place_block(air, 1, 5, 9);
    placer.place_block(air, 10, 5, 9);

    for z in [0, 14] {
        moss_box(placer, 2, 4, z, 2, 5, z, false, random);
        moss_box(placer, 4, 4, z, 4, 5, z, false, random);
        moss_box(placer, 7, 4, z, 7, 5, z, false, random);
        moss_box(placer, 9, 4, z, 9, 5, z, false, random);
    }

    moss_box(placer, 5, 6, 0, 6, 6, 0, false, random);

    for x in [0, 11] {
        for z in (2..=12).step_by(2) {
            moss_box(placer, x, 4, z, x, 5, z, false, random);
        }
        moss_box(placer, x, 6, 5, x, 6, 5, false, random);
        moss_box(placer, x, 6, 9, x, 6, 9, false, random);
    }

    moss_box(placer, 2, 7, 2, 2, 9, 2, false, random);
    moss_box(placer, 9, 7, 2, 9, 9, 2, false, random);
    moss_box(placer, 2, 7, 12, 2, 9, 12, false, random);
    moss_box(placer, 9, 7, 12, 9, 9, 12, false, random);
    moss_box(placer, 4, 9, 4, 4, 9, 4, false, random);
    moss_box(placer, 7, 9, 4, 7, 9, 4, false, random);
    moss_box(placer, 4, 9, 10, 4, 9, 10, false, random);
    moss_box(placer, 7, 9, 10, 7, 9, 10, false, random);
    moss_box(placer, 5, 9, 7, 6, 9, 7, false, random);

    let east_stairs = stairs(Direction::East);
    let west_stairs = stairs(Direction::West);
    let south_stairs = stairs(Direction::South);
    let north_stairs = stairs(Direction::North);
    placer.place_block(north_stairs, 5, 9, 6);
    placer.place_block(north_stairs, 6, 9, 6);
    placer.place_block(south_stairs, 5, 9, 8);
    placer.place_block(south_stairs, 6, 9, 8);
    placer.place_block(north_stairs, 4, 0, 0);
    placer.place_block(north_stairs, 5, 0, 0);
    placer.place_block(north_stairs, 6, 0, 0);
    placer.place_block(north_stairs, 7, 0, 0);
    placer.place_block(north_stairs, 4, 1, 8);
    placer.place_block(north_stairs, 4, 2, 9);
    placer.place_block(north_stairs, 4, 3, 10);
    placer.place_block(north_stairs, 7, 1, 8);
    placer.place_block(north_stairs, 7, 2, 9);
    placer.place_block(north_stairs, 7, 3, 10);
    moss_box(placer, 4, 1, 9, 4, 1, 9, false, random);
    moss_box(placer, 7, 1, 9, 7, 1, 9, false, random);
    moss_box(placer, 4, 1, 10, 7, 2, 10, false, random);
    moss_box(placer, 5, 4, 5, 6, 4, 5, false, random);
    placer.place_block(east_stairs, 4, 4, 5);
    placer.place_block(west_stairs, 7, 4, 5);

    for i in 0..4 {
        placer.place_block(south_stairs, 5, -i, 6 + i);
        placer.place_block(south_stairs, 6, -i, 6 + i);
        placer.generate_air_box(5, -i, 7 + i, 6, -i, 9 + i);
    }

    placer.generate_air_box(1, -3, 12, 10, -1, 13);
    placer.generate_air_box(1, -3, 1, 3, -1, 13);
    placer.generate_air_box(1, -3, 1, 9, -1, 5);

    for z in (1..=13).step_by(2) {
        moss_box(placer, 1, -3, z, 1, -2, z, false, random);
    }
    for z in (2..=12).step_by(2) {
        moss_box(placer, 1, -1, z, 3, -1, z, false, random);
    }

    moss_box(placer, 2, -2, 1, 5, -2, 1, false, random);
    moss_box(placer, 7, -2, 1, 9, -2, 1, false, random);
    moss_box(placer, 6, -3, 1, 6, -3, 1, false, random);
    moss_box(placer, 6, -1, 1, 6, -1, 1, false, random);
    placer.place_block(tripwire_hook(Direction::East), 1, -3, 8);
    placer.place_block(tripwire_hook(Direction::West), 4, -3, 8);
    placer.place_block(tripwire_east_west(), 2, -3, 8);
    placer.place_block(tripwire_east_west(), 3, -3, 8);

    let redstone_wire_ns = redstone_ns();
    placer.place_block(redstone_wire_ns, 5, -3, 7);
    placer.place_block(redstone_wire_ns, 5, -3, 6);
    placer.place_block(redstone_wire_ns, 5, -3, 5);
    placer.place_block(redstone_wire_ns, 5, -3, 4);
    placer.place_block(redstone_wire_ns, 5, -3, 3);
    placer.place_block(redstone_wire_ns, 5, -3, 2);
    placer.place_block(redstone_nw(), 5, -3, 1);
    placer.place_block(redstone_ew(), 4, -3, 1);
    placer.place_block(mossy_cobblestone, 3, -3, 1);
    if !data.placed_trap1 {
        data.placed_trap1 = placer.create_dispenser(
            random,
            3,
            -2,
            1,
            Direction::North,
            JUNGLE_TEMPLE_DISPENSER_LOOT,
        );
    }

    placer.place_block(vine(Direction::South), 3, -2, 2);
    placer.place_block(tripwire_hook(Direction::North), 7, -3, 1);
    placer.place_block(tripwire_hook(Direction::South), 7, -3, 5);
    placer.place_block(tripwire_north_south(), 7, -3, 2);
    placer.place_block(tripwire_north_south(), 7, -3, 3);
    placer.place_block(tripwire_north_south(), 7, -3, 4);
    placer.place_block(redstone_ew(), 8, -3, 6);
    placer.place_block(redstone_ws(), 9, -3, 6);
    placer.place_block(redstone_n_side_s_up(), 9, -3, 5);
    placer.place_block(mossy_cobblestone, 9, -3, 4);
    placer.place_block(redstone_wire_ns, 9, -2, 4);
    if !data.placed_trap2 {
        data.placed_trap2 = placer.create_dispenser(
            random,
            9,
            -2,
            3,
            Direction::West,
            JUNGLE_TEMPLE_DISPENSER_LOOT,
        );
    }

    placer.place_block(vine(Direction::East), 8, -1, 3);
    placer.place_block(vine(Direction::East), 8, -2, 3);
    if !data.placed_main_chest {
        data.placed_main_chest = placer.create_chest(random, 8, -3, 3, JUNGLE_TEMPLE_LOOT);
    }

    placer.place_block(mossy_cobblestone, 9, -3, 2);
    placer.place_block(mossy_cobblestone, 8, -3, 1);
    placer.place_block(mossy_cobblestone, 4, -3, 5);
    placer.place_block(mossy_cobblestone, 5, -2, 5);
    placer.place_block(mossy_cobblestone, 5, -1, 5);
    placer.place_block(mossy_cobblestone, 6, -3, 5);
    placer.place_block(mossy_cobblestone, 7, -2, 5);
    placer.place_block(mossy_cobblestone, 7, -1, 5);
    placer.place_block(mossy_cobblestone, 8, -3, 5);
    moss_box(placer, 9, -1, 1, 9, -1, 5, false, random);
    placer.generate_air_box(8, -3, 8, 10, -1, 10);
    placer.place_block(chiseled_stone_bricks, 8, -2, 11);
    placer.place_block(chiseled_stone_bricks, 9, -2, 11);
    placer.place_block(chiseled_stone_bricks, 10, -2, 11);
    let lever = vanilla_blocks::LEVER
        .default_state()
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::North)
        .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Wall);
    placer.place_block(lever, 8, -2, 12);
    placer.place_block(lever, 9, -2, 12);
    placer.place_block(lever, 10, -2, 12);
    moss_box(placer, 8, -3, 8, 8, -3, 10, false, random);
    moss_box(placer, 10, -3, 8, 10, -3, 10, false, random);
    placer.place_block(mossy_cobblestone, 10, -2, 9);
    placer.place_block(redstone_wire_ns, 8, -2, 9);
    placer.place_block(redstone_wire_ns, 8, -2, 10);
    placer.place_block(redstone_all_sides(), 10, -1, 9);
    placer.place_block(sticky_piston(Direction::Up), 9, -2, 8);
    placer.place_block(sticky_piston(Direction::West), 10, -2, 8);
    placer.place_block(sticky_piston(Direction::West), 10, -1, 8);
    placer.place_block(repeater(Direction::North), 10, -2, 10);
    if !data.placed_hidden_chest {
        data.placed_hidden_chest = placer.create_chest(random, 9, -3, 10, JUNGLE_TEMPLE_LOOT);
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla StructurePiece.generateBox selector overload"
)]
fn moss_box(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
    skip_air: bool,
    random: &mut WorldgenRandom,
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
        |rng, _, _, _, _| {
            if rng.next_f32() < 0.4 {
                vanilla_blocks::COBBLESTONE.default_state()
            } else {
                vanilla_blocks::MOSSY_COBBLESTONE.default_state()
            }
        },
    );
}

fn stairs(facing: Direction) -> BlockStateId {
    vanilla_blocks::COBBLESTONE_STAIRS
        .default_state()
        .set_value(&BlockStateProperties::FACING, facing)
}

fn tripwire_hook(facing: Direction) -> BlockStateId {
    vanilla_blocks::TRIPWIRE_HOOK
        .default_state()
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
        .set_value(&BlockStateProperties::ATTACHED, true)
}

fn tripwire_east_west() -> BlockStateId {
    vanilla_blocks::TRIPWIRE
        .default_state()
        .set_value(&BlockStateProperties::EAST, true)
        .set_value(&BlockStateProperties::WEST, true)
        .set_value(&BlockStateProperties::ATTACHED, true)
}

fn tripwire_north_south() -> BlockStateId {
    vanilla_blocks::TRIPWIRE
        .default_state()
        .set_value(&BlockStateProperties::NORTH, true)
        .set_value(&BlockStateProperties::SOUTH, true)
        .set_value(&BlockStateProperties::ATTACHED, true)
}

fn redstone_ns() -> BlockStateId {
    vanilla_blocks::REDSTONE_WIRE
        .default_state()
        .set_value(&BlockStateProperties::NORTH_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::SOUTH_REDSTONE, RedstoneSide::Side)
}

fn redstone_ew() -> BlockStateId {
    vanilla_blocks::REDSTONE_WIRE
        .default_state()
        .set_value(&BlockStateProperties::EAST_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::WEST_REDSTONE, RedstoneSide::Side)
}

fn redstone_nw() -> BlockStateId {
    vanilla_blocks::REDSTONE_WIRE
        .default_state()
        .set_value(&BlockStateProperties::NORTH_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::WEST_REDSTONE, RedstoneSide::Side)
}

fn redstone_ws() -> BlockStateId {
    vanilla_blocks::REDSTONE_WIRE
        .default_state()
        .set_value(&BlockStateProperties::WEST_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::SOUTH_REDSTONE, RedstoneSide::Side)
}

fn redstone_n_side_s_up() -> BlockStateId {
    vanilla_blocks::REDSTONE_WIRE
        .default_state()
        .set_value(&BlockStateProperties::NORTH_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::SOUTH_REDSTONE, RedstoneSide::Up)
}

fn redstone_all_sides() -> BlockStateId {
    vanilla_blocks::REDSTONE_WIRE
        .default_state()
        .set_value(&BlockStateProperties::NORTH_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::SOUTH_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::EAST_REDSTONE, RedstoneSide::Side)
        .set_value(&BlockStateProperties::WEST_REDSTONE, RedstoneSide::Side)
}

fn vine(side: Direction) -> BlockStateId {
    let vine = vanilla_blocks::VINE.default_state();
    match side {
        Direction::North => vine.set_value(&BlockStateProperties::NORTH, true),
        Direction::East => vine.set_value(&BlockStateProperties::EAST, true),
        Direction::South => vine.set_value(&BlockStateProperties::SOUTH, true),
        Direction::West => vine.set_value(&BlockStateProperties::WEST, true),
        Direction::Up => vine.set_value(&BlockStateProperties::UP, true),
        Direction::Down => vine,
    }
}

fn sticky_piston(facing: Direction) -> BlockStateId {
    vanilla_blocks::STICKY_PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, facing)
}

fn repeater(facing: Direction) -> BlockStateId {
    vanilla_blocks::REPEATER
        .default_state()
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
}
