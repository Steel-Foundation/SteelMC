#![expect(
    clippy::too_many_lines,
    reason = "monster room placement is kept linear to mirror vanilla"
)]

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use steel_registry::vanilla_block_entity_types;

const SIMPLE_DUNGEON: &str = "minecraft:chests/simple_dungeon";
const MONSTER_ROOM_MOBS: [&str; 4] = [
    "minecraft:skeleton",
    "minecraft:zombie",
    "minecraft:zombie",
    "minecraft:spider",
];

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_monster_room_feature(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let xr = random.next_i32_bounded(2) + 2;
        let min_x = -xr - 1;
        let max_x = xr + 1;
        let zr = random.next_i32_bounded(2) + 2;
        let min_z = -zr - 1;
        let max_z = zr + 1;
        let mut hole_count = 0;

        for dx in min_x..=max_x {
            for dy in -1..=4 {
                for dz in min_z..=max_z {
                    let hole_pos = origin.offset(dx, dy, dz);
                    let state = region.block_state(hole_pos);
                    let solid = state.is_solid();
                    if dy == -1 && !solid {
                        return false;
                    }
                    if dy == 4 && !solid {
                        return false;
                    }
                    if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                        && dy == 0
                        && state.is_air()
                        && region.block_state(hole_pos.above()).is_air()
                    {
                        hole_count += 1;
                    }
                }
            }
        }

        if !(1..=5).contains(&hole_count) {
            return false;
        }

        let air = vanilla_blocks::CAVE_AIR.default_state();
        let mossy_cobble = vanilla_blocks::MOSSY_COBBLESTONE.default_state();
        let cobble = vanilla_blocks::COBBLESTONE.default_state();

        for dx in min_x..=max_x {
            for dy in (-1..=3).rev() {
                for dz in min_z..=max_z {
                    let wall_block = origin.offset(dx, dy, dz);
                    let wall_state = region.block_state(wall_block);
                    if dx == min_x
                        || dy == -1
                        || dz == min_z
                        || dx == max_x
                        || dy == 4
                        || dz == max_z
                    {
                        if wall_block.y() >= region.min_y()
                            && !region.block_state(wall_block.below()).is_solid()
                        {
                            let _ = region.set_block_state(
                                wall_block,
                                air,
                                UpdateFlags::UPDATE_CLIENTS,
                            );
                        } else if wall_state.is_solid()
                            && wall_state.get_block() != &vanilla_blocks::CHEST
                        {
                            let state = if dy == -1 && random.next_i32_bounded(4) != 0 {
                                mossy_cobble
                            } else {
                                cobble
                            };
                            let _ =
                                Self::safe_set_feature_block(region, registry, wall_block, state);
                        }
                    } else if wall_state.get_block() != &vanilla_blocks::CHEST
                        && wall_state.get_block() != &vanilla_blocks::SPAWNER
                    {
                        let _ = Self::safe_set_feature_block(region, registry, wall_block, air);
                    }
                }
            }
        }

        for _ in 0..2 {
            for _ in 0..3 {
                let x = origin.x() + random.next_i32_bounded(xr * 2 + 1) - xr;
                let z = origin.z() + random.next_i32_bounded(zr * 2 + 1) - zr;
                let chest_pos = BlockPos::new(x, origin.y(), z);
                if region.block_state(chest_pos).is_air() {
                    let wall_count = Self::VANILLA_HORIZONTAL_DIRECTIONS
                        .iter()
                        .filter(|&&direction| {
                            region.block_state(chest_pos.relative(direction)).is_solid()
                        })
                        .count();

                    if wall_count == 1 {
                        let chest = Self::reorient_chest(
                            region,
                            chest_pos,
                            vanilla_blocks::CHEST.default_state(),
                        );
                        if Self::safe_set_feature_block(region, registry, chest_pos, chest) {
                            Self::set_loot_table_block_entity(
                                region,
                                chest_pos,
                                &vanilla_block_entity_types::CHEST,
                                chest,
                                SIMPLE_DUNGEON,
                                random.next_i64(),
                            );
                            break;
                        }
                    }
                }
            }
        }

        let spawner = vanilla_blocks::SPAWNER.default_state();
        if Self::safe_set_feature_block(region, registry, origin, spawner) {
            let entity_id = MONSTER_ROOM_MOBS[random.next_i32_bounded(4) as usize];
            Self::set_spawner_entity(region, origin, spawner, entity_id);
        }

        true
    }

    fn reorient_chest(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockStateId {
        let mut solid_neighbor = None;

        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            let relative_pos = pos.relative(direction);
            let neighbor = region.block_state(relative_pos);
            if neighbor.get_block() == &vanilla_blocks::CHEST {
                return state;
            }

            if neighbor.is_solid_render() {
                if solid_neighbor.is_some() {
                    solid_neighbor = None;
                    break;
                }
                solid_neighbor = Some(direction);
            }
        }

        if let Some(direction) = solid_neighbor {
            state.set_value(
                &BlockStateProperties::HORIZONTAL_FACING,
                direction.opposite(),
            )
        } else {
            let mut lock_dir = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
            let mut relative_pos = pos.relative(lock_dir);
            if region.block_state(relative_pos).is_solid_render() {
                lock_dir = lock_dir.opposite();
                relative_pos = pos.relative(lock_dir);
            }
            if region.block_state(relative_pos).is_solid_render() {
                lock_dir = lock_dir.rotate_y_clockwise();
                relative_pos = pos.relative(lock_dir);
            }
            if region.block_state(relative_pos).is_solid_render() {
                lock_dir = lock_dir.opposite();
            }
            state.set_value(&BlockStateProperties::HORIZONTAL_FACING, lock_dir)
        }
    }
}
