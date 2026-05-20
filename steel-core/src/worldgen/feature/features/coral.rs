use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_coral_claw_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let Some(coral) =
            Self::random_block_from_tag(registry, random, &vanilla_block_tags::CORAL_BLOCKS_TAG)
        else {
            return false;
        };

        let state = coral.default_state();
        if !Self::place_coral_block(region, registry, random, origin, state) {
            return false;
        }

        let claw_direction = Self::random_horizontal_direction(random);
        let n_branches = random.next_i32_bounded(2) + 2;
        let possible_directions = Self::shuffled_directions(
            random,
            [
                claw_direction,
                claw_direction.rotate_y_clockwise(),
                claw_direction.rotate_y_counter_clockwise(),
            ],
        );

        for branch_direction in possible_directions.into_iter().take(n_branches as usize) {
            let mut pos = origin.relative(branch_direction);
            let sideway_length = random.next_i32_bounded(2) + 1;
            let segment_direction;
            let inway_length;
            if branch_direction == claw_direction {
                segment_direction = claw_direction;
                inway_length = random.next_i32_bounded(3) + 2;
            } else {
                pos = pos.above();
                segment_direction = if random.next_i32_bounded(2) == 0 {
                    branch_direction
                } else {
                    Direction::Up
                };
                inway_length = random.next_i32_bounded(3) + 3;
            }

            for _ in 0..sideway_length {
                if !Self::place_coral_block(region, registry, random, pos, state) {
                    break;
                }
                pos = pos.relative(segment_direction);
            }

            pos = pos.relative(segment_direction.opposite()).above();

            for _ in 0..inway_length {
                pos = pos.relative(claw_direction);
                if !Self::place_coral_block(region, registry, random, pos, state) {
                    break;
                }

                if random.next_f32() < 0.25 {
                    pos = pos.above();
                }
            }
        }

        true
    }

    pub(in crate::worldgen::feature) fn place_coral_mushroom_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let Some(coral) =
            Self::random_block_from_tag(registry, random, &vanilla_block_tags::CORAL_BLOCKS_TAG)
        else {
            return false;
        };

        let state = coral.default_state();
        let height = random.next_i32_bounded(3) + 3;
        let width = random.next_i32_bounded(3) + 3;
        let length = random.next_i32_bounded(3) + 3;
        let sink_value = random.next_i32_bounded(3) + 1;

        for x in 0..=width {
            for y in 0..=height {
                for z in 0..=length {
                    let pos = origin.offset(x, y - sink_value, z);
                    if (y != 0 && y != height || x != 0 && x != width && z != 0 && z != length)
                        && ((x != 0 && x != width) || (z != 0 && z != length))
                        && (x == 0 || x == width || y == 0 || y == height || z == 0 || z == length)
                        && random.next_f32() >= 0.1
                    {
                        let _ = Self::place_coral_block(region, registry, random, pos, state);
                    }
                }
            }
        }

        true
    }

    pub(in crate::worldgen::feature) fn place_coral_tree_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let Some(coral) =
            Self::random_block_from_tag(registry, random, &vanilla_block_tags::CORAL_BLOCKS_TAG)
        else {
            return false;
        };

        let state = coral.default_state();
        let mut pos = origin;
        let trunk_height = random.next_i32_bounded(3) + 1;
        for _ in 0..trunk_height {
            if !Self::place_coral_block(region, registry, random, pos, state) {
                return true;
            }
            pos = pos.above();
        }

        let trunk_top = pos;
        let n_branches = random.next_i32_bounded(3) + 2;
        let directions = Self::shuffled_directions(random, Self::VANILLA_HORIZONTAL_DIRECTIONS);

        for branch_direction in directions.into_iter().take(n_branches as usize) {
            pos = trunk_top.relative(branch_direction);
            let branch_height = random.next_i32_bounded(5) + 2;
            let mut segment_length = 0;

            for j in 0..branch_height {
                if !Self::place_coral_block(region, registry, random, pos, state) {
                    break;
                }
                segment_length += 1;
                pos = pos.above();
                if j == 0 || (segment_length >= 2 && random.next_f32() < 0.25) {
                    pos = pos.relative(branch_direction);
                    segment_length = 0;
                }
            }
        }

        true
    }

    fn place_coral_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        state: BlockStateId,
    ) -> bool {
        let above = pos.above();
        let target_state = region.block_state(pos);
        if target_state.get_block() != &vanilla_blocks::WATER
            && !registry
                .blocks
                .is_in_tag(target_state.get_block(), &vanilla_block_tags::CORALS_TAG)
        {
            return false;
        }

        if region.block_state(above).get_block() != &vanilla_blocks::WATER {
            return false;
        }

        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL);
        if random.next_f32() < 0.25 {
            if let Some(coral) =
                Self::random_block_from_tag(registry, random, &vanilla_block_tags::CORALS_TAG)
            {
                let _ = region.set_block_state(
                    above,
                    coral.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                );
            }
        } else if random.next_f32() < 0.05 {
            let pickles = random.next_i32_bounded(4) as u8 + 1;
            let sea_pickle = vanilla_blocks::SEA_PICKLE
                .default_state()
                .set_value(&BlockStateProperties::PICKLES, pickles);
            let _ = region.set_block_state(above, sea_pickle, UpdateFlags::UPDATE_CLIENTS);
        }

        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            if random.next_f32() >= 0.2 {
                continue;
            }

            let relative_pos = pos.relative(direction);
            if region.block_state(relative_pos).get_block() != &vanilla_blocks::WATER {
                continue;
            }

            if let Some(coral) =
                Self::random_block_from_tag(registry, random, &vanilla_block_tags::WALL_CORALS_TAG)
            {
                let mut coral_fan_state = coral.default_state();
                if coral_fan_state
                    .try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
                    .is_some()
                {
                    coral_fan_state = coral_fan_state
                        .set_value(&BlockStateProperties::HORIZONTAL_FACING, direction);
                }
                let _ = region.set_block_state(
                    relative_pos,
                    coral_fan_state,
                    UpdateFlags::UPDATE_CLIENTS,
                );
            }
        }

        true
    }

    pub(super) fn random_block_from_tag(
        registry: &Registry,
        random: &mut WorldgenRandom,
        tag: &Identifier,
    ) -> Option<BlockRef> {
        let blocks = registry.blocks.get_tag(tag)?;
        if blocks.is_empty() {
            return None;
        }

        let Ok(block_count) = i32::try_from(blocks.len()) else {
            panic!("block tag {tag} has too many entries to choose randomly");
        };
        Some(blocks[random.next_i32_bounded(block_count) as usize])
    }
}
