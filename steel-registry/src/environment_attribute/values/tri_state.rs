use steel_utils::{DowncastType, DowncastTypeKey};

/// A boolean that may defer to a caller-provided default, as vanilla's
/// `util.TriState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriState {
    True,
    False,
    Default,
}

impl TriState {
    #[must_use]
    pub const fn from_bool(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }

    /// Resolves the state, using `default_value` for [`TriState::Default`].
    #[must_use]
    pub const fn to_boolean(self, default_value: bool) -> bool {
        match self {
            Self::True => true,
            Self::False => false,
            Self::Default => default_value,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Default => "default",
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies `TriState` within the process.
unsafe impl DowncastType for TriState {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:environment_attribute/tri_state");
}
