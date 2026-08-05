use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_spring_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        config: &SpringConfiguration,
        origin: BlockPos,
    ) -> bool {
        if !Self::block_matches_holder_set(
            region.block_state(origin.above()).get_block(),
            &config.valid_blocks,
        ) {
            return false;
        }

        if config.requires_block_below
            && !Self::block_matches_holder_set(
                region.block_state(origin.below()).get_block(),
                &config.valid_blocks,
            )
        {
            return false;
        }

        let current_state = region.block_state(origin);
        if !current_state.is_air()
            && !Self::block_matches_holder_set(current_state.get_block(), &config.valid_blocks)
        {
            return false;
        }

        let rock_count = [
            origin.west(),
            origin.east(),
            origin.north(),
            origin.south(),
            origin.below(),
        ]
        .into_iter()
        .filter(|&pos| {
            Self::block_matches_holder_set(
                region.block_state(pos).get_block(),
                &config.valid_blocks,
            )
        })
        .count();

        let hole_count = [
            origin.west(),
            origin.east(),
            origin.north(),
            origin.south(),
            origin.below(),
        ]
        .into_iter()
        .filter(|&pos| region.block_state(pos).is_air())
        .count();

        let Ok(expected_rock_count) = usize::try_from(config.rock_count) else {
            panic!(
                "spring feature rock_count {} is negative",
                config.rock_count
            );
        };
        let Ok(expected_hole_count) = usize::try_from(config.hole_count) else {
            panic!(
                "spring feature hole_count {} is negative",
                config.hole_count
            );
        };

        if rock_count != expected_rock_count || hole_count != expected_hole_count {
            return false;
        }

        let fluid_state = Self::fluid_state_from_data(&config.state);
        let block_state = Self::legacy_block_from_fluid_state(registry, fluid_state);
        let placed = region.set_block_state(origin, block_state, UpdateFlags::UPDATE_CLIENTS);
        if placed {
            let _ = region.schedule_fluid_tick_default(origin, fluid_state.fluid_id, 0);
        }
        placed
    }
}
