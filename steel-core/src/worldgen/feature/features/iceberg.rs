#![expect(
    clippy::too_many_lines,
    reason = "iceberg placement is kept linear to preserve vanilla parity"
)]

use std::cmp::Ordering;
use std::f64::consts::PI;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_iceberg_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &BlockStateData,
        origin: BlockPos,
    ) -> bool {
        let origin = BlockPos::new(origin.x(), region.sea_level(), origin.z());
        let snow_on_top = random.next_f64() > 0.7;
        let main_block_state = Self::block_state_from_data(registry, config);
        let shape_angle = random.next_f64() * 2.0 * PI;
        let shape_ellipse_a = 11 - random.next_i32_bounded(5);
        let shape_ellipse_c = 3 + random.next_i32_bounded(3);
        let is_ellipse = random.next_f64() > 0.7;
        let mut over_water_height = if is_ellipse {
            random.next_i32_bounded(6) + 6
        } else {
            random.next_i32_bounded(15) + 3
        };
        if !is_ellipse && random.next_f64() > 0.9 {
            over_water_height += random.next_i32_bounded(19) + 7;
        }

        let under_water_height = (over_water_height + random.next_i32_bounded(11)).min(18);
        let width =
            (over_water_height + random.next_i32_bounded(7) - random.next_i32_bounded(5)).min(11);
        let a = if is_ellipse { shape_ellipse_a } else { 11 };

        for x_offset in -a..a {
            for z_offset in -a..a {
                for y_offset in 0..over_water_height {
                    let radius = if is_ellipse {
                        Self::height_dependent_radius_ellipse(y_offset, over_water_height, width)
                    } else {
                        Self::height_dependent_radius_round(
                            random,
                            y_offset,
                            over_water_height,
                            width,
                        )
                    };

                    if is_ellipse || x_offset < radius {
                        Self::generate_iceberg_block(
                            region,
                            random,
                            origin,
                            over_water_height,
                            x_offset,
                            y_offset,
                            z_offset,
                            radius,
                            a,
                            is_ellipse,
                            shape_ellipse_c,
                            shape_angle,
                            snow_on_top,
                            main_block_state,
                        );
                    }
                }
            }
        }

        Self::smooth_iceberg(
            region,
            origin,
            width,
            over_water_height,
            is_ellipse,
            shape_ellipse_a,
        );

        for x_offset in -a..a {
            for z_offset in -a..a {
                for y_offset in (-(under_water_height - 1)..=-1).rev() {
                    let new_a = if is_ellipse {
                        ((a as f32)
                            * (1.0 - (y_offset as f32).powi(2) / (under_water_height as f32 * 8.0)))
                            .ceil() as i32
                    } else {
                        a
                    };
                    let radius = Self::height_dependent_radius_steep(
                        random,
                        -y_offset,
                        under_water_height,
                        width,
                    );

                    if x_offset < radius {
                        Self::generate_iceberg_block(
                            region,
                            random,
                            origin,
                            under_water_height,
                            x_offset,
                            y_offset,
                            z_offset,
                            radius,
                            new_a,
                            is_ellipse,
                            shape_ellipse_c,
                            shape_angle,
                            snow_on_top,
                            main_block_state,
                        );
                    }
                }
            }
        }

        let do_cut_out = if is_ellipse {
            random.next_f64() > 0.1
        } else {
            random.next_f64() > 0.7
        };
        if do_cut_out {
            Self::generate_iceberg_cut_out(
                region,
                random,
                width,
                over_water_height,
                origin,
                is_ellipse,
                shape_ellipse_a,
                shape_angle,
                shape_ellipse_c,
            );
        }

        true
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Mirrors vanilla IcebergFeature call shape"
    )]
    fn generate_iceberg_cut_out(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        width: i32,
        height: i32,
        global_origin: BlockPos,
        is_ellipse: bool,
        shape_ellipse_a: i32,
        shape_angle: f64,
        shape_ellipse_c: i32,
    ) {
        let random_sign_x = if random.next_bool() { -1 } else { 1 };
        let random_sign_z = if random.next_bool() { -1 } else { 1 };
        let mut x_offset = random.next_i32_bounded((width / 2 - 2).max(1));
        if random.next_bool() {
            x_offset = width / 2 + 1 - random.next_i32_bounded((width - width / 2 - 1).max(1));
        }

        let mut z_offset = random.next_i32_bounded((width / 2 - 2).max(1));
        if random.next_bool() {
            z_offset = width / 2 + 1 - random.next_i32_bounded((width - width / 2 - 1).max(1));
        }

        if is_ellipse {
            let offset = random.next_i32_bounded((shape_ellipse_a - 5).max(1));
            x_offset = offset;
            z_offset = offset;
        }

        let local_origin = BlockPos::new(random_sign_x * x_offset, 0, random_sign_z * z_offset);
        let angle = if is_ellipse {
            shape_angle + PI / 2.0
        } else {
            random.next_f64() * 2.0 * PI
        };

        for y_offset in 0..height - 3 {
            let radius = Self::height_dependent_radius_round(random, y_offset, height, width);
            Self::carve_iceberg(
                region,
                radius,
                y_offset,
                global_origin,
                false,
                angle,
                local_origin,
                shape_ellipse_a,
                shape_ellipse_c,
            );
        }

        let mut y_offset = -1;
        while y_offset > -height + random.next_i32_bounded(5) {
            let radius = Self::height_dependent_radius_steep(random, -y_offset, height, width);
            Self::carve_iceberg(
                region,
                radius,
                y_offset,
                global_origin,
                true,
                angle,
                local_origin,
                shape_ellipse_a,
                shape_ellipse_c,
            );
            y_offset -= 1;
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Mirrors vanilla IcebergFeature call shape"
    )]
    fn carve_iceberg(
        region: &mut WorldGenRegion<'_>,
        radius: i32,
        y_offset: i32,
        global_origin: BlockPos,
        under_water: bool,
        angle: f64,
        local_origin: BlockPos,
        shape_ellipse_a: i32,
        shape_ellipse_c: i32,
    ) {
        let a = radius + 1 + shape_ellipse_a / 3;
        let c = (radius - 3).min(3) + shape_ellipse_c / 2 - 1;

        for x_offset in -a..a {
            for z_offset in -a..a {
                let signed_distance =
                    Self::signed_distance_ellipse(x_offset, z_offset, local_origin, a, c, angle);
                if signed_distance.partial_cmp(&0.0) != Some(Ordering::Less) {
                    continue;
                }

                let pos = global_origin.offset(x_offset, y_offset, z_offset);
                let state = region.block_state(pos);
                if !Self::is_iceberg_state(state)
                    && state.get_block() != &vanilla_blocks::SNOW_BLOCK
                {
                    continue;
                }

                if under_water {
                    Self::set_iceberg_block(region, pos, vanilla_blocks::WATER.default_state());
                } else {
                    Self::set_iceberg_block(region, pos, vanilla_blocks::AIR.default_state());
                    Self::remove_floating_snow_layer(region, pos);
                }
            }
        }
    }

    fn remove_floating_snow_layer(region: &mut WorldGenRegion<'_>, pos: BlockPos) {
        let above = pos.above();
        if region.block_state(above).get_block() == &vanilla_blocks::SNOW {
            Self::set_iceberg_block(region, above, vanilla_blocks::AIR.default_state());
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Mirrors vanilla IcebergFeature call shape"
    )]
    fn generate_iceberg_block(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        height: i32,
        x_offset: i32,
        y_offset: i32,
        z_offset: i32,
        radius: i32,
        a: i32,
        is_ellipse: bool,
        shape_ellipse_c: i32,
        shape_angle: f64,
        snow_on_top: bool,
        main_block_state: BlockStateId,
    ) {
        let signed_distance = if is_ellipse {
            Self::signed_distance_ellipse(
                x_offset,
                z_offset,
                BlockPos::ZERO,
                a,
                Self::get_ellipse_c(y_offset, height, shape_ellipse_c),
                shape_angle,
            )
        } else {
            Self::signed_distance_circle(x_offset, z_offset, BlockPos::ZERO, radius, random)
        };

        if signed_distance.partial_cmp(&0.0) != Some(Ordering::Less) {
            return;
        }

        let compare_value = if is_ellipse {
            -0.5
        } else {
            -6.0 - f64::from(random.next_i32_bounded(3))
        };
        if signed_distance > compare_value && random.next_f64() > 0.9 {
            return;
        }

        let pos = origin.offset(x_offset, y_offset, z_offset);
        Self::set_iceberg_shape_block(
            region,
            random,
            pos,
            height - y_offset,
            height,
            is_ellipse,
            snow_on_top,
            main_block_state,
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Mirrors vanilla IcebergFeature call shape"
    )]
    fn set_iceberg_shape_block(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        height_difference: i32,
        height: i32,
        is_ellipse: bool,
        snow_on_top: bool,
        main_block_state: BlockStateId,
    ) {
        let state = region.block_state(pos);
        if !state.is_air()
            && state.get_block() != &vanilla_blocks::SNOW_BLOCK
            && state.get_block() != &vanilla_blocks::ICE
            && state.get_block() != &vanilla_blocks::WATER
        {
            return;
        }

        let randomness = !is_ellipse || random.next_f64() > 0.05;
        if snow_on_top && state.get_block() != &vanilla_blocks::WATER {
            let divisor = if is_ellipse { 3 } else { 2 };
            let limit = f64::from(random.next_i32_bounded((height / divisor).max(1)))
                + f64::from(height) * 0.6;
            if f64::from(height_difference) <= limit && randomness {
                Self::set_iceberg_block(region, pos, vanilla_blocks::SNOW_BLOCK.default_state());
                return;
            }
        }

        Self::set_iceberg_block(region, pos, main_block_state);
    }

    const fn get_ellipse_c(y_offset: i32, height: i32, shape_ellipse_c: i32) -> i32 {
        if y_offset > 0 && height - y_offset <= 3 {
            shape_ellipse_c - (4 - (height - y_offset))
        } else {
            shape_ellipse_c
        }
    }

    fn signed_distance_circle(
        x_offset: i32,
        z_offset: i32,
        origin: BlockPos,
        radius: i32,
        random: &mut WorldgenRandom,
    ) -> f64 {
        let offset = 10.0 * random.next_f32().clamp(0.2, 0.8) / radius as f32;
        f64::from(offset)
            + f64::from((x_offset - origin.x()).pow(2))
            + f64::from((z_offset - origin.z()).pow(2))
            - f64::from(radius.pow(2))
    }

    fn signed_distance_ellipse(
        x_offset: i32,
        z_offset: i32,
        origin: BlockPos,
        a: i32,
        c: i32,
        angle: f64,
    ) -> f64 {
        ((f64::from(x_offset - origin.x()) * angle.cos()
            - f64::from(z_offset - origin.z()) * angle.sin())
            / f64::from(a))
        .powi(2)
            + ((f64::from(x_offset - origin.x()) * angle.sin()
                + f64::from(z_offset - origin.z()) * angle.cos())
                / f64::from(c))
            .powi(2)
            - 1.0
    }

    fn height_dependent_radius_round(
        random: &mut WorldgenRandom,
        y_offset: i32,
        height: i32,
        width: i32,
    ) -> i32 {
        let k = 3.5 - random.next_f32();
        let mut scale = (1.0 - (y_offset as f32).powi(2) / (height as f32 * k)) * width as f32;
        if height > 15 + random.next_i32_bounded(5) {
            let temp_y_offset = if y_offset < 3 + random.next_i32_bounded(6) {
                y_offset / 2
            } else {
                y_offset
            };
            scale = (1.0 - temp_y_offset as f32 / (height as f32 * k * 0.4)) * width as f32;
        }

        (scale / 2.0).ceil() as i32
    }

    fn height_dependent_radius_ellipse(y_offset: i32, height: i32, width: i32) -> i32 {
        let scale = (1.0 - (y_offset as f32).powi(2) / height as f32) * width as f32;
        (scale / 2.0).ceil() as i32
    }

    fn height_dependent_radius_steep(
        random: &mut WorldgenRandom,
        y_offset: i32,
        height: i32,
        width: i32,
    ) -> i32 {
        let k = 1.0 + random.next_f32() / 2.0;
        let scale = (1.0 - y_offset as f32 / (height as f32 * k)) * width as f32;
        (scale / 2.0).ceil() as i32
    }

    fn is_iceberg_state(state: BlockStateId) -> bool {
        let block = state.get_block();
        block == &vanilla_blocks::PACKED_ICE
            || block == &vanilla_blocks::SNOW_BLOCK
            || block == &vanilla_blocks::BLUE_ICE
    }

    fn smooth_iceberg(
        region: &mut WorldGenRegion<'_>,
        origin: BlockPos,
        width: i32,
        height: i32,
        is_ellipse: bool,
        shape_ellipse_a: i32,
    ) {
        let a = if is_ellipse {
            shape_ellipse_a
        } else {
            width / 2
        };

        for x_offset in -a..=a {
            for z_offset in -a..=a {
                for y_offset in 0..=height {
                    let pos = origin.offset(x_offset, y_offset, z_offset);
                    let state = region.block_state(pos);
                    if !Self::is_iceberg_state(state) && state.get_block() != &vanilla_blocks::SNOW
                    {
                        continue;
                    }

                    if region.block_state(pos.below()).is_air() {
                        Self::set_iceberg_block(region, pos, vanilla_blocks::AIR.default_state());
                        Self::set_iceberg_block(
                            region,
                            pos.above(),
                            vanilla_blocks::AIR.default_state(),
                        );
                    } else if Self::is_iceberg_state(state) {
                        let side_count = [pos.west(), pos.east(), pos.north(), pos.south()]
                            .into_iter()
                            .filter(|side| !Self::is_iceberg_state(region.block_state(*side)))
                            .count();

                        if side_count >= 3 {
                            Self::set_iceberg_block(
                                region,
                                pos,
                                vanilla_blocks::AIR.default_state(),
                            );
                        }
                    }
                }
            }
        }
    }

    fn set_iceberg_block(region: &mut WorldGenRegion<'_>, pos: BlockPos, state: BlockStateId) {
        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL);
    }
}
