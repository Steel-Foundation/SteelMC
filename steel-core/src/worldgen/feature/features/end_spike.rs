use std::f64::consts::PI;
use std::sync::Arc;

use glam::DVec3;
use steel_registry::vanilla_entities;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use crate::entity::{Entity, entities::EndCrystalEntity, next_entity_id};

const END_SPIKE_COUNT: usize = 10;
const END_SPIKE_DISTANCE: f64 = 42.0;
const END_SPIKE_ANGLE_STEP: f64 = PI / 10.0;
const END_SPIKE_CLEAR_AIR_MIN_Y: i32 = 65;
const END_SPIKE_CAGE_RADIUS: i32 = 2;
const END_SPIKE_CAGE_HEIGHT: i32 = 3;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_end_spike_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &EndSpikeConfiguration,
        origin: BlockPos,
    ) -> bool {
        let generated_spikes;
        let spikes = if config.spikes.is_empty() {
            generated_spikes = Self::end_spikes_for_level(region.seed());
            &generated_spikes
        } else {
            &config.spikes
        };

        for spike in spikes {
            if Self::end_spike_center_is_within_chunk(spike, origin) {
                Self::place_end_spike(region, random, config, spike);
            }
        }

        true
    }

    fn end_spikes_for_level(seed: i64) -> Vec<EndSpike> {
        let mut seed_random = LegacyRandom::from_seed(seed as u64);
        let cache_key = seed_random.next_i64() & 65_535;
        let mut random = LegacyRandom::from_seed(cache_key as u64);
        let mut sizes = [0_i32; END_SPIKE_COUNT];
        for (index, size) in sizes.iter_mut().enumerate() {
            *size = index as i32;
        }
        for bound in (2..=END_SPIKE_COUNT).rev() {
            let swap_to = random.next_i32_bounded(bound as i32) as usize;
            sizes.swap(bound - 1, swap_to);
        }

        sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                let angle = 2.0 * (-PI + END_SPIKE_ANGLE_STEP * index as f64);
                EndSpike {
                    center_x: fast_floor(END_SPIKE_DISTANCE * angle.cos()),
                    center_z: fast_floor(END_SPIKE_DISTANCE * angle.sin()),
                    radius: 2 + size / 3,
                    height: 76 + size * 3,
                    guarded: *size == 1 || *size == 2,
                }
            })
            .collect()
    }

    const fn end_spike_center_is_within_chunk(spike: &EndSpike, origin: BlockPos) -> bool {
        SectionPos::block_to_section_coord(origin.x())
            == SectionPos::block_to_section_coord(spike.center_x)
            && SectionPos::block_to_section_coord(origin.z())
                == SectionPos::block_to_section_coord(spike.center_z)
    }

    fn place_end_spike(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &EndSpikeConfiguration,
        spike: &EndSpike,
    ) {
        Self::place_end_spike_body(region, spike);
        if spike.guarded {
            Self::place_end_spike_cage(region, spike);
        }
        Self::place_end_spike_crystal(region, random, config, spike);
    }

    fn place_end_spike_body(region: &mut WorldGenRegion<'_>, spike: &EndSpike) {
        let radius_squared_plus_one = spike.radius * spike.radius + 1;
        let obsidian = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::OBSIDIAN);
        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);

        for x in (spike.center_x - spike.radius)..=(spike.center_x + spike.radius) {
            for y in region.min_y()..=(spike.height + 10) {
                for z in (spike.center_z - spike.radius)..=(spike.center_z + spike.radius) {
                    let pos = BlockPos::new(x, y, z);
                    if Self::end_spike_inside_radius(spike, pos, radius_squared_plus_one)
                        && y < spike.height
                    {
                        let _ = region.set_block_state(pos, obsidian, UpdateFlags::UPDATE_ALL);
                    } else if y > END_SPIKE_CLEAR_AIR_MIN_Y {
                        let _ = region.set_block_state(pos, air, UpdateFlags::UPDATE_ALL);
                    }
                }
            }
        }
    }

    fn end_spike_inside_radius(
        spike: &EndSpike,
        pos: BlockPos,
        radius_squared_plus_one: i32,
    ) -> bool {
        let dx = f64::from(pos.x() - spike.center_x);
        let dz = f64::from(pos.z() - spike.center_z);
        dx * dx + dz * dz <= f64::from(radius_squared_plus_one)
    }

    fn place_end_spike_cage(region: &mut WorldGenRegion<'_>, spike: &EndSpike) {
        for dx in -END_SPIKE_CAGE_RADIUS..=END_SPIKE_CAGE_RADIUS {
            for dz in -END_SPIKE_CAGE_RADIUS..=END_SPIKE_CAGE_RADIUS {
                for dy in 0..=END_SPIKE_CAGE_HEIGHT {
                    let touches_width_limit = dx.abs() == END_SPIKE_CAGE_RADIUS;
                    let touches_depth_limit = dz.abs() == END_SPIKE_CAGE_RADIUS;
                    let top = dy == END_SPIKE_CAGE_HEIGHT;
                    if !touches_width_limit && !touches_depth_limit && !top {
                        continue;
                    }

                    let x_edge = touches_width_limit || top;
                    let z_edge = touches_depth_limit || top;
                    let state = Self::end_spike_iron_bars_state(x_edge, z_edge, dx, dz);
                    let pos =
                        BlockPos::new(spike.center_x + dx, spike.height + dy, spike.center_z + dz);
                    let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL);
                }
            }
        }
    }

    fn end_spike_iron_bars_state(x_edge: bool, z_edge: bool, dx: i32, dz: i32) -> BlockStateId {
        REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::IRON_BARS)
            .set_value(
                &BlockStateProperties::NORTH,
                x_edge && dz != -END_SPIKE_CAGE_RADIUS,
            )
            .set_value(
                &BlockStateProperties::SOUTH,
                x_edge && dz != END_SPIKE_CAGE_RADIUS,
            )
            .set_value(
                &BlockStateProperties::WEST,
                z_edge && dx != -END_SPIKE_CAGE_RADIUS,
            )
            .set_value(
                &BlockStateProperties::EAST,
                z_edge && dx != END_SPIKE_CAGE_RADIUS,
            )
    }

    fn place_end_spike_crystal(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &EndSpikeConfiguration,
        spike: &EndSpike,
    ) {
        let position = DVec3::new(
            f64::from(spike.center_x) + 0.5,
            f64::from(spike.height + 1),
            f64::from(spike.center_z) + 0.5,
        );
        let crystal = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            position,
            region.weak_world(),
        ));
        crystal.set_beam_target(config.crystal_beam_target.map(BlockPos));
        crystal.set_invulnerable(config.crystal_invulnerable);
        crystal.snap_to(position, random.next_f32() * 360.0, 0.0);
        let _ = region.add_fresh_entity(crystal);

        let crystal_pos = BlockPos::from(position);
        let bedrock = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::BEDROCK);
        let fire = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::FIRE);
        let _ = region.set_block_state(crystal_pos.below(), bedrock, UpdateFlags::UPDATE_ALL);
        let _ = region.set_block_state(crystal_pos, fire, UpdateFlags::UPDATE_ALL);
    }
}
