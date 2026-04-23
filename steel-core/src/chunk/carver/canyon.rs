//! Canyon (ravine) carver.
//!
//! Mirrors vanilla's `CanyonWorldCarver`. Carves a single long, narrow tunnel
//! per chunk with per-height width variation; runs only from an overworld
//! biome's carver list (`minecraft:canyon`, 1 % probability).

use std::f32::consts::{PI, TAU};

use steel_registry::carver::CanyonCarverConfiguration;
use steel_utils::ChunkPos;
use steel_utils::density::DimensionNoises;
use steel_utils::math::mth;
use steel_utils::random::{Random, legacy_random::LegacyRandom};

use crate::chunk::carver::{
    CarveParams, CarverBlockIds, CarverStyle, CarvingContext, can_reach, carve_ellipsoid,
};
use crate::chunk::carving_mask::CarvingMask;
use crate::chunk::chunk_access::ChunkAccess;

/// Runs a canyon carver pass rooted in `source_pos`. `random` must have been
/// seeded by the caller via `set_large_feature_seed(seed + carver_index,
/// source.x, source.z)` and the `isStartChunk` probability check must have
/// already passed.
///
/// Mirrors vanilla's `CanyonWorldCarver.carve` — one tunnel per chunk, no
/// splits, no rooms.
#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla CanyonWorldCarver.carve signature closely"
)]
pub fn carve_canyon<N, F>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    chunk: &ChunkAccess,
    chunk_min_x: i32,
    chunk_min_z: i32,
    mut biome_getter: F,
    mask: &mut CarvingMask,
    ids: CarverBlockIds,
    config: &CanyonCarverConfiguration,
    source_pos: ChunkPos,
    random: &mut LegacyRandom,
) where
    N: DimensionNoises,
    F: FnMut(i32, i32, i32) -> u16,
{
    // Vanilla: `(this.getRange() * 2 - 1) * 16` = (4*2-1)*16 = 112.
    let max_distance = (carver_range() * 2 - 1) * 16;

    let source_min_x = source_pos.0.x * 16;
    let source_min_z = source_pos.0.y * 16;

    let lava_level_y = config.base.lava_level.resolve_y(ctx.min_y, ctx.gen_depth);
    let params = CarveParams {
        replaceable_tag: &config.base.replaceable_tag,
        lava_level_y,
        style: CarverStyle::Overworld,
        ids,
    };

    let x = f64::from(source_min_x + random.next_i32_bounded(16));
    let y_i = config.base.y.sample(random, ctx.min_y, ctx.gen_depth);
    let y = f64::from(y_i);
    let z = f64::from(source_min_z + random.next_i32_bounded(16));
    let horizontal_rotation = random.next_f32() * TAU;
    let vertical_rotation = config.vertical_rotation.sample(random);
    let y_scale = f64::from(config.base.y_scale.sample(random));
    let thickness = config.shape.thickness.sample(random);
    let distance =
        (f64::from(max_distance) * f64::from(config.shape.distance_factor.sample(random))) as i32;
    let tunnel_seed = random.next_i64();

    do_carve(
        ctx,
        noises,
        chunk,
        chunk_min_x,
        chunk_min_z,
        &mut biome_getter,
        mask,
        &params,
        config,
        tunnel_seed,
        x,
        y,
        z,
        thickness,
        horizontal_rotation,
        vertical_rotation,
        0,
        distance,
        y_scale,
    );
}

const fn carver_range() -> i32 {
    4
}

/// Vanilla `CanyonWorldCarver.doCarve`.
#[expect(
    clippy::too_many_arguments,
    reason = "port of vanilla doCarve parameter list"
)]
fn do_carve<N, F>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    chunk: &ChunkAccess,
    chunk_min_x: i32,
    chunk_min_z: i32,
    biome_getter: &mut F,
    mask: &mut CarvingMask,
    params: &CarveParams<'_>,
    config: &CanyonCarverConfiguration,
    tunnel_seed: i64,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    thickness: f32,
    mut horizontal_rotation: f32,
    mut vertical_rotation: f32,
    step: i32,
    distance: i32,
    y_scale: f64,
) where
    N: DimensionNoises,
    F: FnMut(i32, i32, i32) -> u16,
{
    let mut random = LegacyRandom::from_seed(tunnel_seed as u64);
    let width_factors =
        init_width_factors(ctx.gen_depth, config.shape.width_smoothness, &mut random);
    let mut y_rota: f32 = 0.0;
    let mut x_rota: f32 = 0.0;

    for current_step in step..distance {
        let progress = PI * current_step as f32 / distance as f32;
        let mut horizontal_radius =
            1.5 + f64::from(mth::sin(f64::from(progress))) * f64::from(thickness);
        let mut vertical_radius = horizontal_radius * y_scale;
        horizontal_radius *= f64::from(config.shape.horizontal_radius_factor.sample(&mut random));
        vertical_radius = update_vertical_radius(
            config,
            &mut random,
            vertical_radius,
            distance as f32,
            current_step as f32,
        );

        let xc = mth::cos(f64::from(vertical_rotation));
        let xs = mth::sin(f64::from(vertical_rotation));
        x += f64::from(mth::cos(f64::from(horizontal_rotation)) * xc);
        y += f64::from(xs);
        z += f64::from(mth::sin(f64::from(horizontal_rotation)) * xc);
        vertical_rotation *= 0.7;
        vertical_rotation += x_rota * 0.05;
        horizontal_rotation += y_rota * 0.05;
        x_rota *= 0.8;
        y_rota *= 0.5;
        x_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 2.0;
        y_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 4.0;

        if random.next_i32_bounded(4) == 0 {
            continue;
        }

        if !can_reach(
            chunk_min_x,
            chunk_min_z,
            x,
            z,
            current_step,
            distance,
            thickness,
        ) {
            return;
        }

        let min_y = ctx.min_y;
        let skip_checker = |xd: f64, yd: f64, zd: f64, world_y: i32| {
            should_skip_canyon(&width_factors, min_y, xd, yd, zd, world_y)
        };

        carve_ellipsoid(
            ctx,
            noises,
            chunk,
            chunk_min_x,
            chunk_min_z,
            &mut *biome_getter,
            mask,
            params,
            x,
            y,
            z,
            horizontal_radius,
            vertical_radius,
            skip_checker,
        );
    }
}

/// Vanilla `CanyonWorldCarver.initWidthFactors` — fresh width factor at every
/// `width_smoothness`-th Y level, squared for the radial distance test.
fn init_width_factors(
    gen_depth: i32,
    width_smoothness: i32,
    random: &mut LegacyRandom,
) -> Vec<f32> {
    let depth = gen_depth as usize;
    let mut width_factors = vec![0.0_f32; depth];
    let mut width_factor = 1.0_f32;

    for (y_index, slot) in width_factors.iter_mut().enumerate() {
        if y_index == 0 || random.next_i32_bounded(width_smoothness) == 0 {
            width_factor = 1.0 + random.next_f32() * random.next_f32();
        }
        *slot = width_factor * width_factor;
    }

    width_factors
}

/// Vanilla `CanyonWorldCarver.updateVerticalRadius`.
fn update_vertical_radius(
    config: &CanyonCarverConfiguration,
    random: &mut LegacyRandom,
    vertical_radius: f64,
    distance: f32,
    current_step: f32,
) -> f64 {
    // Vanilla: `Mth.abs(0.5F - currentStep/distance)` → float arithmetic.
    let vertical_multiplier = 1.0_f32 - (0.5 - current_step / distance).abs() * 2.0;
    let factor = config.shape.vertical_radius_default_factor
        + config.shape.vertical_radius_center_factor * vertical_multiplier;
    // `Mth.randomBetween(random, 0.75F, 1.0F)` = 0.75 + nextFloat()*0.25.
    let jitter = 0.75 + random.next_f32() * 0.25;
    f64::from(factor) * vertical_radius * f64::from(jitter)
}

/// Vanilla `CanyonWorldCarver.shouldSkip`.
///
/// Vanilla indexes `widthFactorPerHeight[yIndex - 1]` where
/// `yIndex = world_y - context.getMinGenY()`; i.e. the previous Y's width
/// factor applied to this block's radial test.
fn should_skip_canyon(
    width_factors: &[f32],
    min_y: i32,
    xd: f64,
    yd: f64,
    zd: f64,
    world_y: i32,
) -> bool {
    let y_index = (world_y - min_y - 1) as usize;
    let factor = width_factors[y_index];
    (xd * xd + zd * zd) * f64::from(factor) + yd * yd / 6.0 >= 1.0
}
