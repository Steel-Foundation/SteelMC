//! Vanilla affected-block ray traversal and immutable-calculator fast paths.

use std::sync::LazyLock;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::random::Random;
use steel_utils::{BlockPos, BlockStateId};

use crate::world::explosion::{
    ExplosionBlockReader, ExplosionDamageCalculator as _, ImmutableExplosionBlockCalculator,
};
use crate::world::{BlockRegionBounds, BlockRegionRead, World};

use super::ServerExplosion;

mod cache;
mod java_block_pos_set;

use java_block_pos_set::JavaBlockPosSet;

#[cfg(test)]
use cache::{
    DenseExplosionBlockCache, ExplosionBlockCache, ImmutableRayCachePolicy, bounded_floor_to_i32,
    visit_immutable_ray_positions_cached,
};

const RAY_GRID_SIZE: i32 = 16;
const RAY_GRID_LAST_INDEX: i32 = RAY_GRID_SIZE - 1;
const RAY_GRID_INTERIOR_SIZE: i32 = RAY_GRID_SIZE - 2;
pub(super) const RAY_COUNT: usize = (RAY_GRID_SIZE * RAY_GRID_SIZE * RAY_GRID_SIZE
    - RAY_GRID_INTERIOR_SIZE * RAY_GRID_INTERIOR_SIZE * RAY_GRID_INTERIOR_SIZE)
    as usize;
const RAY_STEP: f64 = 0.3_f32 as f64;
const RAY_POWER_DECAY: f32 = 0.225_000_01;
const INITIAL_RAY_POWER_BASE: f32 = 0.7;
const INITIAL_RAY_POWER_RANDOM_SCALE: f32 = 0.6;
const MAX_INITIAL_RAY_POWER_SCALE: f32 = 1.3;
const RESISTANCE_POWER_OFFSET: f32 = 0.3;
const RESISTANCE_POWER_SCALE: f32 = 0.3;
const RAY_REGION_BLOCK_PADDING: f64 = 1.0;

#[derive(Clone, Copy)]
struct ExplosionRay {
    step: DVec3,
    initial_power: f32,
}

struct RegionExplosionBlockReader<'reader, 'world> {
    region: &'reader BlockRegionRead<'world>,
}

impl<'reader, 'world> RegionExplosionBlockReader<'reader, 'world> {
    const fn new(region: &'reader BlockRegionRead<'world>) -> Self {
        Self { region }
    }
}

impl ExplosionBlockReader for RegionExplosionBlockReader<'_, '_> {
    #[inline]
    fn block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        self.region.get_block_state(pos)
    }
}

static RAY_STEPS: LazyLock<[DVec3; RAY_COUNT]> = LazyLock::new(|| {
    let mut steps = [DVec3::ZERO; RAY_COUNT];
    let mut index = 0;
    // Keep Vanilla's X/Y/Z traversal order: random ray powers are consumed in this order.
    for xx in 0..RAY_GRID_SIZE {
        for yy in 0..RAY_GRID_SIZE {
            for zz in 0..RAY_GRID_SIZE {
                if is_boundary_ray(xx, yy, zz) {
                    steps[index] = ray_direction(xx, yy, zz) * RAY_STEP;
                    index += 1;
                }
            }
        }
    }
    debug_assert_eq!(index, RAY_COUNT);
    steps
});

#[derive(Clone, Copy)]
struct ExplosionRayContext {
    center: DVec3,
    bounds: ExplosionWorldBounds,
}

impl ExplosionRayContext {
    /// Proves the cached traversal's current and first out-of-region samples fit in `i32`.
    /// Every component is bounded by [`RAY_STEP`]. One-cell headroom on each face therefore covers
    /// the sample that makes the bounded reader request the generic fallback.
    fn can_use_bounded_floor(self, region_bounds: BlockRegionBounds) -> bool {
        let (min, max) = region_bounds.corners();
        self.center.is_finite()
            && ray_axis_has_bounded_floor(self.center.x, min.x(), max.x())
            && ray_axis_has_bounded_floor(self.center.y, min.y(), max.y())
            && ray_axis_has_bounded_floor(self.center.z, min.z(), max.z())
    }
}

fn ray_axis_has_bounded_floor(center: f64, min: i32, max: i32) -> bool {
    min > i32::MIN && max < i32::MAX && center >= f64::from(min) && center < f64::from(max) + 1.0
}

#[derive(Clone, Copy)]
struct ExplosionWorldBounds {
    min_y: i32,
    max_y: i32,
}

impl ExplosionWorldBounds {
    const fn from_world(world: &World) -> Self {
        Self {
            min_y: world.get_min_y(),
            max_y: world.get_max_y(),
        }
    }

    const fn contains(self, pos: BlockPos) -> bool {
        pos.y() >= self.min_y && pos.y() <= self.max_y && World::is_in_world_bounds_horizontal(pos)
    }
}

impl ServerExplosion<'_> {
    pub(super) fn calculate_exploded_positions_from_level_random(&self) -> Vec<BlockPos> {
        let Some(calculator) = self.immutable_calculator_for_rays() else {
            return self.calculate_exploded_positions_sequential(|| {
                self.world.with_random(Random::next_f32)
            });
        };

        let powers = self
            .world
            .with_random(|random| self.draw_immutable_ray_powers(|| random.next_f32()));
        self.calculate_immutable_ray_powers(&powers, calculator)
    }

    #[cfg(test)]
    pub(super) fn calculate_exploded_positions(
        &self,
        mut next_float: impl FnMut() -> f32,
    ) -> Vec<BlockPos> {
        let Some(calculator) = self.immutable_calculator_for_rays() else {
            return self.calculate_exploded_positions_sequential(next_float);
        };

        let powers = self.draw_immutable_ray_powers(&mut next_float);
        self.calculate_immutable_ray_powers(&powers, calculator)
    }

    fn immutable_calculator_for_rays(&self) -> Option<&dyn ImmutableExplosionBlockCalculator> {
        if !self.radius.is_finite() || self.radius < 0.0 {
            return None;
        }
        self.immutable_block_calculator
    }

    fn calculate_immutable_ray_powers(
        &self,
        powers: &[f32; RAY_COUNT],
        calculator: &dyn ImmutableExplosionBlockCalculator,
    ) -> Vec<BlockPos> {
        if let Some(read_radius) = calculator.bounded_block_read_radius()
            && let Some(bounds) = self.immutable_ray_region_bounds(read_radius)
            && let Some(Some(affected)) = self.world.try_with_block_region(bounds, |region| {
                if !region.has_complete_data() {
                    return None;
                }
                let reader = RegionExplosionBlockReader::new(region);
                self.calculate_immutable_ray_powers_with_reader(powers, calculator, &reader, bounds)
            })
        {
            return affected;
        }

        self.calculate_immutable_ray_powers_uncached_with_reader(
            powers,
            calculator,
            self.world.as_ref(),
        )
    }

    fn calculate_immutable_ray_powers_uncached_with_reader<R: ExplosionBlockReader>(
        &self,
        powers: &[f32; RAY_COUNT],
        calculator: &dyn ImmutableExplosionBlockCalculator,
        reader: &R,
    ) -> Vec<BlockPos> {
        let context = ExplosionRayContext {
            center: self.center,
            bounds: ExplosionWorldBounds::from_world(self.world),
        };
        let mut affected = JavaBlockPosSet::default();
        for (&step, &initial_power) in RAY_STEPS.iter().zip(powers) {
            visit_immutable_ray_positions(
                ExplosionRay {
                    step,
                    initial_power,
                },
                context,
                reader,
                calculator,
                |pos| {
                    affected.insert(pos);
                },
            );
        }
        affected.into_iter().collect()
    }

    fn immutable_ray_region_bounds(&self, read_radius: u32) -> Option<BlockRegionBounds> {
        if !self.center.is_finite() {
            return None;
        }
        let maximum_ray_distance = f64::from(self.radius) * f64::from(MAX_INITIAL_RAY_POWER_SCALE)
            / f64::from(RAY_POWER_DECAY)
            * RAY_STEP;
        let extent = maximum_ray_distance + f64::from(read_radius) + RAY_REGION_BLOCK_PADDING;
        if !extent.is_finite() {
            return None;
        }
        let extent = DVec3::splat(extent);
        Some(BlockRegionBounds::from_corners(
            BlockPos::from(self.center - extent),
            BlockPos::from(self.center + extent),
        ))
    }

    fn calculate_exploded_positions_sequential(
        &self,
        mut next_float: impl FnMut() -> f32,
    ) -> Vec<BlockPos> {
        let mut affected = JavaBlockPosSet::default();
        let bounds = ExplosionWorldBounds::from_world(self.world);

        for &step in RAY_STEPS.iter() {
            let mut remaining_power = initial_ray_power(self.radius, next_float());
            let mut ray_pos = self.center;
            while remaining_power > 0.0 {
                let pos = BlockPos::from(ray_pos);
                let state = self.world.get_block_state(pos);
                let fluid = state.get_fluid_state();
                if !bounds.contains(pos) {
                    break;
                }

                if let Some(resistance) = self
                    .damage_calculator
                    .block_explosion_resistance(self, self.world, pos, state, fluid)
                {
                    remaining_power -= ray_power_loss_from_resistance(resistance);
                }

                if remaining_power > 0.0
                    && self.damage_calculator.should_block_explode(
                        self,
                        self.world,
                        pos,
                        state,
                        remaining_power,
                    )
                {
                    affected.insert(pos);
                }

                ray_pos += step;
                remaining_power -= RAY_POWER_DECAY;
            }
        }

        affected.into_iter().collect()
    }

    fn draw_immutable_ray_powers(&self, mut next_float: impl FnMut() -> f32) -> [f32; RAY_COUNT] {
        let mut powers = [0.0; RAY_COUNT];
        for power in &mut powers {
            *power = initial_ray_power(self.radius, next_float());
        }
        powers
    }

    #[cfg(test)]
    fn draw_immutable_rays(&self, next_float: impl FnMut() -> f32) -> Vec<ExplosionRay> {
        let powers = self.draw_immutable_ray_powers(next_float);
        RAY_STEPS
            .iter()
            .copied()
            .zip(powers)
            .map(|(step, initial_power)| ExplosionRay {
                step,
                initial_power,
            })
            .collect()
    }
}

fn ray_direction(xx: i32, yy: i32, zz: i32) -> DVec3 {
    let mut xd = ray_direction_component(xx);
    let mut yd = ray_direction_component(yy);
    let mut zd = ray_direction_component(zz);
    let direction_length = (xd * xd + yd * yd + zd * zd).sqrt();
    xd /= direction_length;
    yd /= direction_length;
    zd /= direction_length;
    DVec3::new(xd, yd, zd)
}

fn ray_direction_component(index: i32) -> f64 {
    f64::from(index as f32 / RAY_GRID_LAST_INDEX as f32 * 2.0 - 1.0)
}

fn initial_ray_power(radius: f32, random: f32) -> f32 {
    radius * (INITIAL_RAY_POWER_BASE + random * INITIAL_RAY_POWER_RANDOM_SCALE)
}

fn ray_power_loss_from_resistance(resistance: f32) -> f32 {
    (resistance + RESISTANCE_POWER_OFFSET) * RESISTANCE_POWER_SCALE
}

const fn is_boundary_ray(xx: i32, yy: i32, zz: i32) -> bool {
    xx == 0
        || xx == RAY_GRID_LAST_INDEX
        || yy == 0
        || yy == RAY_GRID_LAST_INDEX
        || zz == 0
        || zz == RAY_GRID_LAST_INDEX
}

fn visit_immutable_ray_positions<R: ExplosionBlockReader>(
    ray: ExplosionRay,
    context: ExplosionRayContext,
    reader: &R,
    calculator: &dyn ImmutableExplosionBlockCalculator,
    mut visit: impl FnMut(BlockPos),
) {
    let mut remaining_power = ray.initial_power;
    let mut ray_pos = context.center;
    while remaining_power > 0.0 {
        let pos = BlockPos::from(ray_pos);
        let Some(state) = reader.block_state(pos) else {
            return;
        };
        let fluid = state.get_fluid_state();
        if !context.bounds.contains(pos) {
            break;
        }

        if let Some(resistance) = calculator.explosion_resistance(reader, pos, state, fluid) {
            remaining_power -= ray_power_loss_from_resistance(resistance);
        }

        if remaining_power > 0.0 && calculator.should_explode(reader, pos, state, remaining_power) {
            visit(pos);
        }

        ray_pos += ray.step;
        remaining_power -= RAY_POWER_DECAY;
    }
}

#[cfg(test)]
mod tests;
