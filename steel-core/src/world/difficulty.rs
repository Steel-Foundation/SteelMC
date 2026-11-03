//! Vanilla difficulty scaling based on world and chunk time.

use steel_registry::vanilla_world_clocks;
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, ChunkPos, SectionPos};

use super::World;
use super::environment;

/// Time-adjusted difficulty used by mob spawning and spawn bonuses.
#[derive(Clone, Copy, Debug)]
pub struct DifficultyInstance {
    base: Difficulty,
    effective_difficulty: f32,
}

impl DifficultyInstance {
    const GLOBAL_TIME_OFFSET: f32 = -72_000.0;
    const MAX_GLOBAL_TIME: f32 = 1_440_000.0;
    const MAX_LOCAL_TIME: f32 = 3_600_000.0;
    const MOON_BRIGHTNESS_SCALE: f32 = 0.25;

    /// Builds the instance from vanilla's four difficulty inputs.
    #[must_use]
    pub fn new(
        base: Difficulty,
        total_game_time: i64,
        local_game_time: i64,
        moon_brightness: f32,
    ) -> Self {
        let effective_difficulty =
            Self::calculate(base, total_game_time, local_game_time, moon_brightness);
        Self {
            base,
            effective_difficulty,
        }
    }

    fn calculate(
        base: Difficulty,
        total_game_time: i64,
        local_game_time: i64,
        moon_brightness: f32,
    ) -> f32 {
        if base == Difficulty::Peaceful {
            return 0.0;
        }

        let is_hard = base == Difficulty::Hard;
        let mut scale = 0.75;
        let global_scale = ((total_game_time as f32 + Self::GLOBAL_TIME_OFFSET)
            / Self::MAX_GLOBAL_TIME)
            .clamp(0.0, 1.0)
            * 0.25;
        scale += global_scale;

        let mut local_scale = (local_game_time as f32 / Self::MAX_LOCAL_TIME).clamp(0.0, 1.0)
            * if is_hard { 1.0 } else { 0.75 };
        local_scale += (moon_brightness * Self::MOON_BRIGHTNESS_SCALE).clamp(0.0, global_scale);
        if base == Difficulty::Easy {
            local_scale *= 0.5;
        }

        scale += local_scale;
        f32::from(u8::from(base)) * scale
    }

    /// Returns the configured world difficulty.
    #[must_use]
    pub const fn difficulty(self) -> Difficulty {
        self.base
    }

    /// Returns the time-adjusted difficulty level.
    #[must_use]
    pub const fn effective_difficulty(self) -> f32 {
        self.effective_difficulty
    }

    /// Returns whether the effective difficulty reached vanilla hard.
    #[must_use]
    pub fn is_hard(self) -> bool {
        self.effective_difficulty >= f32::from(u8::from(Difficulty::Hard))
    }

    /// Returns vanilla's equipment and attribute spawn-roll multiplier.
    #[must_use]
    pub fn special_multiplier(self) -> f32 {
        if self.effective_difficulty < 2.0 {
            0.0
        } else if self.effective_difficulty > 4.0 {
            1.0
        } else {
            (self.effective_difficulty - 2.0) / 2.0
        }
    }
}

impl World {
    /// Returns vanilla difficulty scaling for a block position.
    ///
    /// Vanilla only contributes moon brightness and chunk time when the chunk is
    /// loaded at Full status.
    #[must_use]
    pub fn get_current_difficulty_at(&self, pos: BlockPos) -> DifficultyInstance {
        let chunk_pos = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        );
        let local_game_time = self
            .chunk_map
            .with_full_chunk(chunk_pos, |chunk| chunk.common().inhabited_time());

        let (base, total_game_time, moon_brightness) = {
            let level_data = self.level_data.read();
            let base = level_data.data().difficulty;
            let total_game_time = level_data
                .world_clocks()
                .total_ticks(&vanilla_world_clocks::OVERWORLD)
                .unwrap_or(0);
            let moon_brightness = local_game_time.map_or(0.0, |_| {
                environment::moon_brightness(self.dimension_type, level_data.world_clocks())
            });
            (base, total_game_time, moon_brightness)
        };

        DifficultyInstance::new(
            base,
            total_game_time,
            local_game_time.unwrap_or(0),
            moon_brightness,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSE_ENOUGH: f32 = 0.000_001;

    fn assert_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < CLOSE_ENOUGH,
            "left={left}, right={right}"
        );
    }

    #[test]
    fn peaceful_is_always_zero() {
        let instance = DifficultyInstance::new(Difficulty::Peaceful, i64::MAX, i64::MAX, 1.0);
        assert_close(instance.effective_difficulty(), 0.0);
        assert_close(instance.special_multiplier(), 0.0);
        assert!(!instance.is_hard());
    }

    #[test]
    fn normal_difficulty_combines_global_local_and_moon_time() {
        // Global time is capped, local time is capped, and moon brightness is
        // limited to the global time bonus.
        let instance = DifficultyInstance::new(Difficulty::Normal, 1_512_000, 3_600_000, 1.0);
        assert_close(instance.effective_difficulty(), 4.0);
        assert_close(instance.special_multiplier(), 1.0);
        assert!(instance.is_hard());
    }

    #[test]
    fn easy_scales_only_the_local_contribution() {
        let instance = DifficultyInstance::new(Difficulty::Easy, 0, 3_600_000, 0.0);
        // global = 0.0, local = 1.0 * 0.75 * 0.5 = 0.375, scale = 1.125.
        assert_close(instance.effective_difficulty(), 1.125);
        assert_close(instance.special_multiplier(), 0.0);
    }

    #[test]
    fn hard_contributes_more_local_time_than_normal() {
        let hard = DifficultyInstance::new(Difficulty::Hard, 0, 3_600_000, 0.0);
        let normal = DifficultyInstance::new(Difficulty::Normal, 0, 3_600_000, 0.0);
        // hard = 3 * (0.75 + 1.0), normal = 2 * (0.75 + 0.75).
        assert_close(hard.effective_difficulty(), 5.25);
        assert_close(normal.effective_difficulty(), 3.0);
    }
}
