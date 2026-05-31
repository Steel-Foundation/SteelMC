use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use steel_registry::vanilla_block_entity_types;

const SPAWN_BONUS_CHEST: &str = "minecraft:chests/spawn_bonus_chest";

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_bonus_chest_feature(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(origin.x());
        let chunk_z = SectionPos::block_to_section_coord(origin.z());
        let mut x_positions =
            Self::shuffled_inclusive_range(chunk_x << 4, (chunk_x << 4) + 15, random);
        let z_positions = Self::shuffled_inclusive_range(chunk_z << 4, (chunk_z << 4) + 15, random);
        let chest = vanilla_blocks::CHEST.default_state();
        let torch = vanilla_blocks::TORCH.default_state();

        for x in x_positions.drain(..) {
            for &z in &z_positions {
                let y = region.height_at(HeightmapType::MotionBlockingNoLeaves, x, z);
                let chest_pos = BlockPos::new(x, y, z);
                let state = region.block_state(chest_pos);
                if state.is_air() || state.get_collision_shape().is_empty() {
                    let _ = region.set_block_state(chest_pos, chest, UpdateFlags::UPDATE_CLIENTS);
                    Self::set_loot_table_block_entity(
                        region,
                        chest_pos,
                        &vanilla_block_entity_types::CHEST,
                        chest,
                        SPAWN_BONUS_CHEST,
                        random.next_i64(),
                    );

                    let torch_behavior = BLOCK_BEHAVIORS.get_behavior(torch.get_block());
                    for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
                        let torch_pos = chest_pos.relative(direction);
                        if torch_behavior.can_survive(torch, region, torch_pos) {
                            let _ = region.set_block_state(
                                torch_pos,
                                torch,
                                UpdateFlags::UPDATE_CLIENTS,
                            );
                        }
                    }

                    return true;
                }
            }
        }

        false
    }

    fn shuffled_inclusive_range(start: i32, end: i32, random: &mut WorldgenRandom) -> Vec<i32> {
        let mut values: Vec<i32> = (start..=end).collect();
        for i in (1..values.len()).rev() {
            let bound = (i + 1) as i32;
            let j = random.next_i32_bounded(bound) as usize;
            values.swap(i, j);
        }
        values
    }
}
