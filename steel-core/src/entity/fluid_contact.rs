//! Entity contact with world fluids.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_utils::{BlockPos, WorldAabb};

use crate::fluid::{get_fluid_state, get_height};
use crate::world::World;

const FLUID_INTERACTION_MARGIN: f64 = 0.001;

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
}

impl EntityFluidContact {
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn from_parts(
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
        }
    }

    /// Scans the world for water/lava touching `bounding_box`.
    #[must_use]
    pub fn scan(world: &Arc<World>, position: DVec3, eye_y: f64, bounding_box: WorldAabb) -> Self {
        Self::scan_with(
            bounding_box,
            position,
            eye_y,
            |pos| get_fluid_state(world, pos),
            |pos, fluid_state| get_height(world, pos, fluid_state),
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

    fn scan_with(
        bounding_box: WorldAabb,
        position: DVec3,
        eye_y: f64,
        mut fluid_at: impl FnMut(BlockPos) -> FluidState,
        mut height_at: impl FnMut(BlockPos, FluidState) -> f32,
    ) -> Self {
        let interaction_box = bounding_box.deflate(FLUID_INTERACTION_MARGIN);
        if interaction_box.is_empty() {
            return Self::default();
        }

        let x0 = interaction_box.min_x().floor() as i32;
        let y0 = interaction_box.min_y().floor() as i32;
        let z0 = interaction_box.min_z().floor() as i32;
        let x1 = interaction_box.max_x().ceil() as i32 - 1;
        let y1 = interaction_box.max_y().ceil() as i32 - 1;
        let z1 = interaction_box.max_z().ceil() as i32 - 1;
        if x0 > x1 || y0 > y1 || z0 > z1 {
            return Self::default();
        }

        let mut contact = Self::default();
        let entity_y = bounding_box.min_y();
        let eye_block_x = position.x.floor() as i32;
        let eye_block_z = position.z.floor() as i32;

        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let pos = BlockPos::new(x, y, z);
                    let fluid_state = fluid_at(pos);
                    if fluid_state.is_empty() {
                        continue;
                    }

                    let fluid_bottom = f64::from(y);
                    let fluid_top = fluid_bottom + f64::from(height_at(pos, fluid_state));
                    if fluid_top < interaction_box.min_y() {
                        continue;
                    }

                    let eye_inside = x == eye_block_x
                        && z == eye_block_z
                        && eye_y >= fluid_bottom
                        && eye_y <= fluid_top;
                    let height = fluid_top - entity_y;
                    if fluid_state.is_water() {
                        contact.water_height = contact.water_height.max(height);
                        contact.eye_in_water |= eye_inside;
                    } else if fluid_state.is_lava() {
                        contact.lava_height = contact.lava_height.max(height);
                        contact.eye_in_lava |= eye_inside;
                    }
                }
            }
        }

        contact
    }
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
                    FluidState::flowing(&vanilla_fluids::LAVA, 4, false)
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
                    FluidState::flowing(&vanilla_fluids::WATER, 1, false)
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
}
