use steel_utils::{DowncastType, DowncastTypeKey};

/// Vanilla `world.level.MoonPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MoonPhase {
    FullMoon,
    WaningGibbous,
    ThirdQuarter,
    WaningCrescent,
    NewMoon,
    WaxingCrescent,
    FirstQuarter,
    WaxingGibbous,
}

impl MoonPhase {
    /// Number of moon phases.
    pub const COUNT: i32 = 8;
    /// Ticks each phase lasts on the moon timeline.
    pub const PHASE_LENGTH: i32 = 24_000;

    #[must_use]
    pub const fn index(self) -> i32 {
        self as i32
    }

    /// Tick of the moon timeline at which this phase starts.
    #[must_use]
    pub const fn start_tick(self) -> i32 {
        self.index() * Self::PHASE_LENGTH
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullMoon => "full_moon",
            Self::WaningGibbous => "waning_gibbous",
            Self::ThirdQuarter => "third_quarter",
            Self::WaningCrescent => "waning_crescent",
            Self::NewMoon => "new_moon",
            Self::WaxingCrescent => "waxing_crescent",
            Self::FirstQuarter => "first_quarter",
            Self::WaxingGibbous => "waxing_gibbous",
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies `MoonPhase` within the process.
unsafe impl DowncastType for MoonPhase {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:environment_attribute/moon_phase");
}
