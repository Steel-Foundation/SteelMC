//! The context for game events
use steel_utils::BlockStateId;

use crate::entity::Entity;

/// The context for a game event
#[derive(Clone, Default)]
pub struct GameEventContext<'a> {
    /// The entity that caused the game event
    source_entity: Option<&'a dyn Entity>,
    /// The block state involved in the game event
    affected_state: Option<BlockStateId>,
}

impl<'a> GameEventContext<'a> {
    /// Creates a new `GameEventContext`
    #[must_use]
    pub fn new(
        source_entity: Option<&'a dyn Entity>,
        affected_state: Option<BlockStateId>,
    ) -> Self {
        Self {
            source_entity,
            affected_state,
        }
    }

    /// Returns the entity that caused the game event.
    #[must_use]
    pub fn source_entity(&self) -> Option<&'a dyn Entity> {
        self.source_entity
    }

    /// Returns the block state involved in the game event.
    #[must_use]
    pub const fn affected_state(&self) -> Option<BlockStateId> {
        self.affected_state
    }
}
