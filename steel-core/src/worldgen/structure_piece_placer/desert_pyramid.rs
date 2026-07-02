use std::cmp::Ordering;

use glam::IVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{Registry, vanilla_blocks};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::random::{PositionalRandom, Random};
use steel_utils::{BlockPos, BlockStateId, BoundingBox, Direction, types::UpdateFlags};

use super::StructurePiecePlacer;
use crate::behavior::BlockStateBehaviorExt as _;
use crate::chunk::heightmap::HeightmapType;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::template::StructureTemplate;
use steel_worldgen::structure::desert_pyramid::DesertPyramidPieceData;
use steel_worldgen::structure::{
    ProceduralPieceData, StructurePiece, StructurePiecePayload, StructureStart,
};

const DESERT_PYRAMID_LOOT: &str = "minecraft:chests/desert_pyramid";
const DESERT_PYRAMID_ARCHAEOLOGY: &str = "minecraft:archaeology/desert_pyramid";
const HORIZONTAL_PLANE: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

impl StructurePiecePlacer {
    pub(super) fn place_desert_pyramid_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounding_box: &mut BoundingBox,
        orientation: Option<Direction>,
        data: &mut DesertPyramidPieceData,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
    ) -> bool {
        let offset = -random.next_i32_bounded(3);
        let mut placer = DesertPyramidPlacer {
            region,
            registry,
            bounding_box,
            orientation,
            clip,
            data,
        };
        if !placer.update_height_position_to_lowest_ground_height(offset) {
            return false;
        }

        placer.place(random);
        true
    }

    pub(super) fn after_place_desert_pyramid(
        region: &mut WorldGenRegion<'_>,
        pieces: &mut [StructurePiece],
        clip: BoundingBox,
    ) {
        let mut unique_sand_placements = Vec::new();
        for piece in pieces.iter_mut() {
            let StructurePiecePayload::Procedural(ProceduralPieceData::DesertPyramid(data)) =
                &mut piece.payload
            else {
                continue;
            };
            unique_sand_placements.extend(
                data.potential_suspicious_sand_world_positions
                    .iter()
                    .copied(),
            );
            Self::place_desert_pyramid_suspicious_sand(
                region,
                clip,
                data.random_collapsed_roof_pos,
            );
        }

        unique_sand_placements.sort_by(Self::compare_vec3i);
        unique_sand_placements.dedup();

        let Some(structure_box) = StructureStart::compute_bounding_box(pieces, 0) else {
            return;
        };
        let center = structure_box.center();
        let mut seed_random = LegacyRandom::from_seed(region.seed() as u64);
        let splitter = seed_random.next_positional();
        let mut positional_random = splitter.at(center.x, center.y, center.z);
        Self::shuffle_block_positions(&mut unique_sand_placements, &mut positional_random);
        let mut suspicious_sand_to_place = unique_sand_placements
            .len()
            .min(positional_random.next_i32_between_exclusive(5, 8) as usize);

        for pos in unique_sand_placements {
            if suspicious_sand_to_place > 0 {
                suspicious_sand_to_place -= 1;
                Self::place_desert_pyramid_suspicious_sand(region, clip, pos);
            } else if clip.contains_blockpos(pos) {
                let _ = region.set_block_state(
                    pos,
                    vanilla_blocks::SAND.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                );
            }
        }
    }

    fn place_desert_pyramid_suspicious_sand(
        region: &mut WorldGenRegion<'_>,
        clip: BoundingBox,
        pos: BlockPos,
    ) {
        if !clip.contains_blockpos(pos) {
            return;
        }
        let state = vanilla_blocks::SUSPICIOUS_SAND.default_state();
        if region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS) {
            let _ = Self::set_brushable_loot_table(region, pos, state, DESERT_PYRAMID_ARCHAEOLOGY);
        }
    }

    fn compare_vec3i(left: &BlockPos, right: &BlockPos) -> Ordering {
        left.y()
            .cmp(&right.y())
            .then_with(|| left.z().cmp(&right.z()))
            .then_with(|| left.x().cmp(&right.x()))
    }

    fn shuffle_block_positions(positions: &mut [BlockPos], random: &mut impl Random) {
        for i in (1..positions.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!("block position shuffle length exceeds i32 range");
            };
            let j = random.next_i32_bounded(bound) as usize;
            positions.swap(i, j);
        }
    }
}

struct DesertPyramidPlacer<'a, 'world> {
    region: &'a mut WorldGenRegion<'world>,
    registry: &'a Registry,
    bounding_box: &'a mut BoundingBox,
    orientation: Option<Direction>,
    clip: BoundingBox,
    data: &'a mut DesertPyramidPieceData,
}

impl DesertPyramidPlacer<'_, '_> {
    fn update_height_position_to_lowest_ground_height(&mut self, offset: i32) -> bool {
        if self.data.height_position.is_some() {
            return true;
        }

        let mut lowest_ground_height = self.region.max_y_exclusive() + 1;
        let mut found_position_within_bounding_box = false;
        for z in self.bounding_box.min_z()..=self.bounding_box.max_z() {
            for x in self.bounding_box.min_x()..=self.bounding_box.max_x() {
                lowest_ground_height = lowest_ground_height.min(self.region.height_at(
                    HeightmapType::MotionBlockingNoLeaves,
                    x,
                    z,
                ));
                found_position_within_bounding_box = true;
            }
        }

        if !found_position_within_bounding_box {
            return false;
        }

        self.data.height_position = Some(lowest_ground_height);
        let dy = lowest_ground_height - self.bounding_box.min_y() + offset;
        *self.bounding_box = self.bounding_box.translate(IVec3::new(0, dy, 0));
        true
    }

    #[expect(
        clippy::too_many_lines,
        reason = "desert pyramid placement is a direct port of vanilla's linear postProcess"
    )]
    fn place(&mut self, random: &mut WorldgenRandom) {
        let sandstone = vanilla_blocks::SANDSTONE.default_state();
        let air = vanilla_blocks::AIR.default_state();
        let cut_sandstone = vanilla_blocks::CUT_SANDSTONE.default_state();
        let chiseled_sandstone = vanilla_blocks::CHISELED_SANDSTONE.default_state();
        let orange_terracotta = vanilla_blocks::ORANGE_TERRACOTTA.default_state();
        let blue_terracotta = vanilla_blocks::BLUE_TERRACOTTA.default_state();
        let sand = vanilla_blocks::SAND.default_state();

        self.generate_box(0, -4, 0, 20, 0, 20, sandstone, sandstone, false);

        for pos in 1..=9 {
            self.generate_box(
                pos,
                pos,
                pos,
                20 - pos,
                pos,
                20 - pos,
                sandstone,
                sandstone,
                false,
            );
            self.generate_box(
                pos + 1,
                pos,
                pos + 1,
                19 - pos,
                pos,
                19 - pos,
                air,
                air,
                false,
            );
        }

        for x in 0..21 {
            for z in 0..21 {
                self.fill_column_down(sandstone, x, -5, z);
            }
        }

        let north_stairs = Self::stairs(Direction::North);
        let south_stairs = Self::stairs(Direction::South);
        let east_stairs = Self::stairs(Direction::East);
        let west_stairs = Self::stairs(Direction::West);

        self.generate_box(0, 0, 0, 4, 9, 4, sandstone, air, false);
        self.generate_box(1, 10, 1, 3, 10, 3, sandstone, sandstone, false);
        self.place_block(north_stairs, 2, 10, 0);
        self.place_block(south_stairs, 2, 10, 4);
        self.place_block(east_stairs, 0, 10, 2);
        self.place_block(west_stairs, 4, 10, 2);
        self.generate_box(16, 0, 0, 20, 9, 4, sandstone, air, false);
        self.generate_box(17, 10, 1, 19, 10, 3, sandstone, sandstone, false);
        self.place_block(north_stairs, 18, 10, 0);
        self.place_block(south_stairs, 18, 10, 4);
        self.place_block(east_stairs, 16, 10, 2);
        self.place_block(west_stairs, 20, 10, 2);
        self.generate_box(8, 0, 0, 12, 4, 4, sandstone, air, false);
        self.generate_box(9, 1, 0, 11, 3, 4, air, air, false);
        self.place_block(cut_sandstone, 9, 1, 1);
        self.place_block(cut_sandstone, 9, 2, 1);
        self.place_block(cut_sandstone, 9, 3, 1);
        self.place_block(cut_sandstone, 10, 3, 1);
        self.place_block(cut_sandstone, 11, 3, 1);
        self.place_block(cut_sandstone, 11, 2, 1);
        self.place_block(cut_sandstone, 11, 1, 1);
        self.generate_box(4, 1, 1, 8, 3, 3, sandstone, air, false);
        self.generate_box(4, 1, 2, 8, 2, 2, air, air, false);
        self.generate_box(12, 1, 1, 16, 3, 3, sandstone, air, false);
        self.generate_box(12, 1, 2, 16, 2, 2, air, air, false);
        self.generate_box(5, 4, 5, 15, 4, 15, sandstone, sandstone, false);
        self.generate_box(9, 4, 9, 11, 4, 11, air, air, false);
        self.generate_box(8, 1, 8, 8, 3, 8, cut_sandstone, cut_sandstone, false);
        self.generate_box(12, 1, 8, 12, 3, 8, cut_sandstone, cut_sandstone, false);
        self.generate_box(8, 1, 12, 8, 3, 12, cut_sandstone, cut_sandstone, false);
        self.generate_box(12, 1, 12, 12, 3, 12, cut_sandstone, cut_sandstone, false);
        self.generate_box(1, 1, 5, 4, 4, 11, sandstone, sandstone, false);
        self.generate_box(16, 1, 5, 19, 4, 11, sandstone, sandstone, false);
        self.generate_box(6, 7, 9, 6, 7, 11, sandstone, sandstone, false);
        self.generate_box(14, 7, 9, 14, 7, 11, sandstone, sandstone, false);
        self.generate_box(5, 5, 9, 5, 7, 11, cut_sandstone, cut_sandstone, false);
        self.generate_box(15, 5, 9, 15, 7, 11, cut_sandstone, cut_sandstone, false);
        self.place_block(air, 5, 5, 10);
        self.place_block(air, 5, 6, 10);
        self.place_block(air, 6, 6, 10);
        self.place_block(air, 15, 5, 10);
        self.place_block(air, 15, 6, 10);
        self.place_block(air, 14, 6, 10);
        self.generate_box(2, 4, 4, 2, 6, 4, air, air, false);
        self.generate_box(18, 4, 4, 18, 6, 4, air, air, false);
        self.place_block(north_stairs, 2, 4, 5);
        self.place_block(north_stairs, 2, 3, 4);
        self.place_block(north_stairs, 18, 4, 5);
        self.place_block(north_stairs, 18, 3, 4);
        self.generate_box(1, 1, 3, 2, 2, 3, sandstone, sandstone, false);
        self.generate_box(18, 1, 3, 19, 2, 3, sandstone, sandstone, false);
        self.place_block(sandstone, 1, 1, 2);
        self.place_block(sandstone, 19, 1, 2);
        self.place_block(vanilla_blocks::SANDSTONE_SLAB.default_state(), 1, 2, 2);
        self.place_block(vanilla_blocks::SANDSTONE_SLAB.default_state(), 19, 2, 2);
        self.place_block(west_stairs, 2, 1, 2);
        self.place_block(east_stairs, 18, 1, 2);
        self.generate_box(4, 3, 5, 4, 3, 17, sandstone, sandstone, false);
        self.generate_box(16, 3, 5, 16, 3, 17, sandstone, sandstone, false);
        self.generate_box(3, 1, 5, 4, 2, 16, air, air, false);
        self.generate_box(15, 1, 5, 16, 2, 16, air, air, false);

        for z in (5..=17).step_by(2) {
            self.place_block(cut_sandstone, 4, 1, z);
            self.place_block(chiseled_sandstone, 4, 2, z);
            self.place_block(cut_sandstone, 16, 1, z);
            self.place_block(chiseled_sandstone, 16, 2, z);
        }

        self.place_block(orange_terracotta, 10, 0, 7);
        self.place_block(orange_terracotta, 10, 0, 8);
        self.place_block(orange_terracotta, 9, 0, 9);
        self.place_block(orange_terracotta, 11, 0, 9);
        self.place_block(orange_terracotta, 8, 0, 10);
        self.place_block(orange_terracotta, 12, 0, 10);
        self.place_block(orange_terracotta, 7, 0, 10);
        self.place_block(orange_terracotta, 13, 0, 10);
        self.place_block(orange_terracotta, 9, 0, 11);
        self.place_block(orange_terracotta, 11, 0, 11);
        self.place_block(orange_terracotta, 10, 0, 12);
        self.place_block(orange_terracotta, 10, 0, 13);
        self.place_block(blue_terracotta, 10, 0, 10);

        for x in [0, 20] {
            self.place_block(cut_sandstone, x, 2, 1);
            self.place_block(orange_terracotta, x, 2, 2);
            self.place_block(cut_sandstone, x, 2, 3);
            self.place_block(cut_sandstone, x, 3, 1);
            self.place_block(orange_terracotta, x, 3, 2);
            self.place_block(cut_sandstone, x, 3, 3);
            self.place_block(orange_terracotta, x, 4, 1);
            self.place_block(chiseled_sandstone, x, 4, 2);
            self.place_block(orange_terracotta, x, 4, 3);
            self.place_block(cut_sandstone, x, 5, 1);
            self.place_block(orange_terracotta, x, 5, 2);
            self.place_block(cut_sandstone, x, 5, 3);
            self.place_block(orange_terracotta, x, 6, 1);
            self.place_block(chiseled_sandstone, x, 6, 2);
            self.place_block(orange_terracotta, x, 6, 3);
            self.place_block(orange_terracotta, x, 7, 1);
            self.place_block(orange_terracotta, x, 7, 2);
            self.place_block(orange_terracotta, x, 7, 3);
            self.place_block(cut_sandstone, x, 8, 1);
            self.place_block(cut_sandstone, x, 8, 2);
            self.place_block(cut_sandstone, x, 8, 3);
        }

        for x in [2, 18] {
            self.place_block(cut_sandstone, x - 1, 2, 0);
            self.place_block(orange_terracotta, x, 2, 0);
            self.place_block(cut_sandstone, x + 1, 2, 0);
            self.place_block(cut_sandstone, x - 1, 3, 0);
            self.place_block(orange_terracotta, x, 3, 0);
            self.place_block(cut_sandstone, x + 1, 3, 0);
            self.place_block(orange_terracotta, x - 1, 4, 0);
            self.place_block(chiseled_sandstone, x, 4, 0);
            self.place_block(orange_terracotta, x + 1, 4, 0);
            self.place_block(cut_sandstone, x - 1, 5, 0);
            self.place_block(orange_terracotta, x, 5, 0);
            self.place_block(cut_sandstone, x + 1, 5, 0);
            self.place_block(orange_terracotta, x - 1, 6, 0);
            self.place_block(chiseled_sandstone, x, 6, 0);
            self.place_block(orange_terracotta, x + 1, 6, 0);
            self.place_block(orange_terracotta, x - 1, 7, 0);
            self.place_block(orange_terracotta, x, 7, 0);
            self.place_block(orange_terracotta, x + 1, 7, 0);
            self.place_block(cut_sandstone, x - 1, 8, 0);
            self.place_block(cut_sandstone, x, 8, 0);
            self.place_block(cut_sandstone, x + 1, 8, 0);
        }

        self.generate_box(8, 4, 0, 12, 6, 0, cut_sandstone, cut_sandstone, false);
        self.place_block(air, 8, 6, 0);
        self.place_block(air, 12, 6, 0);
        self.place_block(orange_terracotta, 9, 5, 0);
        self.place_block(chiseled_sandstone, 10, 5, 0);
        self.place_block(orange_terracotta, 11, 5, 0);
        self.generate_box(8, -14, 8, 12, -11, 12, cut_sandstone, cut_sandstone, false);
        self.generate_box(
            8,
            -10,
            8,
            12,
            -10,
            12,
            chiseled_sandstone,
            chiseled_sandstone,
            false,
        );
        self.generate_box(8, -9, 8, 12, -9, 12, cut_sandstone, cut_sandstone, false);
        self.generate_box(8, -8, 8, 12, -1, 12, sandstone, sandstone, false);
        self.generate_box(9, -11, 9, 11, -1, 11, air, air, false);
        self.place_block(
            vanilla_blocks::STONE_PRESSURE_PLATE.default_state(),
            10,
            -11,
            10,
        );
        self.generate_box(
            9,
            -13,
            9,
            11,
            -13,
            11,
            vanilla_blocks::TNT.default_state(),
            air,
            false,
        );
        self.place_block(air, 8, -11, 10);
        self.place_block(air, 8, -10, 10);
        self.place_block(chiseled_sandstone, 7, -10, 10);
        self.place_block(cut_sandstone, 7, -11, 10);
        self.place_block(air, 12, -11, 10);
        self.place_block(air, 12, -10, 10);
        self.place_block(chiseled_sandstone, 13, -10, 10);
        self.place_block(cut_sandstone, 13, -11, 10);
        self.place_block(air, 10, -11, 8);
        self.place_block(air, 10, -10, 8);
        self.place_block(chiseled_sandstone, 10, -10, 7);
        self.place_block(cut_sandstone, 10, -11, 7);
        self.place_block(air, 10, -11, 12);
        self.place_block(air, 10, -10, 12);
        self.place_block(chiseled_sandstone, 10, -10, 13);
        self.place_block(cut_sandstone, 10, -11, 13);

        for direction in HORIZONTAL_PLANE {
            let chest_index = direction_2d_data_value(direction);
            if !self.data.has_placed_chest[chest_index] {
                let (dx, dz) = direction.offset_xz();
                let chest_pos = self.world_pos(10 + dx * 2, -11, 10 + dz * 2);
                self.data.has_placed_chest[chest_index] = StructurePiecePlacer::create_loot_chest(
                    self.region,
                    self.clip,
                    random,
                    chest_pos,
                    DESERT_PYRAMID_LOOT,
                );
            }
        }

        self.add_cellar(
            sand,
            sandstone,
            cut_sandstone,
            chiseled_sandstone,
            orange_terracotta,
            blue_terracotta,
        );
    }

    fn add_cellar(
        &mut self,
        sand: BlockStateId,
        sandstone: BlockStateId,
        cut_sandstone: BlockStateId,
        chiseled_sandstone: BlockStateId,
        orange_terracotta: BlockStateId,
        blue_terracotta: BlockStateId,
    ) {
        self.add_cellar_stairs(sand, sandstone);
        self.add_cellar_room(
            cut_sandstone,
            chiseled_sandstone,
            orange_terracotta,
            blue_terracotta,
        );
    }

    fn add_cellar_stairs(&mut self, sand: BlockStateId, sandstone: BlockStateId) {
        let stairs = Self::stairs(Direction::West);
        self.place_block(stairs, 13, -1, 17);
        self.place_block(stairs, 14, -2, 17);
        self.place_block(stairs, 15, -3, 17);
        let variant = self.region.random_mut().next_bool();
        self.place_block(sand, 12, 0, 17);
        self.place_block(sand, 13, 0, 17);
        self.place_block(sand, 14, 0, 17);
        self.place_block(sand, 15, 0, 17);
        self.place_block(sand, 16, 0, 17);
        self.place_block(sand, 14, -1, 17);
        self.place_block(if variant { sand } else { sandstone }, 15, -1, 17);
        self.place_block(if variant { sandstone } else { sand }, 16, -1, 17);
        self.place_block(sand, 15, -2, 17);
        self.place_block(sandstone, 16, -2, 17);
        self.place_block(sand, 16, -3, 17);
    }

    fn add_cellar_room(
        &mut self,
        cut_sandstone: BlockStateId,
        chiseled_sandstone: BlockStateId,
        orange_terracotta: BlockStateId,
        blue_terracotta: BlockStateId,
    ) {
        self.generate_box(13, -3, 10, 13, -3, 15, cut_sandstone, cut_sandstone, true);
        self.generate_box(19, -3, 10, 19, -3, 15, cut_sandstone, cut_sandstone, true);
        self.generate_box(13, -3, 10, 19, -3, 11, cut_sandstone, cut_sandstone, true);
        self.generate_box(13, -3, 16, 19, -3, 16, cut_sandstone, cut_sandstone, true);
        self.generate_box(
            13,
            -2,
            10,
            13,
            -2,
            15,
            chiseled_sandstone,
            chiseled_sandstone,
            true,
        );
        self.generate_box(
            19,
            -2,
            10,
            19,
            -2,
            15,
            chiseled_sandstone,
            chiseled_sandstone,
            true,
        );
        self.generate_box(
            13,
            -2,
            10,
            19,
            -2,
            11,
            chiseled_sandstone,
            chiseled_sandstone,
            true,
        );
        self.generate_box(
            13,
            -2,
            16,
            19,
            -2,
            16,
            chiseled_sandstone,
            chiseled_sandstone,
            true,
        );
        self.generate_box(13, -1, 10, 13, -1, 15, cut_sandstone, cut_sandstone, true);
        self.generate_box(19, -1, 10, 19, -1, 15, cut_sandstone, cut_sandstone, true);
        self.generate_box(13, -1, 10, 19, -1, 11, cut_sandstone, cut_sandstone, true);
        self.generate_box(13, -1, 16, 19, -1, 16, cut_sandstone, cut_sandstone, true);
        self.place_sand_box(14, -3, 11, 18, -1, 15);
        self.place_collapsed_roof(14, 0, 11, 18, 15);
        self.place_block(blue_terracotta, 16, -4, 13);
        self.place_block(orange_terracotta, 17, -4, 12);
        self.place_block(orange_terracotta, 17, -4, 14);
        self.place_block(orange_terracotta, 15, -4, 12);
        self.place_block(orange_terracotta, 15, -4, 14);
        self.place_block(orange_terracotta, 18, -4, 13);
        self.place_block(orange_terracotta, 14, -4, 13);
        self.place_block(orange_terracotta, 16, -4, 15);
        self.place_block(orange_terracotta, 16, -4, 11);
        self.place_block(orange_terracotta, 19, -4, 13);
        self.place_sand(19, -3, 13);
        self.place_sand(19, -2, 13);
        self.place_block(cut_sandstone, 20, -3, 13);
        self.place_block(chiseled_sandstone, 20, -2, 13);
        self.place_block(orange_terracotta, 13, -4, 13);
        self.place_sand(13, -3, 13);
        self.place_sand(13, -2, 13);
        self.place_block(cut_sandstone, 12, -3, 13);
        self.place_block(chiseled_sandstone, 12, -2, 13);
        self.place_block(orange_terracotta, 16, -4, 16);
        self.place_sand(16, -3, 16);
        self.place_sand(16, -2, 16);
        self.place_block(orange_terracotta, 16, -4, 10);
        self.place_sand(16, -3, 10);
        self.place_sand(16, -2, 10);
        self.place_block(cut_sandstone, 16, -3, 9);
        self.place_block(chiseled_sandstone, 16, -2, 9);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla StructurePiece.generateBox parameters"
    )]
    fn generate_box(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: BlockStateId,
        fill: BlockStateId,
        skip_air: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if skip_air && self.get_block(x, y, z).is_air() {
                        continue;
                    }
                    let state = if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                        fill
                    } else {
                        edge
                    };
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    fn fill_column_down(&mut self, state: BlockStateId, x: i32, start_y: i32, z: i32) {
        let mut pos = self.world_pos(x, start_y, z);
        if !self.clip.contains_blockpos(pos) {
            return;
        }

        while Self::is_replaceable_by_structures(self.region.block_state(pos))
            && pos.y() > self.region.min_y() + 1
        {
            let _ = self
                .region
                .set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
            pos = pos.below();
        }
    }

    fn place_block(&mut self, state: BlockStateId, x: i32, y: i32, z: i32) {
        let pos = self.world_pos(x, y, z);
        if !self.clip.contains_blockpos(pos) {
            return;
        }

        let state = self.transform_state(state);
        let _ = self
            .region
            .set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
        if StructurePiecePlacer::needs_structure_shape_postprocessing(state) {
            self.region.mark_pos_for_postprocessing(pos);
        }
    }

    fn get_block(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        let pos = self.world_pos(x, y, z);
        if self.clip.contains_blockpos(pos) {
            self.region.block_state(pos)
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn transform_state(&self, state: BlockStateId) -> BlockStateId {
        let (mirror, rotation) = StructurePiecePlacer::orientation_transform(self.orientation);
        StructureTemplate::transform_state(self.registry, state, mirror, rotation)
    }

    fn place_sand(&mut self, x: i32, y: i32, z: i32) {
        let pos = self.world_pos(x, y, z);
        self.data
            .potential_suspicious_sand_world_positions
            .push(pos);
    }

    fn place_sand_box(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    self.place_sand(x, y, z);
                }
            }
        }
    }

    fn place_collapsed_roof_piece(&mut self, x: i32, y: i32, z: i32) {
        let state = if self.region.random_mut().next_f32() < 0.33 {
            vanilla_blocks::SANDSTONE.default_state()
        } else {
            vanilla_blocks::SAND.default_state()
        };
        self.place_block(state, x, y, z);
    }

    fn place_collapsed_roof(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, z1: i32) {
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.place_collapsed_roof_piece(x, y0, z);
            }
        }

        let seed_pos = self.world_pos(x0, y0, z0);
        let mut seed_random = LegacyRandom::from_seed(self.region.seed() as u64);
        let splitter = seed_random.next_positional();
        let mut positional_random = splitter.at(seed_pos.x(), seed_pos.y(), seed_pos.z());
        let roof_x = positional_random.next_i32_between(x0, x1);
        let roof_z = positional_random.next_i32_between(z0, z1);
        self.data.random_collapsed_roof_pos = self.world_pos(roof_x, y0, roof_z);
    }

    const fn world_pos(&self, x: i32, y: i32, z: i32) -> BlockPos {
        let world_y = if self.orientation.is_some() {
            y + self.bounding_box.min_y()
        } else {
            y
        };
        let (world_x, world_z) = match self.orientation {
            None | Some(Direction::Up | Direction::Down) => (x, z),
            Some(Direction::North) => {
                (self.bounding_box.min_x() + x, self.bounding_box.max_z() - z)
            }
            Some(Direction::South) => {
                (self.bounding_box.min_x() + x, self.bounding_box.min_z() + z)
            }
            Some(Direction::West) => (self.bounding_box.max_x() - z, self.bounding_box.min_z() + x),
            Some(Direction::East) => (self.bounding_box.min_x() + z, self.bounding_box.min_z() + x),
        };
        BlockPos::new(world_x, world_y, world_z)
    }

    fn is_replaceable_by_structures(state: BlockStateId) -> bool {
        state.is_air()
            || state.has_fluid()
            || state.get_block() == &vanilla_blocks::GLOW_LICHEN
            || state.get_block() == &vanilla_blocks::SEAGRASS
            || state.get_block() == &vanilla_blocks::TALL_SEAGRASS
    }

    fn stairs(facing: Direction) -> BlockStateId {
        vanilla_blocks::SANDSTONE_STAIRS
            .default_state()
            .set_value(&BlockStateProperties::FACING, facing)
    }
}

const fn direction_2d_data_value(direction: Direction) -> usize {
    match direction {
        Direction::South => 0,
        Direction::West => 1,
        Direction::North => 2,
        Direction::East => 3,
        Direction::Down | Direction::Up => {
            panic!("desert pyramid chest directions come from the horizontal plane")
        }
    }
}
