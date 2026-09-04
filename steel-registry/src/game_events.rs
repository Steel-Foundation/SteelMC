use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::{RegistryTags, TaggedRegistryExt};

#[derive(Debug, Clone)]
pub struct GameEvent {
    pub key: Identifier,
    pub notification_radius: i32,
}

impl GameEvent {
    /// Returns `true` if this game event is tagged with `tag`.
    #[must_use]
    pub fn has_tag(&'static self, tag: &Identifier) -> bool {
        crate::REGISTRY.game_events.is_in_tag(self, tag)
    }
}

pub type GameEventRef = &'static GameEvent;

pub struct GameEventRegistry {
    game_events_by_id: Vec<GameEventRef>,
    game_events_by_key: FxHashMap<Identifier, usize>,
    tags: RegistryTags,
    allows_registering: bool,
}

impl GameEventRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            game_events_by_id: Vec::new(),
            game_events_by_key: FxHashMap::default(),
            tags: RegistryTags::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    GameEventRegistry,
    GameEventRef,
    game_events_by_id,
    game_events_by_key,
    allows_registering
);

crate::impl_registry!(
    GameEventRegistry,
    GameEvent,
    game_events_by_id,
    game_events_by_key,
    game_events
);
crate::impl_tagged_registry!(GameEventRegistry, game_events_by_key, "game event");
