use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::blocks::shapes;
use steel_registry::structure::RuinedPortalPlacementData;
use steel_registry::{Registry, TaggedRegistryExt, vanilla_block_tags, vanilla_blocks};
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockPos, BoundingBox, Direction, types::UpdateFlags};

use crate::chunk::heightmap::HeightmapType;
use crate::world::structure::RuinedPortalProperties;
use crate::worldgen::region::WorldGenRegion;

use super::StructurePiecePlacer;

const NETHERRACK_PROBABILITY_BY_DISTANCE: [f32; 14] = [
    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
];
const RUINED_PORTAL_HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

impl StructurePiecePlacer {
    pub(super) fn post_process_ruined_portal(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        vertical_placement: RuinedPortalPlacementData,
        properties: RuinedPortalProperties,
        portal_box: BoundingBox,
        random: &mut WorldgenRandom,
    ) {
        Self::spread_ruined_portal_netherrack(
            region,
            registry,
            vertical_placement,
            properties,
            portal_box,
            random,
        );
        Self::add_ruined_portal_netherrack_drip_columns(region, properties, portal_box, random);

        if !properties.vines && !properties.overgrown {
            return;
        }

        for z in portal_box.min_z..=portal_box.max_z {
            for y in portal_box.min_y..=portal_box.max_y {
                for x in portal_box.min_x..=portal_box.max_x {
                    let pos = BlockPos::new(x, y, z);
                    if properties.vines {
                        Self::maybe_add_ruined_portal_vines(region, pos, random);
                    }
                    if properties.overgrown {
                        Self::maybe_add_ruined_portal_leaves(region, pos, random);
                    }
                }
            }
        }
    }

    fn maybe_add_ruined_portal_vines(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) {
        let state = region.block_state(pos);
        if state.is_air() || state.get_block() == &vanilla_blocks::VINE {
            return;
        }

        let direction = Self::random_ruined_portal_horizontal_direction(random);
        let neighbor_pos = direction.relative(pos);
        if !region.block_state(neighbor_pos).is_air() {
            return;
        }
        if !shapes::is_face_full(state.get_collision_shape(), direction) {
            return;
        }

        let vine_state = vanilla_blocks::VINE
            .default_state()
            .set_value(Self::vine_face_property(direction.opposite()), true);
        let _ = region.set_block_state(neighbor_pos, vine_state, UpdateFlags::UPDATE_ALL);
    }

    fn maybe_add_ruined_portal_leaves(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) {
        if random.next_f32() >= 0.5 {
            return;
        }
        if region.block_state(pos).get_block() != &vanilla_blocks::NETHERRACK {
            return;
        }
        let above = pos.above();
        if !region.block_state(above).is_air() {
            return;
        }

        let leaves = vanilla_blocks::JUNGLE_LEAVES
            .default_state()
            .set_value(&BlockStateProperties::PERSISTENT, true);
        let _ = region.set_block_state(above, leaves, UpdateFlags::UPDATE_ALL);
    }

    fn add_ruined_portal_netherrack_drip_columns(
        region: &mut WorldGenRegion<'_>,
        properties: RuinedPortalProperties,
        portal_box: BoundingBox,
        random: &mut WorldgenRandom,
    ) {
        for x in portal_box.min_x + 1..portal_box.max_x {
            for z in portal_box.min_z + 1..portal_box.max_z {
                let pos = BlockPos::new(x, portal_box.min_y, z);
                if region.block_state(pos).get_block() == &vanilla_blocks::NETHERRACK {
                    Self::add_ruined_portal_netherrack_drip_column(
                        region,
                        properties,
                        pos.below(),
                        random,
                    );
                }
            }
        }
    }

    fn add_ruined_portal_netherrack_drip_column(
        region: &mut WorldGenRegion<'_>,
        properties: RuinedPortalProperties,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) {
        let mut current = pos;
        Self::place_ruined_portal_netherrack_or_magma(region, properties, current, random);
        let mut remaining_cap = 8;
        while remaining_cap > 0 && random.next_f32() < 0.5 {
            current = current.below();
            remaining_cap -= 1;
            Self::place_ruined_portal_netherrack_or_magma(region, properties, current, random);
        }
    }

    fn spread_ruined_portal_netherrack(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        vertical_placement: RuinedPortalPlacementData,
        properties: RuinedPortalProperties,
        portal_box: BoundingBox,
        random: &mut WorldgenRandom,
    ) {
        let follow_ground_surface = matches!(
            vertical_placement,
            RuinedPortalPlacementData::OnLandSurface | RuinedPortalPlacementData::OnOceanFloor
        );
        let center = portal_box.get_center();
        let average_width = i32::midpoint(portal_box.get_x_span(), portal_box.get_z_span());
        let distance_adjustment = random.next_i32_bounded(1.max(8 - average_width / 2));
        let max_distance =
            i32::try_from(NETHERRACK_PROBABILITY_BY_DISTANCE.len()).unwrap_or(i32::MAX);

        for x in center.x() - max_distance..=center.x() + max_distance {
            for z in center.z() - max_distance..=center.z() + max_distance {
                let distance = (x - center.x()).abs() + (z - center.z()).abs();
                let adjusted_distance = 0.max(distance + distance_adjustment);
                if adjusted_distance >= max_distance {
                    continue;
                }

                let probability = NETHERRACK_PROBABILITY_BY_DISTANCE[adjusted_distance as usize];
                if random.next_f64() >= f64::from(probability) {
                    continue;
                }

                let surface_y = Self::ruined_portal_surface_y(region, vertical_placement, x, z);
                let y = if follow_ground_surface {
                    surface_y
                } else {
                    portal_box.min_y.min(surface_y)
                };
                let pos = BlockPos::new(x, y, z);
                if (y - portal_box.min_y).abs() > 3
                    || !Self::can_replace_with_ruined_portal_netherrack_or_magma(
                        region,
                        registry,
                        vertical_placement,
                        pos,
                    )
                {
                    continue;
                }

                Self::place_ruined_portal_netherrack_or_magma(region, properties, pos, random);
                if properties.overgrown {
                    Self::maybe_add_ruined_portal_leaves(region, pos, random);
                }
                Self::add_ruined_portal_netherrack_drip_column(
                    region,
                    properties,
                    pos.below(),
                    random,
                );
            }
        }
    }

    fn can_replace_with_ruined_portal_netherrack_or_magma(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        vertical_placement: RuinedPortalPlacementData,
        pos: BlockPos,
    ) -> bool {
        let state = region.block_state(pos);
        Self::can_block_be_replaced_with_ruined_portal_netherrack_or_magma(
            registry,
            vertical_placement,
            state.get_block(),
        )
    }

    fn can_block_be_replaced_with_ruined_portal_netherrack_or_magma(
        registry: &Registry,
        vertical_placement: RuinedPortalPlacementData,
        block: BlockRef,
    ) -> bool {
        block != &vanilla_blocks::AIR
            && block != &vanilla_blocks::OBSIDIAN
            && !registry
                .blocks
                .is_in_tag(block, &vanilla_block_tags::FEATURES_CANNOT_REPLACE_TAG)
            && (vertical_placement == RuinedPortalPlacementData::InNether
                || block != &vanilla_blocks::LAVA)
    }

    fn place_ruined_portal_netherrack_or_magma(
        region: &mut WorldGenRegion<'_>,
        properties: RuinedPortalProperties,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) {
        let state = if !properties.cold && random.next_f32() < 0.07 {
            vanilla_blocks::MAGMA_BLOCK.default_state()
        } else {
            vanilla_blocks::NETHERRACK.default_state()
        };
        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL);
    }

    fn ruined_portal_surface_y(
        region: &WorldGenRegion<'_>,
        vertical_placement: RuinedPortalPlacementData,
        x: i32,
        z: i32,
    ) -> i32 {
        let heightmap = if vertical_placement == RuinedPortalPlacementData::OnOceanFloor {
            HeightmapType::OceanFloorWg
        } else {
            HeightmapType::WorldSurfaceWg
        };
        region.height_at(heightmap, x, z) - 1
    }

    fn random_ruined_portal_horizontal_direction(random: &mut WorldgenRandom) -> Direction {
        RUINED_PORTAL_HORIZONTAL_DIRECTIONS
            [random.next_i32_bounded(RUINED_PORTAL_HORIZONTAL_DIRECTIONS.len() as i32) as usize]
    }

    const fn vine_face_property(direction: Direction) -> &'static BoolProperty {
        match direction {
            Direction::Up => &BlockStateProperties::UP,
            Direction::North => &BlockStateProperties::NORTH,
            Direction::East => &BlockStateProperties::EAST,
            Direction::South => &BlockStateProperties::SOUTH,
            Direction::West => &BlockStateProperties::WEST,
            Direction::Down => panic!("vine has no down face property"),
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::Registry;

    use super::*;

    #[test]
    fn ruined_portal_netherrack_replacement_matches_vanilla_air_checks() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();

        assert!(
            StructurePiecePlacer::can_block_be_replaced_with_ruined_portal_netherrack_or_magma(
                &registry,
                RuinedPortalPlacementData::InNether,
                &vanilla_blocks::CAVE_AIR,
            )
        );
        assert!(
            !StructurePiecePlacer::can_block_be_replaced_with_ruined_portal_netherrack_or_magma(
                &registry,
                RuinedPortalPlacementData::InNether,
                &vanilla_blocks::AIR,
            )
        );
        assert!(
            !StructurePiecePlacer::can_block_be_replaced_with_ruined_portal_netherrack_or_magma(
                &registry,
                RuinedPortalPlacementData::InNether,
                &vanilla_blocks::OBSIDIAN,
            )
        );
        assert!(
            StructurePiecePlacer::can_block_be_replaced_with_ruined_portal_netherrack_or_magma(
                &registry,
                RuinedPortalPlacementData::InNether,
                &vanilla_blocks::LAVA,
            )
        );
        assert!(
            !StructurePiecePlacer::can_block_be_replaced_with_ruined_portal_netherrack_or_magma(
                &registry,
                RuinedPortalPlacementData::OnLandSurface,
                &vanilla_blocks::LAVA,
            )
        );
    }
}
