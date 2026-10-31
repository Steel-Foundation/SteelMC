#[cfg(feature = "host")]
use super::core_types;
use super::types::Vector3;

/// Result of a block interaction, preserving every core interaction outcome.
#[stabby::stabby]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionResult {
    /// Successful action with normal item effects.
    Success,
    /// Successful block action without item effects.
    SuccessWithoutItem,
    /// Successful action requiring a server-directed swing.
    SuccessServer,
    /// Consumed action without a swing.
    Consume,
    /// Rejected action that consumes the interaction.
    Fail,
    /// Continue normal item handling.
    Pass,
    /// Try the empty-hand block interaction.
    TryEmptyHandInteraction,
}

macro_rules! interaction_conversion {
    ($($variant:ident),* $(,)?) => {
        #[cfg(feature = "host")]
        impl From<core_types::InteractionResult> for InteractionResult {
            fn from(value: core_types::InteractionResult) -> Self {
                match value { $(core_types::InteractionResult::$variant => Self::$variant,)* }
            }
        }
        #[cfg(feature = "host")]
        impl From<InteractionResult> for core_types::InteractionResult {
            fn from(value: InteractionResult) -> Self {
                match value { $(InteractionResult::$variant => Self::$variant,)* }
            }
        }
    };
}
interaction_conversion! { Success, SuccessWithoutItem, SuccessServer, Consume, Fail, Pass,
TryEmptyHandInteraction }

/// Velocity and bounce outcome returned by block landing behavior.
#[stabby::stabby]
pub struct MovementResult {
    /// Velocity to apply.
    pub velocity: Vector3,
    /// Whether the landing bounced.
    pub bounced: bool,
}
