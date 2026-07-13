//! Read-only world view shared by live worlds and world-generation regions.
//!
//! This mirrors vanilla's `LevelReader` role: block behavior such as
//! `canSurvive` should depend on the world-reading surface, not on the concrete
//! `World` type. `World` and `WorldGenRegion` both implement this trait.

use steel_registry::blocks::BlockRef;
use steel_registry::fluid::FluidRef;
use steel_registry::game_events::GameEventRef;
use steel_registry::sound_event::SoundEventRef;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::block_entity::SharedBlockEntity;
use crate::world::game_event_context::GameEventContext;

const VANILLA_HORIZONTAL_LIMIT: i32 = 30_000_000;

/// Read-only level access needed by block behavior and worldgen predicates.
pub trait LevelReader {
    /// Gets the block state at a position.
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId;

    /// Gets the block entity at a position when this level surface supports it
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores position"
    )]
    fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        None
    }

    /// Returns vanilla raw brightness at a position after sky darkening.
    fn raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8;

    /// Returns vanilla `BlockAndLightGetter.canSeeSky`.
    fn can_see_sky(&self, pos: BlockPos) -> bool {
        self.raw_brightness(pos, 0) >= 15
    }

    /// Returns this dimension's vanilla ambient light factor.
    fn ambient_light(&self) -> f32 {
        0.0
    }

    /// Returns the minimum build height.
    fn min_y(&self) -> i32;

    /// Returns the build height.
    fn height(&self) -> i32;

    /// Returns the exclusive maximum build height.
    fn max_y_exclusive(&self) -> i32 {
        self.min_y() + self.height()
    }

    /// Checks if a Y coordinate is outside build height.
    fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y() || y >= self.max_y_exclusive()
    }

    /// Returns vanilla `LevelReader.getMaxLocalRawBrightness`.
    fn max_local_raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8 {
        if pos.x() < -VANILLA_HORIZONTAL_LIMIT
            || pos.z() < -VANILLA_HORIZONTAL_LIMIT
            || pos.x() >= VANILLA_HORIZONTAL_LIMIT
            || pos.z() >= VANILLA_HORIZONTAL_LIMIT
        {
            return 15;
        }

        self.raw_brightness(pos, sky_darkening)
    }

    /// Returns vanilla `LevelReader.getLightLevelDependentMagicValue`.
    fn light_level_dependent_magic_value(&self, pos: BlockPos) -> f32 {
        let value = f32::from(self.max_local_raw_brightness(pos, 0)) / 15.0;
        let curved_value = value / value.mul_add(-3.0, 4.0);
        curved_value + self.ambient_light() * (1.0 - curved_value)
    }

    /// Returns vanilla `LevelReader.getPathfindingCostFromLightLevels`.
    fn pathfinding_cost_from_light_levels(&self, pos: BlockPos) -> f32 {
        self.light_level_dependent_magic_value(pos) - 0.5
    }
}

/// Level access needed by vanilla block `updateShape` logic.
///
/// Vanilla passes both `LevelReader` and `ScheduledTickAccess` to block shape updates.
/// Steel combines those surfaces so the same block behavior can run against a live
/// `World` and a `WorldGenRegion`.
pub trait ScheduledTickAccess: LevelReader {
    /// Returns the fluid tick delay in this level.
    fn fluid_tick_delay(&self, fluid: FluidRef) -> i32;

    /// Schedules a block tick using vanilla's default priority.
    fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) -> bool;

    /// Returns whether a tick is already scheduled for the same `(pos, block)`.
    #[expect(
        unused_variables,
        reason = "most test/worldgen level surfaces do not track scheduled tick presence"
    )]
    fn has_scheduled_block_tick(&self, pos: BlockPos, block: BlockRef) -> bool {
        false
    }

    /// Schedules a fluid tick using vanilla's default priority.
    fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) -> bool;
}

/// Mutable level access needed by vanilla `LevelAccessor` block hooks.
pub trait LevelAccessor: ScheduledTickAccess {
    /// Sets a block state with vanilla update flags.
    fn set_block_state(&self, pos: BlockPos, state: BlockStateId, flags: UpdateFlags) -> bool;

    /// Plays a block sound when this level surface supports runtime side effects.
    #[expect(
        unused_variables,
        reason = "worldgen and test level surfaces do not emit sounds"
    )]
    fn play_block_sound(
        &self,
        sound: SoundEventRef,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
    }

    /// Dispatches a game event when this level surface supports runtime listeners.
    #[expect(
        unused_variables,
        reason = "worldgen and test level surfaces do not emit game events"
    )]
    fn game_event(&self, event: GameEventRef, pos: BlockPos, context: &GameEventContext<'_>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLevel {
        raw_brightness: u8,
        ambient_light: f32,
    }

    impl LevelReader for TestLevel {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            BlockStateId(0)
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            self.raw_brightness
        }

        fn ambient_light(&self) -> f32 {
            self.ambient_light
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    fn assert_f32_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < 0.000_001,
            "left={left}, right={right}"
        );
    }

    #[test]
    fn pathfinding_cost_uses_vanilla_curved_light_value() {
        let level = TestLevel {
            raw_brightness: 6,
            ambient_light: 0.0,
        };

        assert_f32_close(
            level.pathfinding_cost_from_light_levels(BlockPos::ZERO),
            -0.357_142_87,
        );
    }

    #[test]
    fn pathfinding_cost_lerps_toward_full_light_with_ambient_light() {
        let level = TestLevel {
            raw_brightness: 6,
            ambient_light: 0.2,
        };

        assert_f32_close(
            level.pathfinding_cost_from_light_levels(BlockPos::ZERO),
            -0.185_714_3,
        );
    }

    #[test]
    fn max_local_raw_brightness_matches_vanilla_horizontal_limit() {
        let level = TestLevel {
            raw_brightness: 0,
            ambient_light: 0.0,
        };

        assert_eq!(
            level.max_local_raw_brightness(BlockPos::new(29_999_999, 64, 0), 0),
            0
        );
        assert_eq!(
            level.max_local_raw_brightness(BlockPos::new(30_000_000, 64, 0), 0),
            15
        );
    }

    #[test]
    fn can_see_sky_uses_vanilla_sky_light_threshold() {
        assert!(
            TestLevel {
                raw_brightness: 15,
                ambient_light: 0.0,
            }
            .can_see_sky(BlockPos::ZERO)
        );
        assert!(
            !TestLevel {
                raw_brightness: 14,
                ambient_light: 0.0,
            }
            .can_see_sky(BlockPos::ZERO)
        );
    }
}
