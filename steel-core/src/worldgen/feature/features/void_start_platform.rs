use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

const PLATFORM_OFFSET_X: i32 = 8;
const PLATFORM_OFFSET_Y: i32 = 3;
const PLATFORM_OFFSET_Z: i32 = 8;
const PLATFORM_RADIUS: i32 = 16;
const PLATFORM_RADIUS_CHUNKS: i32 = 1;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_void_start_platform_feature(
        region: &mut WorldGenRegion<'_>,
        origin: BlockPos,
    ) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(origin.x());
        let chunk_z = SectionPos::block_to_section_coord(origin.z());
        let platform_origin_chunk_x = SectionPos::block_to_section_coord(PLATFORM_OFFSET_X);
        let platform_origin_chunk_z = SectionPos::block_to_section_coord(PLATFORM_OFFSET_Z);

        if Self::checkerboard_distance(
            chunk_x,
            chunk_z,
            platform_origin_chunk_x,
            platform_origin_chunk_z,
        ) > PLATFORM_RADIUS_CHUNKS
        {
            return true;
        }

        let platform_origin = BlockPos::new(
            PLATFORM_OFFSET_X,
            origin.y() + PLATFORM_OFFSET_Y,
            PLATFORM_OFFSET_Z,
        );
        let stone = vanilla_blocks::STONE.default_state();
        let cobblestone = vanilla_blocks::COBBLESTONE.default_state();
        let min_x = chunk_x * 16;
        let min_z = chunk_z * 16;

        for z in min_z..=min_z + 15 {
            for x in min_x..=min_x + 15 {
                if Self::checkerboard_distance(platform_origin.x(), platform_origin.z(), x, z)
                    <= PLATFORM_RADIUS
                {
                    let pos = BlockPos::new(x, platform_origin.y(), z);
                    let state = if pos == platform_origin {
                        cobblestone
                    } else {
                        stone
                    };
                    let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
                }
            }
        }

        true
    }

    const fn checkerboard_distance(xa: i32, za: i32, xb: i32, zb: i32) -> i32 {
        let dx = if xa >= xb { xa - xb } else { xb - xa };
        let dz = if za >= zb { za - zb } else { zb - za };
        if dx > dz { dx } else { dz }
    }
}

#[cfg(test)]
mod tests {
    use super::FeatureDecorationRunner;

    #[test]
    fn void_start_platform_checkerboard_distance_matches_vanilla() {
        assert_eq!(
            FeatureDecorationRunner::checkerboard_distance(0, 0, 0, 0),
            0
        );
        assert_eq!(
            FeatureDecorationRunner::checkerboard_distance(0, 0, 1, -1),
            1
        );
        assert_eq!(
            FeatureDecorationRunner::checkerboard_distance(-2, 3, 4, 1),
            6
        );
    }
}
