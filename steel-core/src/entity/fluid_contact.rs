//! Entity contact with world fluids.

use std::sync::Arc;

use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_utils::{BlockPos, WorldAabb};

use crate::fluid::{get_fluid_state, get_height};
use crate::world::World;

const FLUID_INTERACTION_MARGIN: f64 = 0.001;

/// Fluid heights intersecting an entity's current bounding box.
///
/// Mirrors the height-tracking part of vanilla's `EntityFluidInteraction`.
/// Current pushing and eye-fluid tracking should build on this scan rather
/// than storing separate water/lava flags on individual entity types.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntityFluidContact {
    water_height: f64,
    lava_height: f64,
}

impl EntityFluidContact {
    /// Scans the world for water/lava touching `bounding_box`.
    #[must_use]
    pub fn scan(world: &Arc<World>, bounding_box: WorldAabb) -> Self {
        Self::scan_with(
            bounding_box,
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

    fn scan_with(
        bounding_box: WorldAabb,
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

                    let height = fluid_top - entity_y;
                    if fluid_state.is_water() {
                        contact.water_height = contact.water_height.max(height);
                    } else if fluid_state.is_lava() {
                        contact.lava_height = contact.lava_height.max(height);
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
    }

    #[test]
    fn scan_uses_effective_fluid_height() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.0, 0.1, 0.9, 10.5, 0.9);

        let contact = EntityFluidContact::scan_with(
            bounding_box,
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
    }

    #[test]
    fn scan_ignores_fluid_below_interaction_box() {
        init_test_registry();
        let bounding_box = WorldAabb::new(0.1, 10.2, 0.1, 0.9, 10.6, 0.9);

        let contact = EntityFluidContact::scan_with(
            bounding_box,
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
}
