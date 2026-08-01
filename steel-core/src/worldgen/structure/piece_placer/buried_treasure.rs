use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{vanilla_block_entity_types, vanilla_blocks};
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockPos, BlockStateId, BoundingBox, Direction, types::UpdateFlags};

use super::StructurePiecePlacer;
use crate::chunk::heightmap::HeightmapType;
use crate::worldgen::region::WorldGenRegion;

const BURIED_TREASURE_LOOT: &str = "minecraft:chests/buried_treasure";
const VANILLA_DIRECTION_VALUES: [Direction; 6] = [
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

impl StructurePiecePlacer {
    pub(super) fn place_buried_treasure_piece(
        region: &mut WorldGenRegion<'_>,
        bounding_box: &mut BoundingBox,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
    ) -> bool {
        let x = bounding_box.min_x();
        let z = bounding_box.min_z();
        let mut y = region.height_at(HeightmapType::OceanFloorWg, x, z);

        while y > region.min_y() {
            let pos = BlockPos::new(x, y, z);
            let current_state = region.block_state(pos);
            let below_state = region.block_state(pos.below());

            if Self::is_buried_treasure_base(below_state) {
                let soft_state =
                    if !current_state.is_air() && !Self::is_buried_treasure_liquid(current_state) {
                        current_state
                    } else {
                        vanilla_blocks::SAND.default_state()
                    };

                for direction in VANILLA_DIRECTION_VALUES {
                    let relative_pos = pos.relative(direction);
                    let relative_state = region.block_state(relative_pos);
                    if !relative_state.is_air() && !Self::is_buried_treasure_liquid(relative_state)
                    {
                        continue;
                    }

                    let below_relative_pos = relative_pos.below();
                    let below_relative_state = region.block_state(below_relative_pos);
                    let place_state = if direction != Direction::Up
                        && (below_relative_state.is_air()
                            || Self::is_buried_treasure_liquid(below_relative_state))
                    {
                        below_state
                    } else {
                        soft_state
                    };
                    let _ =
                        region.set_block_state(relative_pos, place_state, UpdateFlags::UPDATE_ALL);
                }

                *bounding_box = BoundingBox::from_corners(pos, pos);
                let _ = Self::create_buried_treasure_chest(region, clip, random, pos);
                return true;
            }

            y -= 1;
        }

        true
    }

    fn is_buried_treasure_base(state: BlockStateId) -> bool {
        let block = state.get_block();
        block == &vanilla_blocks::SANDSTONE
            || block == &vanilla_blocks::STONE
            || block == &vanilla_blocks::ANDESITE
            || block == &vanilla_blocks::GRANITE
            || block == &vanilla_blocks::DIORITE
    }

    fn is_buried_treasure_liquid(state: BlockStateId) -> bool {
        let block = state.get_block();
        block == &vanilla_blocks::WATER || block == &vanilla_blocks::LAVA
    }

    fn create_buried_treasure_chest(
        region: &mut WorldGenRegion<'_>,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
        pos: BlockPos,
    ) -> bool {
        if !clip.contains_blockpos(pos)
            || region.block_state(pos).get_block() == &vanilla_blocks::CHEST
        {
            return false;
        }

        let state = Self::reorient_chest(region, pos, vanilla_blocks::CHEST.default_state());
        if !region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS) {
            return false;
        }

        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", BURIED_TREASURE_LOOT);
        nbt.insert("LootTableSeed", random.next_i64());
        region.set_block_entity_data(pos, &vanilla_block_entity_types::CHEST, state, nbt)
    }
}
