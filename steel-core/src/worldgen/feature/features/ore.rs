use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_ore_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &OreConfiguration,
        origin: BlockPos,
    ) -> bool {
        if config.size <= 0 {
            return false;
        }

        let direction = random.next_f32() * std::f32::consts::PI;
        let spread_xz = config.size as f32 / 8.0;
        let spread_xz_ceil = spread_xz.ceil() as i32;
        let max_radius = ((config.size as f32 / 16.0 * 2.0 + 1.0) / 2.0).ceil() as i32;
        let sin = f64::from(direction).sin();
        let cos = f64::from(direction).cos();
        let x0 = f64::from(origin.x()) + sin * f64::from(spread_xz);
        let x1 = f64::from(origin.x()) - sin * f64::from(spread_xz);
        let z0 = f64::from(origin.z()) + cos * f64::from(spread_xz);
        let z1 = f64::from(origin.z()) - cos * f64::from(spread_xz);
        let y0 = f64::from(origin.y() + random.next_i32_bounded(3) - 2);
        let y1 = f64::from(origin.y() + random.next_i32_bounded(3) - 2);
        let x_start = origin.x() - spread_xz_ceil - max_radius;
        let y_start = origin.y() - 2 - max_radius;
        let z_start = origin.z() - spread_xz_ceil - max_radius;
        let size_xz = 2 * (spread_xz_ceil + max_radius);
        let size_y = 2 * (2 + max_radius);

        for x_probe in x_start..=x_start + size_xz {
            for z_probe in z_start..=z_start + size_xz {
                if y_start <= region.height_at(HeightmapType::OceanFloorWg, x_probe, z_probe) {
                    return Self::do_place_ore(
                        region, registry, random, config, x0, x1, z0, z1, y0, y1, x_start, y_start,
                        z_start, size_xz, size_y,
                    );
                }
            }
        }

        false
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla ore vein placement inputs"
    )]
    pub(in crate::worldgen::feature) fn do_place_ore(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &OreConfiguration,
        x0: f64,
        x1: f64,
        z0: f64,
        z1: f64,
        y0: f64,
        y1: f64,
        x_start: i32,
        y_start: i32,
        z_start: i32,
        size_xz: i32,
        size_y: i32,
    ) -> bool {
        let Ok(size) = usize::try_from(config.size) else {
            return false;
        };
        let mut vein_nodes = vec![[0.0; 4]; size];

        for i in 0..size {
            let step = i as f32 / config.size as f32;
            let size_factor = random.next_f64() * f64::from(config.size) / 16.0;
            let radius = ((std::f32::consts::PI * step).sin() as f64 + 1.0) * size_factor + 1.0;
            vein_nodes[i] = [
                lerp(f64::from(step), x0, x1),
                lerp(f64::from(step), y0, y1),
                lerp(f64::from(step), z0, z1),
                radius / 2.0,
            ];
        }

        for i1 in 0..size.saturating_sub(1) {
            if vein_nodes[i1][3] <= 0.0 {
                continue;
            }

            for i2 in i1 + 1..size {
                if vein_nodes[i2][3] <= 0.0 {
                    continue;
                }

                let dx = vein_nodes[i1][0] - vein_nodes[i2][0];
                let dy = vein_nodes[i1][1] - vein_nodes[i2][1];
                let dz = vein_nodes[i1][2] - vein_nodes[i2][2];
                let dr = vein_nodes[i1][3] - vein_nodes[i2][3];
                if dr * dr > dx * dx + dy * dy + dz * dz {
                    if dr > 0.0 {
                        vein_nodes[i2][3] = -1.0;
                    } else {
                        vein_nodes[i1][3] = -1.0;
                    }
                }
            }
        }

        let Some(search_volume) = OreSearchVolume::new(x_start, y_start, z_start, size_xz, size_y)
        else {
            return false;
        };
        let mut placed = 0;
        let mut tested = OreTestedPositions::with_capacity(search_volume.tested_position_count);
        let mut sections = region.bulk_section_access();

        for node in vein_nodes {
            let radius = node[3];
            if radius < 0.0 {
                continue;
            }

            let x_min = floor(node[0] - radius).max(x_start);
            let y_min = floor(node[1] - radius).max(y_start);
            let z_min = floor(node[2] - radius).max(z_start);
            let x_max = floor(node[0] + radius).max(x_min);
            let y_max = floor(node[1] + radius).max(y_min);
            let z_max = floor(node[2] + radius).max(z_min);

            for x in x_min..=x_max {
                let x_distance = (f64::from(x) + 0.5 - node[0]) / radius;
                if x_distance * x_distance >= 1.0 {
                    continue;
                }

                for y in y_min..=y_max {
                    let y_distance = (f64::from(y) + 0.5 - node[1]) / radius;
                    if x_distance * x_distance + y_distance * y_distance >= 1.0 {
                        continue;
                    }

                    for z in z_min..=z_max {
                        let z_distance = (f64::from(z) + 0.5 - node[2]) / radius;
                        if x_distance * x_distance
                            + y_distance * y_distance
                            + z_distance * z_distance
                            >= 1.0
                            || region.is_outside_build_height(y)
                        {
                            continue;
                        }

                        let Some(tested_index) = search_volume.index(x, y, z) else {
                            continue;
                        };
                        if tested.insert(tested_index) {
                            let pos = BlockPos::new(x, y, z);
                            if sections.can_write_to_pos(pos)
                                && Self::try_place_ore_block_in_bulk(
                                    &mut sections,
                                    registry,
                                    random,
                                    config,
                                    pos,
                                )
                            {
                                placed += 1;
                            }
                        }
                    }
                }
            }
        }

        placed > 0
    }

    pub(in crate::worldgen::feature) fn place_scattered_ore_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &OreConfiguration,
        origin: BlockPos,
    ) -> bool {
        if config.size < 0 {
            panic!("scattered ore size {} is negative", config.size);
        }

        let tries = random.next_i32_bounded(config.size + 1);
        for i in 0..tries {
            let max_distance = i.min(7);
            let pos = origin.offset(
                Self::random_scattered_ore_offset(random, max_distance),
                Self::random_scattered_ore_offset(random, max_distance),
                Self::random_scattered_ore_offset(random, max_distance),
            );
            let _ = Self::try_place_ore_block(region, registry, random, config, pos);
        }

        true
    }

    pub(in crate::worldgen::feature) fn random_scattered_ore_offset(
        random: &mut Xoroshiro,
        max_distance: i32,
    ) -> i32 {
        Self::java_round_f32((random.next_f32() - random.next_f32()) * max_distance as f32)
    }

    pub(in crate::worldgen::feature) fn java_round_f32(value: f32) -> i32 {
        (value + 0.5).floor() as i32
    }

    pub(in crate::worldgen::feature) fn try_place_ore_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &OreConfiguration,
        pos: BlockPos,
    ) -> bool {
        let block_state = region.block_state(pos);
        for target in &config.targets {
            if Self::can_place_ore(region, registry, random, config, target, pos, block_state) {
                let state = Self::block_state_from_data(registry, &target.state);
                return region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
            }
        }

        false
    }

    pub(in crate::worldgen::feature) fn try_place_ore_block_in_bulk(
        sections: &mut WorldGenBulkSectionAccess<'_, '_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &OreConfiguration,
        pos: BlockPos,
    ) -> bool {
        let block_state = sections.block_state(pos);
        for target in &config.targets {
            if Self::can_place_ore_in_bulk(
                sections,
                registry,
                random,
                config,
                target,
                pos,
                block_state,
            ) {
                let state = Self::block_state_from_data(registry, &target.state);
                return sections.set_block_state(pos, state);
            }
        }

        false
    }

    pub(in crate::worldgen::feature) fn can_place_ore(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &OreConfiguration,
        target: &OreTarget,
        pos: BlockPos,
        block_state: BlockStateId,
    ) -> bool {
        if !Self::rule_test_matches(registry, &target.target, block_state) {
            return false;
        }

        if Self::should_skip_air_check(random, config.discard_chance_on_air_exposure) {
            return true;
        }

        !Self::is_adjacent_to_air(region, registry, pos)
    }

    pub(in crate::worldgen::feature) fn can_place_ore_in_bulk(
        sections: &mut WorldGenBulkSectionAccess<'_, '_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &OreConfiguration,
        target: &OreTarget,
        pos: BlockPos,
        block_state: BlockStateId,
    ) -> bool {
        if !Self::rule_test_matches(registry, &target.target, block_state) {
            return false;
        }

        if Self::should_skip_air_check(random, config.discard_chance_on_air_exposure) {
            return true;
        }

        !Self::is_adjacent_to_air_in_bulk(sections, registry, pos)
    }

    pub(in crate::worldgen::feature) fn rule_test_matches(
        registry: &Registry,
        target: &RuleTest,
        state: BlockStateId,
    ) -> bool {
        let Some(block) = registry.blocks.by_state_id(state) else {
            panic!("ore feature received invalid block state id {}", state.0);
        };

        match target {
            RuleTest::BlockMatch { block: block_key } => {
                let Some(target_block) = registry.blocks.by_key(block_key) else {
                    panic!("ore rule test references unknown block {block_key}");
                };
                block == target_block
            }
            RuleTest::TagMatch { tag } => registry.blocks.is_in_tag(block, tag),
        }
    }

    pub(in crate::worldgen::feature) fn should_skip_air_check(
        random: &mut Xoroshiro,
        discard_chance_on_air_exposure: f32,
    ) -> bool {
        if discard_chance_on_air_exposure <= 0.0 {
            true
        } else if discard_chance_on_air_exposure >= 1.0 {
            false
        } else {
            random.next_f32() >= discard_chance_on_air_exposure
        }
    }

    pub(in crate::worldgen::feature) fn is_adjacent_to_air(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
    ) -> bool {
        Direction::ALL.into_iter().any(|direction| {
            let neighbor = region.block_state(pos.relative(direction));
            Self::is_air_block_state(registry, neighbor)
        })
    }

    pub(in crate::worldgen::feature) fn is_adjacent_to_air_in_bulk(
        sections: &mut WorldGenBulkSectionAccess<'_, '_>,
        registry: &Registry,
        pos: BlockPos,
    ) -> bool {
        Direction::ALL.into_iter().any(|direction| {
            let neighbor = sections.block_state(pos.relative(direction));
            Self::is_air_block_state(registry, neighbor)
        })
    }

    pub(in crate::worldgen::feature) fn is_air_block_state(
        registry: &Registry,
        state: BlockStateId,
    ) -> bool {
        let Some(block) = registry.blocks.by_state_id(state) else {
            panic!("feature received invalid block state id {}", state.0);
        };
        block.config.is_air
    }
}

struct OreTestedPositions {
    words: Vec<u64>,
}

#[derive(Clone, Copy)]
struct OreSearchVolume {
    x_start: i32,
    y_start: i32,
    z_start: i32,
    size_xz: i64,
    size_y: i64,
    tested_position_count: usize,
}

impl OreSearchVolume {
    fn new(x_start: i32, y_start: i32, z_start: i32, size_xz: i32, size_y: i32) -> Option<Self> {
        let size_xz = i64::from(size_xz);
        let size_y = i64::from(size_y);
        if size_xz <= 0 || size_y <= 0 {
            return None;
        }

        let tested_position_count =
            usize::try_from(size_xz.checked_mul(size_y)?.checked_mul(size_xz)?).ok()?;
        Some(Self {
            x_start,
            y_start,
            z_start,
            size_xz,
            size_y,
            tested_position_count,
        })
    }

    fn index(self, x: i32, y: i32, z: i32) -> Option<usize> {
        let x_offset = i64::from(x) - i64::from(self.x_start);
        let y_offset = i64::from(y) - i64::from(self.y_start);
        let z_offset = i64::from(z) - i64::from(self.z_start);
        if x_offset < 0 || y_offset < 0 || z_offset < 0 {
            return None;
        }

        // Matches vanilla OreFeature's BitSet index layout.
        let index = x_offset
            .checked_add(y_offset.checked_mul(self.size_xz)?)?
            .checked_add(
                z_offset
                    .checked_mul(self.size_xz)?
                    .checked_mul(self.size_y)?,
            )?;
        usize::try_from(index).ok()
    }
}

impl OreTestedPositions {
    fn with_capacity(bit_count: usize) -> Self {
        Self {
            words: vec![0; bit_count.div_ceil(u64::BITS as usize)],
        }
    }

    fn insert(&mut self, index: usize) -> bool {
        let word_index = index / u64::BITS as usize;
        if word_index >= self.words.len() {
            self.words.resize(word_index + 1, 0);
        }

        let mask = 1_u64 << (index % u64::BITS as usize);
        let word = &mut self.words[word_index];
        if *word & mask != 0 {
            return false;
        }

        *word |= mask;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{OreSearchVolume, OreTestedPositions};

    #[test]
    fn ore_tested_position_index_matches_vanilla_layout() {
        let volume = OreSearchVolume::new(10, 60, 20, 4, 6);
        assert_eq!(volume.and_then(|volume| volume.index(12, 63, 21)), Some(38));
    }

    #[test]
    fn ore_tested_positions_deduplicate_and_grow() {
        let mut tested = OreTestedPositions::with_capacity(1);
        assert!(tested.insert(0));
        assert!(!tested.insert(0));
        assert!(tested.insert(130));
        assert!(!tested.insert(130));
    }
}
