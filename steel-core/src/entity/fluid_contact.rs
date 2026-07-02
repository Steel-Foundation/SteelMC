//! Entity contact with world fluids.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_utils::{BlockPos, ChunkPos, SectionPos, WorldAabb, axis::Axis};

use crate::fluid::{get_flow, get_fluid_state, get_height};
use crate::world::World;

const FLUID_INTERACTION_MARGIN: f64 = 0.001;
const MIN_CURRENT_LENGTH_SQUARED: f64 = 1.0e-5;
const STILL_CURRENT_VELOCITY_THRESHOLD: f64 = 0.003;
const MIN_STILL_CURRENT_IMPULSE: f64 = 0.004_500_000_000_000_000_5;
const SHALLOW_CURRENT_HEIGHT: f64 = 0.4;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct EntityFluidCurrent {
    accumulated: DVec3,
    count: u32,
}

impl EntityFluidCurrent {
    fn accumulate(&mut self, flow: DVec3) {
        self.accumulated += flow;
        self.count += 1;
    }

    fn impulse(self, is_player: bool, old_velocity: DVec3, scale: f64) -> DVec3 {
        if self.count == 0 || self.accumulated.length_squared() < MIN_CURRENT_LENGTH_SQUARED {
            return DVec3::ZERO;
        }

        let mut impulse = if is_player {
            self.accumulated / f64::from(self.count)
        } else {
            self.accumulated.normalize_or_zero()
        };
        impulse *= scale;

        if old_velocity.x.abs() < STILL_CURRENT_VELOCITY_THRESHOLD
            && old_velocity.z.abs() < STILL_CURRENT_VELOCITY_THRESHOLD
            && impulse.length() < MIN_STILL_CURRENT_IMPULSE
        {
            impulse = impulse.normalize_or_zero() * MIN_STILL_CURRENT_IMPULSE;
        }

        impulse
    }
}

#[derive(Debug, Clone, Copy)]
struct FluidScanBounds {
    interaction_box: WorldAabb,
    entity_y: f64,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
}

/// Fluid heights intersecting an entity's current bounding box.
///
/// Mirrors the body-height and eye-fluid tracking part of vanilla's
/// `EntityFluidInteraction`. Current pushing should build on this scan rather
/// than storing separate water/lava flags on individual entity types.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntityFluidContact {
    water_height: f64,
    lava_height: f64,
    eye_in_water: bool,
    eye_in_lava: bool,
    water_current: EntityFluidCurrent,
    lava_current: EntityFluidCurrent,
}

impl EntityFluidContact {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_parts(
        water_height: f64,
        lava_height: f64,
        eye_in_water: bool,
        eye_in_lava: bool,
    ) -> Self {
        Self {
            water_height,
            lava_height,
            eye_in_water,
            eye_in_lava,
            water_current: EntityFluidCurrent::default(),
            lava_current: EntityFluidCurrent::default(),
        }
    }

    /// Scans the world for water/lava touching `bounding_box`.
    #[must_use]
    pub fn scan(world: &Arc<World>, position: DVec3, eye_y: f64, bounding_box: WorldAabb) -> Self {
        let Some(bounds) = Self::scan_bounds(bounding_box) else {
            return Self::default();
        };
        if !has_fluid_and_loaded(world, bounds) {
            return Self::default();
        }

        Self::scan_with_bounds(
            bounds,
            position,
            eye_y,
            false,
            |pos| get_fluid_state(world, pos),
            |pos, fluid_state| get_height(world, pos, fluid_state),
            |_pos, _fluid_state| DVec3::ZERO,
        )
    }

    /// Scans fluid contact and optionally accumulates vanilla fluid currents.
    #[must_use]
    pub fn scan_with_currents(
        world: &Arc<World>,
        position: DVec3,
        eye_y: f64,
        bounding_box: WorldAabb,
        include_current: bool,
    ) -> Self {
        let Some(bounds) = Self::scan_bounds(bounding_box) else {
            return Self::default();
        };
        if !has_fluid_and_loaded(world, bounds) {
            return Self::default();
        }

        Self::scan_with_bounds(
            bounds,
            position,
            eye_y,
            include_current,
            |pos| get_fluid_state(world, pos),
            |pos, fluid_state| get_height(world, pos, fluid_state),
            |pos, fluid_state| get_flow(world, pos, fluid_state),
        )
    }

    /// Returns the highest water surface above the entity's feet.
    #[must_use]
    pub const fn water_height(self) -> f64 {
        self.water_height
    }

    /// Returns the highest lava surface above the entity's feet.
    #[must_use]
    pub const fn lava_height(self) -> f64 {
        self.lava_height
    }

    /// Returns whether the entity's eyes are currently inside water.
    #[must_use]
    pub const fn eye_in_water(self) -> bool {
        self.eye_in_water
    }

    /// Returns whether the entity's eyes are currently inside lava.
    #[must_use]
    pub const fn eye_in_lava(self) -> bool {
        self.eye_in_lava
    }

    /// Returns vanilla water-current impulse for this scan.
    #[must_use]
    pub fn water_current_impulse(self, is_player: bool, old_velocity: DVec3, scale: f64) -> DVec3 {
        self.water_current.impulse(is_player, old_velocity, scale)
    }

    /// Returns vanilla lava-current impulse for this scan.
    #[must_use]
    pub fn lava_current_impulse(self, is_player: bool, old_velocity: DVec3, scale: f64) -> DVec3 {
        self.lava_current.impulse(is_player, old_velocity, scale)
    }

    #[cfg(test)]
    fn scan_with(
        bounding_box: WorldAabb,
        position: DVec3,
        eye_y: f64,
        fluid_at: impl FnMut(BlockPos) -> FluidState,
        height_at: impl FnMut(BlockPos, FluidState) -> f32,
    ) -> Self {
        Self::scan_with_flow(
            bounding_box,
            position,
            eye_y,
            false,
            fluid_at,
            height_at,
            |_pos, _fluid_state| DVec3::ZERO,
        )
    }

    #[cfg(test)]
    fn scan_with_flow(
        bounding_box: WorldAabb,
        position: DVec3,
        eye_y: f64,
        include_current: bool,
        fluid_at: impl FnMut(BlockPos) -> FluidState,
        height_at: impl FnMut(BlockPos, FluidState) -> f32,
        flow_at: impl FnMut(BlockPos, FluidState) -> DVec3,
    ) -> Self {
        let Some(bounds) = Self::scan_bounds(bounding_box) else {
            return Self::default();
        };

        Self::scan_with_bounds(
            bounds,
            position,
            eye_y,
            include_current,
            fluid_at,
            height_at,
            flow_at,
        )
    }

    fn scan_bounds(bounding_box: WorldAabb) -> Option<FluidScanBounds> {
        let interaction_box = bounding_box.deflate(FLUID_INTERACTION_MARGIN);
        if interaction_box.is_empty() {
            return None;
        }

        let x0 = interaction_box.min(Axis::X).floor() as i32;
        let y0 = interaction_box.min(Axis::Y).floor() as i32;
        let z0 = interaction_box.min(Axis::Z).floor() as i32;
        let x1 = interaction_box.max(Axis::X).ceil() as i32 - 1;
        let y1 = interaction_box.max(Axis::Y).ceil() as i32 - 1;
        let z1 = interaction_box.max(Axis::Z).ceil() as i32 - 1;
        if x0 > x1 || y0 > y1 || z0 > z1 {
            return None;
        }

        Some(FluidScanBounds {
            interaction_box,
            entity_y: bounding_box.min(Axis::Y),
            x0,
            y0,
            z0,
            x1,
            y1,
            z1,
        })
    }

    fn scan_with_bounds(
        bounds: FluidScanBounds,
        position: DVec3,
        eye_y: f64,
        include_current: bool,
        mut fluid_at: impl FnMut(BlockPos) -> FluidState,
        mut height_at: impl FnMut(BlockPos, FluidState) -> f32,
        mut flow_at: impl FnMut(BlockPos, FluidState) -> DVec3,
    ) -> Self {
        let mut contact = Self::default();
        let eye_block_x = position.x.floor() as i32;
        let eye_block_z = position.z.floor() as i32;

        for x in bounds.x0..=bounds.x1 {
            for y in bounds.y0..=bounds.y1 {
                for z in bounds.z0..=bounds.z1 {
                    let pos = BlockPos::new(x, y, z);
                    let fluid_state = fluid_at(pos);
                    if fluid_state.is_empty() {
                        continue;
                    }

                    let fluid_bottom = f64::from(y);
                    let fluid_top = fluid_bottom + f64::from(height_at(pos, fluid_state));
                    if fluid_top < bounds.interaction_box.min(Axis::Y) {
                        continue;
                    }

                    let eye_inside = x == eye_block_x
                        && z == eye_block_z
                        && eye_y >= fluid_bottom
                        && eye_y <= fluid_top;
                    let height = fluid_top - bounds.entity_y;
                    if fluid_state.is_water() {
                        contact.water_height = contact.water_height.max(height);
                        contact.eye_in_water |= eye_inside;
                        if include_current {
                            let mut flow = flow_at(pos, fluid_state);
                            if contact.water_height < SHALLOW_CURRENT_HEIGHT {
                                flow *= contact.water_height;
                            }
                            contact.water_current.accumulate(flow);
                        }
                    } else if fluid_state.is_lava() {
                        contact.lava_height = contact.lava_height.max(height);
                        contact.eye_in_lava |= eye_inside;
                        if include_current {
                            let mut flow = flow_at(pos, fluid_state);
                            if contact.lava_height < SHALLOW_CURRENT_HEIGHT {
                                flow *= contact.lava_height;
                            }
                            contact.lava_current.accumulate(flow);
                        }
                    }
                }
            }
        }

        contact
    }
}

#[expect(
    clippy::similar_names,
    reason = "axis-paired bounds mirror vanilla hasFluidAndLoaded"
)]
fn has_fluid_and_loaded(world: &World, bounds: FluidScanBounds) -> bool {
    let section_x0 = SectionPos::block_to_section_coord(bounds.x0 - 1);
    let section_y0 = SectionPos::block_to_section_coord(bounds.y0);
    let section_z0 = SectionPos::block_to_section_coord(bounds.z0 - 1);
    let section_x1 = SectionPos::block_to_section_coord(bounds.x1 + 1);
    let section_y1 = SectionPos::block_to_section_coord(bounds.y1);
    let section_z1 = SectionPos::block_to_section_coord(bounds.z1 + 1);

    let mut has_fluid = false;
    for chunk_z in section_z0..=section_z1 {
        for chunk_x in section_x0..=section_x1 {
            let Some(chunk_has_fluid) =
                world
                    .chunk_map
                    .with_full_chunk(ChunkPos::new(chunk_x, chunk_z), |chunk| {
                        let min_section_y = SectionPos::block_to_section_coord(chunk.min_y());
                        let sections = &chunk.sections().sections;
                        let mut chunk_has_fluid = false;

                        for section_y in section_y0..=section_y1 {
                            let section_index = section_y - min_section_y;
                            let Ok(section_index) = usize::try_from(section_index) else {
                                continue;
                            };
                            let Some(section) = sections.get(section_index) else {
                                continue;
                            };

                            chunk_has_fluid |= section.read().has_fluid();
                        }

                        chunk_has_fluid
                    })
            else {
                return false;
            };

            has_fluid |= chunk_has_fluid;
        }
    }

    has_fluid
}

#[cfg(test)]
mod tests {
    use steel_registry::fluid::FluidState;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_fluids;

    use super::*;

    #[test]
    fn scan_reports_fluid_height_above_entity_feet() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.0, 0.1, 0.9, 10.5, 0.9);

        let contact = EntityFluidContact::scan_with(
            bounding_box,
            DVec3::new(0.5, 10.0, 0.5),
            12.0,
            |pos| {
                if pos.y() == 10 {
                    FluidState::source(&vanilla_fluids::WATER)
                } else {
                    FluidState::EMPTY
                }
            },
            |_pos, _fluid_state| 1.0,
        );

        assert!((contact.water_height() - 1.0).abs() < f64::EPSILON);
        assert!(contact.lava_height().abs() < f64::EPSILON);
        assert!(!contact.eye_in_water());
        assert!(!contact.eye_in_lava());
    }

    #[test]
    fn scan_uses_effective_fluid_height() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.0, 0.1, 0.9, 10.5, 0.9);

        let contact = EntityFluidContact::scan_with(
            bounding_box,
            DVec3::new(0.5, 10.0, 0.5),
            12.0,
            |pos| {
                if pos.y() == 10 {
                    FluidState::flowing(&vanilla_fluids::FLOWING_LAVA, 4, false)
                } else {
                    FluidState::EMPTY
                }
            },
            |_pos, _fluid_state| 4.0 / 9.0,
        );

        assert!(contact.water_height().abs() < f64::EPSILON);
        assert!((contact.lava_height() - 4.0 / 9.0).abs() < 1.0e-7);
        assert!(!contact.eye_in_water());
        assert!(!contact.eye_in_lava());
    }

    #[test]
    fn scan_ignores_fluid_below_interaction_box() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.2, 0.1, 0.9, 10.6, 0.9);

        let contact = EntityFluidContact::scan_with(
            bounding_box,
            DVec3::new(0.5, 10.0, 0.5),
            10.3,
            |pos| {
                if pos.y() == 10 {
                    FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 1, false)
                } else {
                    FluidState::EMPTY
                }
            },
            |_pos, _fluid_state| 1.0 / 9.0,
        );

        assert_eq!(contact, EntityFluidContact::default());
    }

    #[test]
    fn scan_marks_eye_inside_matching_fluid_column() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.0, 0.1, 0.9, 11.0, 0.9);

        let contact = EntityFluidContact::scan_with(
            bounding_box,
            DVec3::new(0.5, 10.0, 0.5),
            10.8,
            |pos| {
                if pos.y() == 10 {
                    FluidState::source(&vanilla_fluids::WATER)
                } else {
                    FluidState::EMPTY
                }
            },
            |_pos, _fluid_state| 1.0,
        );

        assert!(contact.eye_in_water());
        assert!(!contact.eye_in_lava());
    }

    #[test]
    fn scan_accumulates_player_fluid_current_as_average_flow() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.0, 0.1, 1.9, 10.5, 0.9);

        let contact = EntityFluidContact::scan_with_flow(
            bounding_box,
            DVec3::new(0.5, 10.0, 0.5),
            12.0,
            true,
            |_pos| FluidState::source(&vanilla_fluids::WATER),
            |_pos, _fluid_state| 1.0,
            |pos, _fluid_state| {
                if pos.x() == 0 { DVec3::X } else { DVec3::Z }
            },
        );

        assert_eq!(
            contact.water_current_impulse(true, DVec3::ZERO, 1.0),
            DVec3::new(0.5, 0.0, 0.5)
        );
    }

    #[test]
    fn scan_accumulates_non_player_fluid_current_as_normalized_flow() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.0, 0.1, 1.9, 10.5, 0.9);

        let contact = EntityFluidContact::scan_with_flow(
            bounding_box,
            DVec3::new(0.5, 10.0, 0.5),
            12.0,
            true,
            |_pos| FluidState::source(&vanilla_fluids::WATER),
            |_pos, _fluid_state| 1.0,
            |pos, _fluid_state| {
                if pos.x() == 0 { DVec3::X } else { DVec3::Z }
            },
        );

        let expected = DVec3::new(1.0, 0.0, 1.0).normalize();
        let impulse = contact.water_current_impulse(false, DVec3::ZERO, 1.0);
        assert!((impulse - expected).length() < f64::EPSILON);
    }

    #[test]
    fn shallow_current_is_scaled_by_fluid_height() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.0, 0.1, 0.9, 10.5, 0.9);

        let contact = EntityFluidContact::scan_with_flow(
            bounding_box,
            DVec3::new(0.5, 10.0, 0.5),
            12.0,
            true,
            |_pos| FluidState::source(&vanilla_fluids::WATER),
            |_pos, _fluid_state| 0.2,
            |_pos, _fluid_state| DVec3::X,
        );

        let impulse = contact.water_current_impulse(true, DVec3::new(0.01, 0.0, 0.0), 1.0);
        assert!((impulse - DVec3::new(0.2, 0.0, 0.0)).length() < 1.0e-7);
    }
}
