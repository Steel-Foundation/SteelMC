use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use std::f32::consts::{PI, TAU};
use steel_math::trig;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::value_providers::FloatProvider;

struct LargeDripstone {
    root: BlockPos,
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

struct WindOffsetter {
    origin_y: i32,
    wind_speed: Option<(f64, f64)>,
    max_offset: i32,
}

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_large_dripstone_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &LargeDripstoneConfiguration,
        origin: BlockPos,
    ) -> bool {
        if !Self::is_empty_or_water(region.block_state(origin)) {
            return false;
        }

        let Some(column) = Self::scan_dripstone_column(
            region,
            origin,
            config.floor_to_ceiling_search_range,
            Self::is_empty_or_water,
            |state| {
                let block = state.get_block();
                block == &vanilla_blocks::DRIPSTONE_BLOCK
                    || Self::block_matches_holder_set(block, &config.replaceable_blocks)
                    || block == &vanilla_blocks::LAVA
            },
        ) else {
            return false;
        };

        let (Some(floor), Some(ceiling)) = (column.floor, column.ceiling) else {
            return false;
        };
        let column_height = ceiling - floor - 1;
        if column_height < 4 {
            return false;
        }

        let max_radius_from_height =
            (column_height as f32 * config.max_column_radius_to_cave_height_ratio) as i32;
        let min_radius = config.column_radius.min();
        let max_radius = max_radius_from_height.clamp(min_radius, config.column_radius.max());
        let radius = random.next_i32_between(min_radius, max_radius);

        let mut stalactite = LargeDripstone::new(
            origin.at_y(ceiling - 1),
            false,
            random,
            radius,
            config.stalactite_bluntness,
            config.height_scale,
        );
        let mut stalagmite = LargeDripstone::new(
            origin.at_y(floor + 1),
            true,
            random,
            radius,
            config.stalagmite_bluntness,
            config.height_scale,
        );
        let wind =
            if stalactite.is_suitable_for_wind(config) && stalagmite.is_suitable_for_wind(config) {
                WindOffsetter::new(origin.y(), random, config.wind_speed, 16 - radius)
            } else {
                WindOffsetter::no_wind()
            };

        let stalactite_base_embedded =
            stalactite.move_back_until_base_is_inside_stone(region, &wind);
        let stalagmite_base_embedded =
            stalagmite.move_back_until_base_is_inside_stone(region, &wind);
        if stalactite_base_embedded {
            stalactite.place_blocks(region, random, &wind);
        }

        if stalagmite_base_embedded {
            stalagmite.place_blocks(region, random, &wind);
        }

        true
    }

    pub(in crate::worldgen::feature) fn dripstone_height(
        mut xz_distance_from_center: f64,
        dripstone_radius: f64,
        scale: f64,
        bluntness: f64,
    ) -> f64 {
        if xz_distance_from_center < bluntness {
            xz_distance_from_center = bluntness;
        }

        let cutoff = 0.384;
        let r = xz_distance_from_center / dripstone_radius * cutoff;
        let part1 = 0.75 * r.powf(4.0 / 3.0);
        let part2 = r.powf(2.0 / 3.0);
        let part3 = (1.0 / 3.0) * r.ln();
        let height_relative_to_max_radius = (scale * (part1 - part2 - part3)).max(0.0);
        height_relative_to_max_radius / cutoff * dripstone_radius
    }

    fn is_circle_mostly_embedded_in_stone(
        region: &WorldGenRegion<'_>,
        center: BlockPos,
        xz_radius: i32,
    ) -> bool {
        if Self::is_empty_or_water_or_lava(region.block_state(center)) {
            return false;
        }

        let angle_increment = 6.0 / xz_radius as f32;
        let mut angle = 0.0f32;
        while angle < TAU {
            let dx = (trig::cos(f64::from(angle)) * xz_radius as f32) as i32;
            let dz = (trig::sin(f64::from(angle)) * xz_radius as f32) as i32;
            if Self::is_empty_or_water_or_lava(region.block_state(center.offset(dx, 0, dz))) {
                return false;
            }
            angle += angle_increment;
        }

        true
    }

    fn random_f32_between(random: &mut WorldgenRandom, min: f32, max: f32) -> f32 {
        random.next_f32() * (max - min) + min
    }
}

impl LargeDripstone {
    fn new(
        root: BlockPos,
        pointing_up: bool,
        random: &mut WorldgenRandom,
        radius: i32,
        bluntness: FloatProvider,
        height_scale: FloatProvider,
    ) -> Self {
        Self {
            root,
            pointing_up,
            radius,
            bluntness: f64::from(bluntness.sample(random)),
            scale: f64::from(height_scale.sample(random)),
        }
    }

    fn height(&self) -> i32 {
        self.height_at_radius(0.0)
    }

    fn move_back_until_base_is_inside_stone(
        &mut self,
        region: &WorldGenRegion<'_>,
        wind: &WindOffsetter,
    ) -> bool {
        while self.radius > 1 {
            let mut new_root = self.root;
            let max_tries = 10.min(self.height());

            for _ in 0..max_tries {
                if FeatureDecorationRunner::is_lava(region, new_root) {
                    return false;
                }

                if FeatureDecorationRunner::is_circle_mostly_embedded_in_stone(
                    region,
                    wind.offset(new_root),
                    self.radius,
                ) {
                    self.root = new_root;
                    return true;
                }

                new_root = new_root.relative(if self.pointing_up {
                    Direction::Down
                } else {
                    Direction::Up
                });
            }

            self.radius /= 2;
        }

        false
    }

    fn height_at_radius(&self, check_radius: f32) -> i32 {
        FeatureDecorationRunner::dripstone_height(
            f64::from(check_radius),
            f64::from(self.radius),
            self.scale,
            self.bluntness,
        ) as i32
    }

    fn place_blocks(
        &self,
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        wind: &WindOffsetter,
    ) {
        for dx in -self.radius..=self.radius {
            for dz in -self.radius..=self.radius {
                let current_radius = ((dx * dx + dz * dz) as f32).sqrt();
                if current_radius > self.radius as f32 {
                    continue;
                }

                let mut height = self.height_at_radius(current_radius);
                if height <= 0 {
                    continue;
                }

                if random.next_f32() < 0.2 {
                    height = (height as f32
                        * FeatureDecorationRunner::random_f32_between(random, 0.8, 1.0))
                        as i32;
                }

                let mut pos = self.root.offset(dx, 0, dz);
                let mut has_been_out_of_stone = false;
                let max_y = if self.pointing_up {
                    region.height_at(HeightmapType::WorldSurfaceWg, pos.x(), pos.z())
                } else {
                    i32::MAX
                };

                for _ in 0..height {
                    if pos.y() >= max_y {
                        break;
                    }

                    let wind_adjusted_pos = wind.offset(pos);
                    let state = region.block_state(wind_adjusted_pos);
                    if FeatureDecorationRunner::is_empty_or_water_or_lava(state) {
                        has_been_out_of_stone = true;
                        let _ = region.set_block_state(
                            wind_adjusted_pos,
                            vanilla_blocks::DRIPSTONE_BLOCK.default_state(),
                            UpdateFlags::UPDATE_CLIENTS,
                        );
                    } else if has_been_out_of_stone
                        && state.get_block().has_tag(&BlockTag::BASE_STONE_OVERWORLD)
                    {
                        break;
                    }

                    pos = pos.relative(if self.pointing_up {
                        Direction::Up
                    } else {
                        Direction::Down
                    });
                }
            }
        }
    }

    fn is_suitable_for_wind(&self, config: &LargeDripstoneConfiguration) -> bool {
        self.radius >= config.min_radius_for_wind
            && self.bluntness >= f64::from(config.min_bluntness_for_wind)
    }
}

impl WindOffsetter {
    fn new(
        origin_y: i32,
        random: &mut WorldgenRandom,
        wind_speed_range: FloatProvider,
        max_offset: i32,
    ) -> Self {
        let speed = wind_speed_range.sample(random);
        let direction = FeatureDecorationRunner::random_f32_between(random, 0.0, PI);
        Self {
            origin_y,
            wind_speed: Some((
                f64::from(trig::cos(f64::from(direction)) * speed),
                f64::from(trig::sin(f64::from(direction)) * speed),
            )),
            max_offset,
        }
    }

    const fn no_wind() -> Self {
        Self {
            origin_y: 0,
            wind_speed: None,
            max_offset: 0,
        }
    }

    fn offset(&self, pos: BlockPos) -> BlockPos {
        let Some((wind_x, wind_z)) = self.wind_speed else {
            return pos;
        };

        let dy = self.origin_y - pos.y();
        pos.offset(
            fast_floor(wind_x * f64::from(dy)).clamp(-self.max_offset, self.max_offset),
            0,
            fast_floor(wind_z * f64::from(dy)).clamp(-self.max_offset, self.max_offset),
        )
    }
}
