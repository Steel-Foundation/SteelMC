use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_disk_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &DiskConfiguration,
        origin: BlockPos,
    ) -> bool {
        let top = origin.y() + config.half_height;
        let bottom = origin.y() - config.half_height - 1;
        let radius = config.radius.sample(random);
        let mut placed_any = false;

        Self::for_each_vanilla_between_closed(
            origin.offset(-radius, 0, -radius),
            origin.offset(radius, 0, radius),
            |column_pos| {
                let dx = column_pos.x() - origin.x();
                let dz = column_pos.z() - origin.z();
                if dx * dx + dz * dz <= radius * radius {
                    placed_any |= Self::place_disk_column(
                        region, registry, random, config, top, bottom, column_pos,
                    );
                }
            },
        );

        placed_any
    }

    pub(in crate::worldgen::feature) fn place_disk_column(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &DiskConfiguration,
        top: i32,
        bottom: i32,
        column_pos: BlockPos,
    ) -> bool {
        let mut placed_any = false;
        let mut placed_above = false;

        for y in (bottom + 1..=top).rev() {
            let pos = BlockPos::new(column_pos.x(), y, column_pos.z());
            if Self::test_block_predicate(region, registry, &config.target, pos) {
                if let Some(state) = Self::sample_block_state_provider_optional(
                    region,
                    registry,
                    random,
                    &config.state_provider,
                    pos,
                ) {
                    let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
                    if !placed_above {
                        Self::mark_above_for_postprocessing(region, pos);
                    }
                    placed_any = true;
                    placed_above = true;
                }
            } else {
                placed_above = false;
            }
        }

        placed_any
    }

    pub(in crate::worldgen::feature) fn mark_above_for_postprocessing(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
    ) {
        let mut mark_pos = pos;
        for _ in 0..2 {
            mark_pos = mark_pos.above();
            if region.block_state(mark_pos).is_air() {
                return;
            }
            region.mark_pos_for_postprocessing(mark_pos);
        }
    }
}
