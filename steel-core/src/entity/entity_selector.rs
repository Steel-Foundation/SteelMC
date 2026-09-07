//! Shared entity predicates.
//!
//! Mirrors the constants of vanilla `net.minecraft.world.entity.EntitySelector`.
//! Only the predicates Steel actually uses are ported; vanilla's remaining
//! constants are added here as their callers land.

use super::Entity;

/// Mirrors vanilla `EntitySelector.NO_CREATIVE_OR_SPECTATOR`.
///
/// Non-players always pass; players pass unless they are spectating or in a
/// creative-style game mode.
#[must_use]
pub fn no_creative_or_spectator(entity: &dyn Entity) -> bool {
    !entity
        .as_player()
        .is_some_and(|player| entity.is_spectator() || player.has_infinite_materials())
}
