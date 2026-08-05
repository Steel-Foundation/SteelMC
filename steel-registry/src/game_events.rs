use rustc_hash::FxHashMap;
use steel_utils::Identifier;

#[derive(Debug, Clone)]
pub struct GameEvent {
    pub key: Identifier,
    pub notification_radius: i32,
}

pub type GameEventRef = &'static GameEvent;

pub struct GameEventRegistry {
    game_events_by_id: Vec<GameEventRef>,
    game_events_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

// TODO: GameEventListenerRegistry per Chunk Section

impl GameEventRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            game_events_by_id: Vec::new(),
            game_events_by_key: FxHashMap::default(),
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
