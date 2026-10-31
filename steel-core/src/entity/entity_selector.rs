//! Shared entity predicates.

use super::Entity;

/// excludes creative and spectator players and accepts all other entities
#[must_use]
pub fn no_creative_or_spectator(entity: &dyn Entity) -> bool {
    !entity
        .as_player()
        .is_some_and(|player| entity.is_spectator() || player.has_infinite_materials())
}
