//! World border state and collision helpers.

use std::{error::Error, fmt};

use steel_utils::{BlockPos, WorldAabb};

use crate::level_data::WorldBorderData;

const MAX_CENTER_COORDINATE: f64 = 2.999_998_4E7;
const DEFAULT_ABSOLUTE_MAX_SIZE: i32 = 29_999_984;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderStatus {
    Growing,
    Shrinking,
    Stationary,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WorldBorderSnapshot {
    pub(crate) center_x: f64,
    pub(crate) center_z: f64,
    pub(crate) old_size: f64,
    pub(crate) new_size: f64,
    pub(crate) lerp_time: i64,
    pub(crate) absolute_max_size: i32,
    pub(crate) warning_blocks: i32,
    pub(crate) warning_time: i32,
    pub(crate) damage_per_block: f64,
    pub(crate) safe_zone: f64,
    pub(crate) min_x: f64,
    pub(crate) min_z: f64,
    pub(crate) max_x: f64,
    pub(crate) max_z: f64,
    pub(crate) status: BorderStatus,
}

impl WorldBorderSnapshot {
    #[must_use]
    pub(crate) fn is_block_within_bounds(self, pos: BlockPos) -> bool {
        self.is_within_bounds_with_margin(f64::from(pos.x()), f64::from(pos.z()), 0.0)
    }

    #[must_use]
    pub(crate) fn is_within_bounds_with_margin(self, x: f64, z: f64, margin: f64) -> bool {
        x >= self.min_x - margin
            && x < self.max_x + margin
            && z >= self.min_z - margin
            && z < self.max_z + margin
    }

    #[must_use]
    pub(crate) fn clamp_to_bounds(self, x: f64, y: f64, z: f64) -> BlockPos {
        let epsilon = f64::from(1.0E-5_f32);
        BlockPos::containing(
            clamp_f64(x, self.min_x, self.max_x - epsilon),
            y,
            clamp_f64(z, self.min_z, self.max_z - epsilon),
        )
    }

    #[must_use]
    pub(crate) fn is_within_bounds(self, bounding_box: WorldAabb) -> bool {
        let epsilon = f64::from(1.0E-5_f32);
        self.is_within_bounds_with_margin(bounding_box.min_x(), bounding_box.min_z(), 0.0)
            && self.is_within_bounds_with_margin(
                bounding_box.max_x() - epsilon,
                bounding_box.max_z() - epsilon,
                0.0,
            )
    }

    #[must_use]
    pub(crate) fn distance_to_border(self, x: f64, z: f64) -> f64 {
        let from_north = z - self.min_z;
        let from_south = self.max_z - z;
        let from_west = x - self.min_x;
        let from_east = self.max_x - x;
        from_west.min(from_east).min(from_north).min(from_south)
    }

    #[must_use]
    pub(crate) fn is_inside_close_to_border(self, x: f64, z: f64, bounding_box: WorldAabb) -> bool {
        let box_max = bounding_box
            .width()
            .abs()
            .max(bounding_box.depth().abs())
            .max(1.0);
        self.distance_to_border(x, z) < box_max * 2.0
            && self.is_within_bounds_with_margin(x, z, box_max)
    }

    #[must_use]
    pub(crate) fn outside_damage_amount(
        self,
        x: f64,
        z: f64,
        bounding_box: WorldAabb,
    ) -> Option<f32> {
        if self.is_within_bounds(bounding_box) {
            return None;
        }

        let distance = self.distance_to_border(x, z) + self.safe_zone;
        if distance >= 0.0 || self.damage_per_block <= 0.0 {
            return None;
        }

        Some((-distance * self.damage_per_block).floor().max(1.0) as f32)
    }

    #[must_use]
    pub(crate) fn collision_shapes_for(self, aabb: WorldAabb) -> Vec<WorldAabb> {
        let min_x = self.min_x.floor();
        let min_z = self.min_z.floor();
        let max_x = self.max_x.ceil();
        let max_z = self.max_z.ceil();
        let shapes = [
            WorldAabb::new(
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                min_x,
                f64::INFINITY,
                f64::INFINITY,
            ),
            WorldAabb::new(
                max_x,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::INFINITY,
                f64::INFINITY,
            ),
            WorldAabb::new(
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::INFINITY,
                min_z,
            ),
            WorldAabb::new(
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                max_z,
                f64::INFINITY,
                f64::INFINITY,
                f64::INFINITY,
            ),
        ];

        shapes
            .into_iter()
            .filter(|shape| shape.intersects(aabb))
            .collect()
    }
}

/// Invalid world border state or update.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldBorderError {
    /// A floating-point field was NaN or infinite.
    NonFinite(&'static str, f64),
    /// A center coordinate exceeded vanilla's allowed range.
    CenterOutOfRange(&'static str, f64),
    /// A size lerp was requested with a negative duration.
    NegativeLerpTime(i64),
}

impl fmt::Display for WorldBorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NonFinite(field, value) => {
                write!(f, "world border {field} must be finite, got {value}")
            }
            Self::CenterOutOfRange(field, value) => write!(
                f,
                "world border {field} must be within +/-{MAX_CENTER_COORDINATE}, got {value}"
            ),
            Self::NegativeLerpTime(value) => {
                write!(
                    f,
                    "world border lerp time must be non-negative, got {value}"
                )
            }
        }
    }
}

impl Error for WorldBorderError {}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BorderExtent {
    Static {
        size: f64,
    },
    Moving {
        from: f64,
        to: f64,
        lerp_duration: i64,
        lerp_progress: i64,
        size: f64,
        previous_size: f64,
    },
}

impl BorderExtent {
    const fn static_size(size: f64) -> Self {
        Self::Static { size }
    }

    #[expect(
        clippy::float_cmp,
        reason = "Vanilla collapses lerps when start and target sizes are exactly equal."
    )]
    fn moving(from: f64, to: f64, ticks: i64) -> Result<Self, WorldBorderError> {
        if ticks < 0 {
            return Err(WorldBorderError::NegativeLerpTime(ticks));
        }
        if ticks == 0 || from == to {
            return Ok(Self::static_size(to));
        }

        Ok(Self::Moving {
            from,
            to,
            lerp_duration: ticks,
            lerp_progress: ticks,
            size: from,
            previous_size: from,
        })
    }

    const fn size(self) -> f64 {
        match self {
            Self::Static { size } | Self::Moving { size, .. } => size,
        }
    }

    const fn lerp_time(self) -> i64 {
        match self {
            Self::Static { .. } => 0,
            Self::Moving { lerp_progress, .. } => lerp_progress,
        }
    }

    const fn lerp_target(self) -> f64 {
        match self {
            Self::Static { size } => size,
            Self::Moving { to, .. } => to,
        }
    }

    fn status(self) -> BorderStatus {
        match self {
            Self::Static { .. } => BorderStatus::Stationary,
            Self::Moving { from, to, .. } if to < from => BorderStatus::Shrinking,
            Self::Moving { .. } => BorderStatus::Growing,
        }
    }

    fn tick(self) -> Self {
        let Self::Moving {
            from,
            to,
            lerp_duration,
            lerp_progress,
            size,
            ..
        } = self
        else {
            return self;
        };

        let next_progress = lerp_progress - 1;
        if next_progress <= 0 {
            return Self::Static { size: to };
        }

        let progress = (lerp_duration - next_progress) as f64 / lerp_duration as f64;
        Self::Moving {
            from,
            to,
            lerp_duration,
            lerp_progress: next_progress,
            size: from + (to - from) * progress,
            previous_size: size,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorldBorder {
    center_x: f64,
    center_z: f64,
    damage_per_block: f64,
    safe_zone: f64,
    warning_time: i32,
    warning_blocks: i32,
    absolute_max_size: i32,
    extent: BorderExtent,
}

impl WorldBorder {
    pub(crate) fn new(data: WorldBorderData) -> Result<Self, WorldBorderError> {
        validate_center("center_x", data.center_x)?;
        validate_center("center_z", data.center_z)?;
        validate_finite("damage_per_block", data.damage_per_block)?;
        validate_finite("safe_zone", data.safe_zone)?;
        validate_finite("size", data.size)?;
        validate_finite("lerp_target", data.lerp_target)?;

        let extent = if data.lerp_time > 0 {
            BorderExtent::moving(data.size, data.lerp_target, data.lerp_time)?
        } else {
            BorderExtent::static_size(data.size)
        };

        Ok(Self {
            center_x: data.center_x,
            center_z: data.center_z,
            damage_per_block: data.damage_per_block,
            safe_zone: data.safe_zone,
            warning_time: data.warning_time,
            warning_blocks: data.warning_blocks,
            absolute_max_size: DEFAULT_ABSOLUTE_MAX_SIZE,
            extent,
        })
    }

    #[must_use]
    pub(crate) fn snapshot(&self) -> WorldBorderSnapshot {
        let old_size = self.extent.size();
        let new_size = self.extent.lerp_target();
        let half_size = old_size / 2.0;
        let absolute_max = f64::from(self.absolute_max_size);
        WorldBorderSnapshot {
            center_x: self.center_x,
            center_z: self.center_z,
            old_size,
            new_size,
            lerp_time: self.extent.lerp_time(),
            absolute_max_size: self.absolute_max_size,
            warning_blocks: self.warning_blocks,
            warning_time: self.warning_time,
            damage_per_block: self.damage_per_block,
            safe_zone: self.safe_zone,
            min_x: clamp_f64(self.center_x - half_size, -absolute_max, absolute_max),
            min_z: clamp_f64(self.center_z - half_size, -absolute_max, absolute_max),
            max_x: clamp_f64(self.center_x + half_size, -absolute_max, absolute_max),
            max_z: clamp_f64(self.center_z + half_size, -absolute_max, absolute_max),
            status: self.extent.status(),
        }
    }

    #[must_use]
    pub(crate) const fn to_data(&self) -> WorldBorderData {
        WorldBorderData {
            center_x: self.center_x,
            center_z: self.center_z,
            damage_per_block: self.damage_per_block,
            safe_zone: self.safe_zone,
            warning_blocks: self.warning_blocks,
            warning_time: self.warning_time,
            size: self.extent.size(),
            lerp_time: self.extent.lerp_time(),
            lerp_target: self.extent.lerp_target(),
        }
    }

    pub(crate) fn tick(&mut self) {
        self.extent = self.extent.tick();
    }

    pub(crate) fn set_center(&mut self, x: f64, z: f64) -> Result<(), WorldBorderError> {
        validate_center("center_x", x)?;
        validate_center("center_z", z)?;
        self.center_x = x;
        self.center_z = z;
        Ok(())
    }

    pub(crate) fn set_size(&mut self, size: f64) -> Result<(), WorldBorderError> {
        validate_finite("size", size)?;
        self.extent = BorderExtent::static_size(size);
        Ok(())
    }

    pub(crate) fn lerp_size_between(
        &mut self,
        from: f64,
        to: f64,
        ticks: i64,
    ) -> Result<(), WorldBorderError> {
        validate_finite("from", from)?;
        validate_finite("to", to)?;
        self.extent = BorderExtent::moving(from, to, ticks)?;
        Ok(())
    }

    pub(crate) const fn set_warning_time(&mut self, warning_time: i32) {
        self.warning_time = warning_time;
    }

    pub(crate) const fn set_warning_blocks(&mut self, warning_blocks: i32) {
        self.warning_blocks = warning_blocks;
    }

    pub(crate) fn set_damage_per_block(
        &mut self,
        damage_per_block: f64,
    ) -> Result<(), WorldBorderError> {
        validate_finite("damage_per_block", damage_per_block)?;
        self.damage_per_block = damage_per_block;
        Ok(())
    }

    pub(crate) fn set_safe_zone(&mut self, safe_zone: f64) -> Result<(), WorldBorderError> {
        validate_finite("safe_zone", safe_zone)?;
        self.safe_zone = safe_zone;
        Ok(())
    }
}

fn validate_center(field: &'static str, value: f64) -> Result<(), WorldBorderError> {
    validate_finite(field, value)?;
    if !(-MAX_CENTER_COORDINATE..=MAX_CENTER_COORDINATE).contains(&value) {
        return Err(WorldBorderError::CenterOutOfRange(field, value));
    }
    Ok(())
}

const fn validate_finite(field: &'static str, value: f64) -> Result<(), WorldBorderError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(WorldBorderError::NonFinite(field, value))
    }
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_f64_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= f64::EPSILON,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_f32_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "expected {expected}, got {actual}"
        );
    }

    fn default_border() -> WorldBorder {
        WorldBorder::new(WorldBorderData::default()).expect("default world border should load")
    }

    #[test]
    fn default_border_matches_vanilla_initial_settings() {
        let border = default_border();
        let snapshot = border.snapshot();

        assert_f64_eq(snapshot.center_x, 0.0);
        assert_f64_eq(snapshot.center_z, 0.0);
        assert_f64_eq(snapshot.old_size, WorldBorderData::default().size);
        assert_f64_eq(snapshot.new_size, WorldBorderData::default().size);
        assert_eq!(snapshot.lerp_time, 0);
        assert_eq!(snapshot.warning_blocks, 5);
        assert_eq!(snapshot.warning_time, 300);
        assert_eq!(snapshot.absolute_max_size, DEFAULT_ABSOLUTE_MAX_SIZE);
    }

    #[test]
    fn border_bounds_are_clamped_to_absolute_max_size() {
        let mut border = default_border();
        border
            .set_center(MAX_CENTER_COORDINATE, MAX_CENTER_COORDINATE)
            .expect("valid vanilla border center");
        let snapshot = border.snapshot();

        assert_f64_eq(snapshot.max_x, f64::from(DEFAULT_ABSOLUTE_MAX_SIZE));
        assert_f64_eq(snapshot.max_z, f64::from(DEFAULT_ABSOLUTE_MAX_SIZE));
    }

    #[test]
    fn block_position_bounds_use_vanilla_inclusive_min_exclusive_max() {
        let mut border = default_border();
        border.set_size(10.0).expect("valid border size");
        let snapshot = border.snapshot();

        assert!(snapshot.is_block_within_bounds(BlockPos::new(-5, 64, -5)));
        assert!(snapshot.is_block_within_bounds(BlockPos::new(4, 64, 4)));
        assert!(!snapshot.is_block_within_bounds(BlockPos::new(5, 64, 0)));
        assert!(!snapshot.is_block_within_bounds(BlockPos::new(0, 64, 5)));
    }

    #[test]
    fn clamp_to_bounds_matches_vanilla_blockpos_containing() {
        let mut border = default_border();
        border.set_size(10.0).expect("valid border size");
        let snapshot = border.snapshot();

        assert_eq!(
            snapshot.clamp_to_bounds(12.7, 64.9, 5.5),
            BlockPos::new(4, 64, 4)
        );
        assert_eq!(
            snapshot.clamp_to_bounds(-8.1, -1.2, -9.3),
            BlockPos::new(-5, -2, -5)
        );
    }

    #[test]
    fn moving_border_ticks_to_target_size() {
        let mut border = default_border();
        border
            .lerp_size_between(10.0, 20.0, 2)
            .expect("valid border lerp");

        border.tick();
        let first_tick = border.snapshot();
        assert_f64_eq(first_tick.old_size, 15.0);
        assert_f64_eq(first_tick.new_size, 20.0);
        assert_eq!(first_tick.lerp_time, 1);
        assert_eq!(first_tick.status, BorderStatus::Growing);

        border.tick();
        let second_tick = border.snapshot();
        assert_f64_eq(second_tick.old_size, 20.0);
        assert_f64_eq(second_tick.new_size, 20.0);
        assert_eq!(second_tick.lerp_time, 0);
        assert_eq!(second_tick.status, BorderStatus::Stationary);
    }

    #[test]
    fn close_to_border_matches_vanilla_distance_gate() {
        let mut border = default_border();
        border.set_size(10.0).expect("valid border size");
        let snapshot = border.snapshot();
        let aabb = WorldAabb::new(3.8, 0.0, -0.3, 4.4, 1.8, 0.3);

        assert!(snapshot.is_inside_close_to_border(4.1, 0.0, aabb));
        assert!(!snapshot.is_inside_close_to_border(0.0, 0.0, aabb));
    }

    #[test]
    fn collision_shape_represents_outside_of_border() {
        let mut border = default_border();
        border.set_size(10.0).expect("valid border size");
        let snapshot = border.snapshot();
        let crossing_east = WorldAabb::new(4.5, 0.0, -0.3, 5.2, 1.8, 0.3);
        let inside = WorldAabb::new(0.0, 0.0, -0.3, 0.6, 1.8, 0.3);

        assert_eq!(snapshot.collision_shapes_for(crossing_east).len(), 1);
        assert!(snapshot.collision_shapes_for(inside).is_empty());
    }

    #[test]
    fn outside_damage_is_ignored_inside_safe_zone() {
        let mut border = default_border();
        border.set_size(10.0).expect("valid border size");
        let snapshot = border.snapshot();
        let just_outside = WorldAabb::entity_box(5.2, 0.0, 0.0, 0.3, 1.8);

        assert!(
            snapshot
                .outside_damage_amount(5.2, 0.0, just_outside)
                .is_none()
        );
    }

    #[test]
    fn outside_damage_uses_vanilla_minimum_damage() {
        let mut border = default_border();
        border.set_size(10.0).expect("valid border size");
        let snapshot = border.snapshot();
        let outside = WorldAabb::entity_box(12.0, 0.0, 0.0, 0.3, 1.8);

        let damage = snapshot
            .outside_damage_amount(12.0, 0.0, outside)
            .expect("outside player should take border damage");

        assert_f32_eq(damage, 1.0);
    }

    #[test]
    fn outside_damage_uses_vanilla_floor_after_scaling() {
        let border = WorldBorder::new(WorldBorderData {
            size: 10.0,
            safe_zone: 0.0,
            damage_per_block: 2.0,
            ..WorldBorderData::default()
        })
        .expect("valid world border");
        let snapshot = border.snapshot();
        let outside = WorldAabb::entity_box(8.4, 0.0, 0.0, 0.3, 1.8);

        let damage = snapshot
            .outside_damage_amount(8.4, 0.0, outside)
            .expect("outside player should take border damage");

        assert_f32_eq(damage, 6.0);
    }

    #[test]
    fn outside_damage_is_disabled_when_damage_per_block_is_not_positive() {
        let border = WorldBorder::new(WorldBorderData {
            size: 10.0,
            safe_zone: 0.0,
            damage_per_block: 0.0,
            ..WorldBorderData::default()
        })
        .expect("valid world border");
        let snapshot = border.snapshot();
        let outside = WorldAabb::entity_box(8.0, 0.0, 0.0, 0.3, 1.8);

        assert!(snapshot.outside_damage_amount(8.0, 0.0, outside).is_none());
    }
}
