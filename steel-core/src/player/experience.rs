/// Vanilla player experience state.
///
/// These three fields are intentionally independent. Vanilla commands and save
/// data can change one without recomputing the others. The player's score is
/// synchronized entity data and is owned by [`super::Player`], not experience.
#[derive(Default, Copy, Clone, Debug)]
pub struct Experience {
    level: i32,
    progress: f32,
    total_points: i32,
    /// Whether the client experience packet needs to be sent.
    pub dirty: bool,
}

impl Experience {
    /// Builds a coherent experience state from a total point count.
    #[must_use]
    pub fn new(total_points: i32) -> Self {
        let total_points = total_points.max(0);
        let level = level_for_total_points(total_points);
        let points_into_level = total_points - Self::total_points_at_level(level);
        let points_for_level = Self::points_for_level(level);
        let progress = if points_for_level > 0 {
            points_into_level as f32 / points_for_level as f32
        } else {
            0.0
        };

        Self {
            level,
            progress,
            total_points,
            dirty: true,
        }
    }

    /// Restores the three independent vanilla experience fields without normalizing them.
    #[must_use]
    pub const fn from_parts(level: i32, progress: f32, total_points: i32) -> Self {
        Self {
            level,
            progress,
            total_points,
            dirty: true,
        }
    }

    /// Points required to advance from `level` to `level + 1`.
    #[must_use]
    pub const fn points_for_level(level: i32) -> i32 {
        if level >= 30 {
            level.wrapping_sub(30).wrapping_mul(9).wrapping_add(112)
        } else if level >= 15 {
            level.wrapping_sub(15).wrapping_mul(5).wrapping_add(37)
        } else {
            level.wrapping_mul(2).wrapping_add(7)
        }
    }

    /// Returns a coherent cumulative point count at the start of `level`.
    ///
    /// This is a Steel construction helper, not a field vanilla derives at runtime.
    #[must_use]
    pub const fn total_points_at_level(level: i32) -> i32 {
        if level <= 0 {
            return 0;
        }

        let level = level as i128;
        let points = if level <= 15 {
            level * level + 6 * level
        } else if level <= 30 {
            360 + level * (5 * level - 81) / 2
        } else {
            level * (9 * level - 325) / 2 + 2220
        };
        if points > i32::MAX as i128 {
            i32::MAX
        } else {
            points as i32
        }
    }

    /// Current experience level.
    #[must_use]
    pub const fn level(&self) -> i32 {
        self.level
    }

    /// Experience points within the current level, matching `Mth.floor`.
    #[must_use]
    pub fn points(&self) -> i32 {
        (self.progress * Self::points_for_level(self.level) as f32).floor() as i32
    }

    /// Vanilla `totalExperience`.
    #[must_use]
    pub const fn total_points(&self) -> i32 {
        self.total_points
    }

    /// Progress toward the next level.
    #[must_use]
    pub const fn progress(&self) -> f32 {
        self.progress
    }

    /// Adds levels like vanilla `Player.giveExperienceLevels`.
    pub const fn add_levels(&mut self, additional_levels: i32) {
        if additional_levels == 0 {
            return;
        }

        self.level = self.level.saturating_add(additional_levels);
        if self.level < 0 {
            self.level = 0;
            self.progress = 0.0;
            self.total_points = 0;
        }
        self.dirty = true;
    }

    /// Adds raw points like vanilla `Player.giveExperiencePoints`.
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla performs these calculations with Java float precision"
    )]
    pub fn add_points(&mut self, additional_points: i32) {
        if additional_points == 0 {
            return;
        }

        self.progress += additional_points as f32 / Self::points_for_level(self.level) as f32;
        self.total_points = self.total_points.wrapping_add(additional_points).max(0);

        while self.progress < 0.0 {
            let remaining = self.progress * Self::points_for_level(self.level) as f32;
            if self.level > 0 {
                self.add_levels(-1);
                self.progress = 1.0 + remaining / Self::points_for_level(self.level) as f32;
            } else {
                self.add_levels(-1);
                self.progress = 0.0;
            }
        }

        while self.progress >= 1.0 {
            self.progress = (self.progress - 1.0) * Self::points_for_level(self.level) as f32;
            self.add_levels(1);
            self.progress /= Self::points_for_level(self.level) as f32;
        }

        self.dirty = true;
    }

    /// Sets the current level without changing progress or total experience.
    pub const fn set_levels(&mut self, level: i32) {
        if self.level != level {
            self.level = level;
            self.dirty = true;
        }
    }

    /// Whether `/experience set ... points` accepts `points` at the current level.
    #[must_use]
    pub const fn can_set_points(&self, points: i32) -> bool {
        points >= 0 && points < Self::points_for_level(self.level)
    }

    /// Sets points within the current level like `ServerPlayer.setExperiencePoints`.
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla performs these calculations with Java float precision"
    )]
    pub fn set_points(&mut self, points: i32) {
        let limit = Self::points_for_level(self.level) as f32;
        let maximum = (limit - 1.0) / limit;
        let requested = points as f32 / limit;
        let progress = if requested < 0.0 {
            0.0
        } else {
            requested.min(maximum)
        };
        if self.progress.to_bits() != progress.to_bits() {
            self.progress = progress;
            self.dirty = true;
        }
    }

    /// Clears level, progress, and total experience.
    pub const fn clear(&mut self) {
        self.level = 0;
        self.progress = 0.0;
        self.total_points = 0;
        self.dirty = true;
    }

    /// Base XP reward dropped on death: `min(level * 7, 100)`.
    #[must_use]
    pub const fn death_xp_reward(&self) -> i32 {
        let reward = self.level.wrapping_mul(7);
        if reward < 100 { reward } else { 100 }
    }
}

fn level_for_total_points(total_points: i32) -> i32 {
    let points = f64::from(total_points);
    if points <= 315.0 {
        return f64::midpoint(-6.0, f64::sqrt(36.0 + 4.0 * points)) as i32;
    }
    if points <= 1507.0 {
        return ((40.5 + f64::sqrt(-1959.75 + 10.0 * points)) / 5.0) as i32;
    }
    ((162.5 + f64::sqrt(-13553.75 + 18.0 * points)) / 9.0) as i32
}
