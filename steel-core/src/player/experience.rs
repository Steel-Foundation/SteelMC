/// Holds the total amount of experience points a player has.
/// Vanilla uses signed `i32` for XP — negative values are clamped to 0.
#[derive(Default, Copy, Clone, Debug)]
pub struct Experience {
    total_points: i32,
    /// Whether the `total_points` has changed since the last time the client was updated
    pub dirty: bool,
}

impl Experience {
    /// A new Experience state with `total_points`
    #[must_use]
    pub fn new(total_points: i32) -> Self {
        Self {
            total_points: total_points.max(0),
            dirty: true,
        }
    }

    /// Points required to go from `level` to `level + 1`.
    #[must_use]
    pub const fn points_for_level(level: i32) -> i32 {
        match level {
            ..0 => 0,
            0..15 => 2 * level + 7,
            15..30 => 37 + (level - 15) * 5,
            _ => 9 * (level - 30) + 112,
        }
    }

    /// Calculates the total cumulative points at a specific level
    #[must_use]
    pub const fn total_points_at_level(level: i32) -> i32 {
        match level {
            ..0 => 0,
            0..=15 => level * level + 6 * level,
            16..=30 => 360 + ((level * (5 * level - 81)) / 2),
            _ => (level * (level * 9 - 325) / 2) + 2220,
        }
    }

    /// Calculates the current level from the total cumulative points
    #[must_use]
    pub fn level(self) -> i32 {
        let points = f64::from(self.total_points);
        if (0.0..=315.0).contains(&points) {
            return f64::midpoint(-6.0, f64::sqrt(36.0 + 4.0 * points)) as i32;
        } else if (316.0..=1507.0).contains(&points) {
            return ((40.5 + f64::sqrt(-1959.75 + 10.0 * points)) / 5.0) as i32;
        }
        ((162.5 + f64::sqrt(-13553.75 + 18.0 * points)) / 9.0) as i32
    }

    /// The points of the player to the next level
    #[must_use]
    pub fn points(self) -> i32 {
        self.total_points - Self::total_points_at_level(self.level())
    }

    /// The total points of experience the player has
    #[must_use]
    pub const fn total_points(self) -> i32 {
        self.total_points
    }

    /// Returns the progress to the next level between 0.0 and 1.0
    #[must_use]
    pub fn progress(self) -> f64 {
        let level = self.level();

        f64::from(self.total_points - Self::total_points_at_level(level))
            / f64::from(Self::points_for_level(level))
    }

    /// Add levels to the total experience
    pub fn add_levels(&mut self, additional_levels: i32) {
        let progress = self.progress();
        let new_level = self.level().saturating_add(additional_levels);
        let new_total_points = Self::total_points_at_level(new_level)
            + (progress * f64::from(Self::points_for_level(new_level))).round() as i32;

        self.set_total_points(new_total_points);
    }

    /// Add points to the total experience
    pub const fn add_points(&mut self, additional_points: i32) {
        let new_total_points = self.total_points.saturating_add(additional_points);

        self.set_total_points(new_total_points);
    }

    /// Sets the level while keeping the progress
    pub fn set_levels(&mut self, level: i32) {
        let progress = self.progress();

        let new_total_points = Self::total_points_at_level(level)
            + (f64::from(Self::points_for_level(level)) * progress).round() as i32;

        self.set_total_points(new_total_points);
    }

    /// Sets the points in the current level
    pub fn set_points(&mut self, points: i32) -> Result<(), String> {
        let level = self.level();
        let points_for_level = Self::points_for_level(level);
        if points >= points_for_level || points < 0 {
            return Err(format!(
                "Cannot set to invalid amount of points for the current level {level}: {points}/{points_for_level}"
            ));
        }
        let new_total_points = Self::total_points_at_level(level) + points;

        self.set_total_points(new_total_points);
        Ok(())
    }

    /// Sets the progress bar to the given percentage
    /// If a value is passed in that is close to 100% the next closest representable value will be chosen
    pub fn set_progress(&mut self, progress: f64) {
        let progress = progress.clamp(0.0, 1.0);
        let level = self.level();
        let points_for_level = Self::points_for_level(level);

        let points = (f64::from(points_for_level) * progress).round() as i32;
        let new_total_points =
            Self::total_points_at_level(level) + points.min(points_for_level - 1);

        self.set_total_points(new_total_points);
    }

    /// Sets the total experience points
    pub const fn set_total_points(&mut self, new_total_points: i32) {
        self.total_points = if new_total_points < 0 {
            0
        } else {
            new_total_points
        };
        self.dirty = true;
    }
}
