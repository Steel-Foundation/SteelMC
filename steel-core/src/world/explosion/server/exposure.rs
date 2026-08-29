//! Vanilla entity exposure sampling and Steel's collision-ray reuse.

use glam::DVec3;
use steel_registry::vanilla_entities;
use steel_utils::{BlockPos, WorldAabb};

use crate::behavior::BlockCollisionContext;
use crate::behavior::blocks::PowderSnowBlock;
use crate::chunk::paletted_container::BlockPalette;
use crate::entity::Entity;
#[cfg(test)]
use crate::world::World as ServerWorld;
use crate::world::raycast::{ExplosionExposureRaycast, collision_path_axis_block_bounds};
use crate::world::{BlockRegionBounds, MAX_BLOCK_REGION_WORKSET_SLOTS};

const EXPOSURE_SAMPLE_DENSITY: f64 = 2.0;
const EXPOSURE_SAMPLE_OFFSET_DIVISOR: f64 = 2.0;
const MAX_EXPOSURE_CERTIFICATE_AXIS_WORK: usize =
    MAX_BLOCK_REGION_WORKSET_SLOTS * BlockPalette::SIZE;

#[derive(Clone, Copy)]
pub(super) struct EntityExplosionExposure {
    bounding_box: WorldAabb,
    pub(super) collision_context: BlockCollisionContext,
    x_step: f64,
    y_step: f64,
    z_step: f64,
    x_offset: f64,
    z_offset: f64,
}

impl EntityExplosionExposure {
    pub(super) fn capture(entity: &dyn Entity) -> Self {
        let bounding_box = entity.bounding_box();
        let x_step = exposure_axis_step(bounding_box.width());
        let y_step = exposure_axis_step(bounding_box.height());
        let z_step = exposure_axis_step(bounding_box.depth());
        let collision_context =
            BlockCollisionContext::entity(entity.position().y, entity.is_descending())
                .with_fall_distance(entity.fall_distance())
                .with_can_walk_on_powder_snow(PowderSnowBlock::can_entity_walk_on_powder_snow(
                    entity,
                ))
                .with_falling_block(entity.entity_type() == &vanilla_entities::FALLING_BLOCK);

        Self {
            bounding_box,
            collision_context,
            x_step,
            y_step,
            z_step,
            x_offset: (1.0 - (1.0 / x_step).floor() * x_step) / EXPOSURE_SAMPLE_OFFSET_DIVISOR,
            z_offset: (1.0 - (1.0 / z_step).floor() * z_step) / EXPOSURE_SAMPLE_OFFSET_DIVISOR,
        }
    }

    const fn has_negative_step(self) -> bool {
        self.x_step < 0.0 || self.y_step < 0.0 || self.z_step < 0.0
    }

    fn sample_position(&self, x_fraction: f64, y_fraction: f64, z_fraction: f64) -> DVec3 {
        DVec3::new(
            self.bounding_box.min_x()
                + (self.bounding_box.max_x() - self.bounding_box.min_x()) * x_fraction
                + self.x_offset,
            self.bounding_box.min_y()
                + (self.bounding_box.max_y() - self.bounding_box.min_y()) * y_fraction,
            self.bounding_box.min_z()
                + (self.bounding_box.max_z() - self.bounding_box.min_z()) * z_fraction
                + self.z_offset,
        )
    }

    fn axis_sample_block_bounds(
        axis_min: f64,
        axis_max: f64,
        step: f64,
        offset: f64,
        center: f64,
    ) -> Option<(i32, i32)> {
        let can_sample_axis = axis_min.is_finite()
            && axis_max.is_finite()
            && axis_max >= axis_min
            && step.is_finite()
            && step > 0.0
            && offset.is_finite()
            && center.is_finite();
        if !can_sample_axis {
            return None;
        }

        let axis_length = axis_max - axis_min;
        let mut min_block = i32::MAX;
        let mut max_block = i32::MIN;
        let mut fraction = 0.0;
        for _ in 0..MAX_EXPOSURE_CERTIFICATE_AXIS_WORK {
            if fraction > 1.0 {
                return Some((min_block, max_block));
            }
            let sample = axis_min + axis_length * fraction + offset;
            let (sample_min, sample_max) = collision_path_axis_block_bounds(
                sample,
                center,
                MAX_EXPOSURE_CERTIFICATE_AXIS_WORK,
            )?;
            min_block = min_block.min(sample_min);
            max_block = max_block.max(sample_max);
            fraction += step;
            if !fraction.is_finite() {
                return None;
            }
        }
        None
    }

    /// Builds a Cartesian envelope containing every block visited by all exposure rays.
    ///
    /// Each axis uses Vanilla's repeated-addition sample sequence and exact scalar DDA recurrence.
    /// This is linear in the three axis sample counts instead of their Cartesian product.
    pub(super) fn stable_air_certificate_bounds(self, center: DVec3) -> Option<BlockRegionBounds> {
        let (min_x, max_x) = Self::axis_sample_block_bounds(
            self.bounding_box.min_x(),
            self.bounding_box.max_x(),
            self.x_step,
            self.x_offset,
            center.x,
        )?;
        let (min_y, max_y) = Self::axis_sample_block_bounds(
            self.bounding_box.min_y(),
            self.bounding_box.max_y(),
            self.y_step,
            0.0,
            center.y,
        )?;
        let (min_z, max_z) = Self::axis_sample_block_bounds(
            self.bounding_box.min_z(),
            self.bounding_box.max_z(),
            self.z_step,
            self.z_offset,
            center.z,
        )?;
        Some(BlockRegionBounds::from_corners(
            BlockPos::new(min_x, min_y, min_z),
            BlockPos::new(max_x, max_y, max_z),
        ))
    }

    fn for_each_sample(self, mut visit: impl FnMut(DVec3)) -> usize {
        let mut sample_count = 0;
        // Repeated addition and inclusive bounds intentionally mirror Vanilla's floating-point
        // sample sequence; deriving each fraction from an integer index can change boundary rays.
        let mut x_fraction = 0.0;
        while x_fraction <= 1.0 {
            let mut y_fraction = 0.0;
            while y_fraction <= 1.0 {
                let mut z_fraction = 0.0;
                while z_fraction <= 1.0 {
                    visit(self.sample_position(x_fraction, y_fraction, z_fraction));
                    sample_count += 1;
                    z_fraction += self.z_step;
                }
                y_fraction += self.y_step;
            }
            x_fraction += self.x_step;
        }
        sample_count
    }

    #[cfg(test)]
    pub(super) fn sample_positions(self) -> Vec<DVec3> {
        let mut samples = Vec::new();
        self.for_each_sample(|sample| samples.push(sample));
        samples
    }

    #[cfg(test)]
    #[inline]
    fn sample_is_visible(self, world: &ServerWorld, center: DVec3, from: DVec3) -> bool {
        world.is_block_collision_path_clear(from, center, self.collision_context)
    }

    fn exposure(visible_samples: u32, sample_count: usize) -> f32 {
        visible_samples as f32 / sample_count as f32
    }

    #[cfg(test)]
    pub(super) fn calculate_uncached(self, world: &ServerWorld, center: DVec3) -> f32 {
        if self.has_negative_step() {
            return 0.0;
        }

        self.calculate_with_visibility(|from| self.sample_is_visible(world, center, from))
    }

    pub(super) fn calculate_with_visibility(
        self,
        mut is_visible: impl FnMut(DVec3) -> bool,
    ) -> f32 {
        let mut visible_samples = 0;
        let sample_count = self.for_each_sample(|from| {
            if is_visible(from) {
                visible_samples += 1;
            }
        });
        Self::exposure(visible_samples, sample_count)
    }

    #[cfg(test)]
    fn calculate_cached(self, world: &ServerWorld, center: DVec3) -> f32 {
        if self.has_negative_step() {
            return 0.0;
        }

        let mut raycast = ExplosionExposureRaycast::new(world, self.collision_context);
        self.calculate_cached_with(&mut raycast, center)
    }

    pub(super) fn calculate_cached_with(
        self,
        raycast: &mut ExplosionExposureRaycast<'_>,
        center: DVec3,
    ) -> f32 {
        if self.has_negative_step() {
            return 0.0;
        }
        if let Some(bounds) = self.stable_air_certificate_bounds(center)
            && raycast.stable_air_box_is_clear(bounds)
        {
            return 1.0;
        }
        raycast.set_collision_context(self.collision_context);
        self.calculate_with_visibility(|from| raycast.is_path_clear(from, center))
    }
}

fn exposure_axis_step(axis_length: f64) -> f64 {
    1.0 / (axis_length * EXPOSURE_SAMPLE_DENSITY + 1.0)
}

#[cfg(test)]
pub(super) fn seen_percent(world: &ServerWorld, center: DVec3, entity: &dyn Entity) -> f32 {
    let exposure = EntityExplosionExposure::capture(entity);
    if exposure.has_negative_step() {
        return 0.0;
    }
    exposure.calculate_cached(world, center)
}
