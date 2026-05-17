#![expect(
    clippy::too_many_arguments,
    reason = "spike body placement mirrors vanilla feature state"
)]

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_spike_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &SpikeConfiguration,
        mut origin: BlockPos,
    ) -> bool {
        while region.block_state(origin).is_air() && origin.y() > region.min_y() + 2 {
            origin = origin.below();
        }

        if !Self::test_block_predicate(region, registry, &config.can_place_on, origin) {
            return false;
        }

        origin = origin.above_n(random.next_i32_bounded(4));
        let height = random.next_i32_bounded(4) + 7;
        let width = height / 4 + random.next_i32_bounded(2);
        if width > 1 && random.next_i32_bounded(60) == 0 {
            origin = origin.above_n(10 + random.next_i32_bounded(30));
        }

        let spike_state = Self::block_state_from_data(registry, &config.state);
        Self::place_spike_body(
            region,
            registry,
            random,
            config,
            origin,
            height,
            width,
            spike_state,
        );
        Self::place_spike_base(region, registry, random, config, origin, width, spike_state);

        true
    }

    fn place_spike_body(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &SpikeConfiguration,
        origin: BlockPos,
        height: i32,
        width: i32,
        spike_state: BlockStateId,
    ) {
        for y_offset in 0..height {
            let scale = (1.0 - y_offset as f32 / height as f32) * width as f32;
            let new_width = scale.ceil() as i32;

            for x_offset in -new_width..=new_width {
                let dx = x_offset.abs() as f32 - 0.25;
                for z_offset in -new_width..=new_width {
                    let dz = z_offset.abs() as f32 - 0.25;
                    let inside_radius =
                        (x_offset == 0 && z_offset == 0) || dx * dx + dz * dz <= scale * scale;
                    let on_edge = x_offset == -new_width
                        || x_offset == new_width
                        || z_offset == -new_width
                        || z_offset == new_width;
                    if !inside_radius || (on_edge && random.next_f32() > 0.75) {
                        continue;
                    }

                    let positive_offset = origin.offset(x_offset, y_offset, z_offset);
                    Self::place_spike_block_if_replaceable(
                        region,
                        registry,
                        config,
                        positive_offset,
                        spike_state,
                    );

                    if y_offset != 0 && new_width > 1 {
                        let negative_offset = origin.offset(x_offset, -y_offset, z_offset);
                        Self::place_spike_block_if_replaceable(
                            region,
                            registry,
                            config,
                            negative_offset,
                            spike_state,
                        );
                    }
                }
            }
        }
    }

    fn place_spike_base(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &SpikeConfiguration,
        origin: BlockPos,
        width: i32,
        spike_state: BlockStateId,
    ) {
        let pillar_width = (width - 1).clamp(0, 1);
        for x_offset in -pillar_width..=pillar_width {
            for z_offset in -pillar_width..=pillar_width {
                let mut cursor = origin.offset(x_offset, -1, z_offset);
                let mut run_length = 50;
                if x_offset.abs() == 1 && z_offset.abs() == 1 {
                    run_length = random.next_i32_bounded(5);
                }

                while cursor.y() > 50 {
                    let state = region.block_state(cursor);
                    if !state.is_air()
                        && !Self::test_block_predicate(
                            region,
                            registry,
                            &config.can_replace,
                            cursor,
                        )
                        && state != spike_state
                    {
                        break;
                    }

                    let _ = region.set_block_state(cursor, spike_state, UpdateFlags::UPDATE_ALL);
                    cursor = cursor.below();
                    run_length -= 1;
                    if run_length <= 0 {
                        cursor = cursor.below_n(random.next_i32_bounded(5) + 1);
                        run_length = random.next_i32_bounded(5);
                    }
                }
            }
        }
    }

    fn place_spike_block_if_replaceable(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        config: &SpikeConfiguration,
        pos: BlockPos,
        spike_state: BlockStateId,
    ) {
        let state = region.block_state(pos);
        if state.is_air() || Self::test_block_predicate(region, registry, &config.can_replace, pos)
        {
            let _ = region.set_block_state(pos, spike_state, UpdateFlags::UPDATE_ALL);
        }
    }
}
