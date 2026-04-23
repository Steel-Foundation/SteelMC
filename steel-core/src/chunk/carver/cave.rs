//! Cave carver (overworld + nether variants).
//!
//! Mirrors vanilla's `CaveWorldCarver` + `NetherWorldCarver`. Single entry
//! point `carve_cave` dispatched off a [`CaveKind`] — vanilla's overrides
//! for nether (cave bound, thickness multiplier, y scale, per-block
//! placement) are captured as kind-specific constants so the tunnel
//! recursion logic stays shared.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use steel_registry::carver::CaveCarverConfiguration;
use steel_utils::ChunkPos;
use steel_utils::density::DimensionNoises;
use steel_utils::math::mth;
use steel_utils::random::{Random, legacy_random::LegacyRandom};

use crate::chunk::carver::{
    CarveParams, CarveSkipChecker, CarverBlockIds, CarverStyle, CarvingContext, can_reach,
    carve_ellipsoid,
};
use crate::chunk::carving_mask::CarvingMask;
use crate::chunk::chunk_access::ChunkAccess;

/// Which cave carver flavor to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaveKind {
    /// `minecraft:cave` / `minecraft:cave_extra_underground`.
    Overworld,
    /// `minecraft:nether_cave`.
    Nether,
}

impl CaveKind {
    /// Vanilla `CaveWorldCarver.getCaveBound` (15) or `NetherWorldCarver`'s
    /// override (10).
    const fn cave_bound(self) -> i32 {
        match self {
            Self::Overworld => 15,
            Self::Nether => 10,
        }
    }

    /// Vanilla `CaveWorldCarver.getYScale` (1.0) or `NetherWorldCarver`'s
    /// override (5.0).
    const fn y_scale(self) -> f64 {
        match self {
            Self::Overworld => 1.0,
            Self::Nether => 5.0,
        }
    }

    const fn style(self) -> CarverStyle {
        match self {
            Self::Overworld => CarverStyle::Overworld,
            Self::Nether => CarverStyle::Nether,
        }
    }

    /// Vanilla `getThickness`. Nether has a completely separate formula — it
    /// skips the `nextInt(10) == 0` branch and doubles a 2-draw base value.
    fn thickness(self, random: &mut impl Random) -> f32 {
        match self {
            Self::Overworld => {
                // CaveWorldCarver.getThickness:
                //   thickness = nextFloat()*2 + nextFloat();
                //   if (nextInt(10) == 0) thickness *= nextFloat()*nextFloat()*3 + 1;
                let mut thickness = random.next_f32() * 2.0 + random.next_f32();
                if random.next_i32_bounded(10) == 0 {
                    thickness *= random.next_f32() * random.next_f32() * 3.0 + 1.0;
                }
                thickness
            }
            Self::Nether => {
                // NetherWorldCarver.getThickness override:
                //   return (nextFloat()*2 + nextFloat()) * 2;
                (random.next_f32() * 2.0 + random.next_f32()) * 2.0
            }
        }
    }
}

/// Runs one cave-carver pass rooted in `source_pos`. `random` must have
/// been seeded by the caller via
/// `LegacyRandom::set_large_feature_seed(seed + carver_index, cx, cz)` and
/// the `isStartChunk` probability check must have already passed.
///
/// Mirrors vanilla's `CaveWorldCarver.carve` / `NetherWorldCarver.carve`
/// (which inherits the cave variant).
#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla CaveWorldCarver.carve signature closely"
)]
pub fn carve_cave<N, F>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    chunk: &ChunkAccess,
    chunk_min_x: i32,
    chunk_min_z: i32,
    mut biome_getter: F,
    mask: &mut CarvingMask,
    ids: CarverBlockIds,
    config: &CaveCarverConfiguration,
    kind: CaveKind,
    source_pos: ChunkPos,
    random: &mut LegacyRandom,
) where
    N: DimensionNoises,
    F: FnMut(i32, i32, i32) -> u16,
{
    // Vanilla: `SectionPos.sectionToBlockCoord(this.getRange() * 2 - 1)`
    //        = (4 * 2 - 1) * 16 = 112
    let max_distance = (carver_range() * 2 - 1) * 16;

    // Triple-nested `random.nextInt(random.nextInt(...)+1)+1` gives a heavily
    // right-skewed distribution of starts per chunk. Split into locals so the
    // Java-style nesting doesn't overlap `&mut random` borrows.
    let bound = kind.cave_bound();
    let inner = random.next_i32_bounded(bound);
    let mid = random.next_i32_bounded(inner + 1);
    let cave_count = random.next_i32_bounded(mid + 1);

    let source_min_x = source_pos.0.x * 16;
    let source_min_z = source_pos.0.y * 16;

    let lava_level_y = config.base.lava_level.resolve_y(ctx.min_y, ctx.gen_depth);
    let params = CarveParams {
        replaceable_tag: &config.base.replaceable_tag,
        lava_level_y,
        style: kind.style(),
        ids,
    };

    for _ in 0..cave_count {
        let x = f64::from(source_min_x + random.next_i32_bounded(16));
        let y = f64::from(config.base.y.sample(random, ctx.min_y, ctx.gen_depth));
        let z = f64::from(source_min_z + random.next_i32_bounded(16));

        let horizontal_radius_multiplier =
            f64::from(config.horizontal_radius_multiplier.sample(random));
        let vertical_radius_multiplier =
            f64::from(config.vertical_radius_multiplier.sample(random));
        let floor_level = f64::from(config.floor_level.sample(random));

        // Vanilla `CaveWorldCarver.shouldSkip`: skip blocks below the noisy
        // floor OR outside the unit sphere in ellipsoid-local coords (xd²+yd²+zd² ≥ 1).
        // Without the sphere test we'd carve cylinders, not ellipsoids — badly
        // visible as a circular over-carve at each tunnel Y level.
        let skip_checker = move |xd: f64, yd: f64, zd: f64, _world_y: i32| {
            yd <= floor_level || xd * xd + yd * yd + zd * zd >= 1.0
        };

        let mut tunnels = 1i32;
        if random.next_i32_bounded(4) == 0 {
            let y_scale = f64::from(config.base.y_scale.sample(random));
            let thickness = 1.0 + random.next_f32() * 6.0;
            create_room(
                ctx,
                noises,
                chunk,
                chunk_min_x,
                chunk_min_z,
                &mut biome_getter,
                mask,
                &params,
                x,
                y,
                z,
                thickness,
                y_scale,
                &skip_checker,
            );
            tunnels += random.next_i32_bounded(4);
        }

        for _ in 0..tunnels {
            let horizontal_rotation = random.next_f32() * TAU;
            let vertical_rotation = (random.next_f32() - 0.5) / 4.0;
            let thickness = kind.thickness(random);
            let distance = max_distance - random.next_i32_bounded(max_distance / 4);
            let tunnel_seed = random.next_i64();
            create_tunnel(
                ctx,
                noises,
                chunk,
                chunk_min_x,
                chunk_min_z,
                &mut biome_getter,
                mask,
                &params,
                tunnel_seed,
                x,
                y,
                z,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                thickness,
                horizontal_rotation,
                vertical_rotation,
                0,
                distance,
                kind.y_scale(),
                skip_checker,
            );
        }
    }
}

/// Vanilla `WorldCarver.getRange()` — range in chunks. Used by both cave
/// and canyon carvers. 4 chunks in each direction.
const fn carver_range() -> i32 {
    4
}

/// Vanilla `CaveWorldCarver.createRoom`. Carves a single ellipsoid at the
/// tunnel origin, offset by +1 on X.
#[expect(
    clippy::too_many_arguments,
    reason = "port of vanilla createRoom parameter list"
)]
fn create_room<N, F, S>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    chunk: &ChunkAccess,
    chunk_min_x: i32,
    chunk_min_z: i32,
    biome_getter: &mut F,
    mask: &mut CarvingMask,
    params: &CarveParams<'_>,
    x: f64,
    y: f64,
    z: f64,
    thickness: f32,
    y_scale: f64,
    skip_checker: S,
) where
    N: DimensionNoises,
    F: FnMut(i32, i32, i32) -> u16,
    S: CarveSkipChecker,
{
    // Vanilla: `1.5 + Mth.sin((float)(Math.PI / 2)) * thickness`. The argument
    // is a float (π/2 cast to f32), looked up in the SIN table; the result
    // equals 1.0f exactly, so the table detour doesn't matter here.
    let horizontal_radius = 1.5 + f64::from(mth::sin(f64::from(FRAC_PI_2))) * f64::from(thickness);
    let vertical_radius = horizontal_radius * y_scale;
    carve_ellipsoid(
        ctx,
        noises,
        chunk,
        chunk_min_x,
        chunk_min_z,
        biome_getter,
        mask,
        params,
        x + 1.0,
        y,
        z,
        horizontal_radius,
        vertical_radius,
        skip_checker,
    );
}

/// Vanilla `CaveWorldCarver.createTunnel`. Steps along a curve, carving an
/// ellipsoid per step, with occasional mid-tunnel splits.
#[expect(
    clippy::too_many_arguments,
    reason = "port of vanilla createTunnel parameter list"
)]
#[expect(
    clippy::too_many_lines,
    reason = "faithful port; splitting would obscure the carver-state coupling"
)]
#[expect(
    clippy::similar_names,
    reason = "x_rota / y_rota / vertical_rotation / horizontal_rotation mirror vanilla"
)]
fn create_tunnel<N, F, S>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    chunk: &ChunkAccess,
    chunk_min_x: i32,
    chunk_min_z: i32,
    biome_getter: &mut F,
    mask: &mut CarvingMask,
    params: &CarveParams<'_>,
    tunnel_seed: i64,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    horizontal_radius_multiplier: f64,
    vertical_radius_multiplier: f64,
    thickness: f32,
    mut horizontal_rotation: f32,
    mut vertical_rotation: f32,
    step: i32,
    dist: i32,
    y_scale: f64,
    skip_checker: S,
) where
    N: DimensionNoises,
    F: FnMut(i32, i32, i32) -> u16,
    S: CarveSkipChecker + Copy,
{
    let mut random = LegacyRandom::from_seed(tunnel_seed as u64);
    let split_point = random.next_i32_bounded(dist / 2) + dist / 4;
    let steep = random.next_i32_bounded(6) == 0;
    let mut y_rota: f32 = 0.0;
    let mut x_rota: f32 = 0.0;

    for current_step in step..dist {
        // Vanilla: `Mth.sin((float)Math.PI * currentStep / dist) * thickness`.
        // The `(float)Math.PI * currentStep / dist` term keeps float precision
        // through to the `Mth.sin` argument before widening to double.
        let progress_arg = PI * current_step as f32 / dist as f32;
        let horizontal_radius =
            1.5 + f64::from(mth::sin(f64::from(progress_arg))) * f64::from(thickness);
        let vertical_radius = horizontal_radius * y_scale;
        let cos_x = mth::cos(f64::from(vertical_rotation));
        x += f64::from(mth::cos(f64::from(horizontal_rotation)) * cos_x);
        y += f64::from(mth::sin(f64::from(vertical_rotation)));
        z += f64::from(mth::sin(f64::from(horizontal_rotation)) * cos_x);
        vertical_rotation *= if steep { 0.92 } else { 0.7 };
        vertical_rotation += x_rota * 0.1;
        horizontal_rotation += y_rota * 0.1;
        x_rota *= 0.9;
        y_rota *= 0.75;
        x_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 2.0;
        y_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 4.0;

        if current_step == split_point && thickness > 1.0 {
            // Vanilla evaluates args left-to-right: `nextLong()` (seed) is
            // arg 5, `nextFloat() * 0.5 + 0.5` (thickness) is arg 11 — so the
            // seed is drawn before the thickness.
            let sub_seed_a = random.next_i64();
            let sub_thickness_a = random.next_f32() * 0.5 + 0.5;
            let sub_rotation_a = horizontal_rotation - FRAC_PI_2;
            let sub_vert_a = vertical_rotation / 3.0;
            let sub_seed_b = random.next_i64();
            let sub_thickness_b = random.next_f32() * 0.5 + 0.5;
            let sub_rotation_b = horizontal_rotation + FRAC_PI_2;
            let sub_vert_b = vertical_rotation / 3.0;
            create_tunnel(
                ctx,
                noises,
                chunk,
                chunk_min_x,
                chunk_min_z,
                biome_getter,
                mask,
                params,
                sub_seed_a,
                x,
                y,
                z,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                sub_thickness_a,
                sub_rotation_a,
                sub_vert_a,
                current_step,
                dist,
                1.0,
                skip_checker,
            );
            create_tunnel(
                ctx,
                noises,
                chunk,
                chunk_min_x,
                chunk_min_z,
                biome_getter,
                mask,
                params,
                sub_seed_b,
                x,
                y,
                z,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                sub_thickness_b,
                sub_rotation_b,
                sub_vert_b,
                current_step,
                dist,
                1.0,
                skip_checker,
            );
            return;
        }

        if random.next_i32_bounded(4) == 0 {
            continue;
        }

        if !can_reach(
            chunk_min_x,
            chunk_min_z,
            x,
            z,
            current_step,
            dist,
            thickness,
        ) {
            return;
        }

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
            horizontal_radius * horizontal_radius_multiplier,
            vertical_radius * vertical_radius_multiplier,
            skip_checker,
        );
    }
}
