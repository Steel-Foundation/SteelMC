//! Verifies the world selection surface from outside the `steel-core` crate.

use std::sync::Arc;

use glam::DVec3;
use steel_core::player::Player;
use steel_core::server::{PreparedSpawn, Server, WorldEntry};
use steel_core::world::World;

/// `Server::queue_player_world_selection`, spelled with paths a plugin can use.
type QueueWorldSelection = fn(&Server, Arc<Player>, &Arc<World>, WorldEntry) -> Result<(), String>;

#[test]
fn a_plugin_can_name_every_world_selection_argument() {
    let queue: QueueWorldSelection = Server::queue_player_world_selection;
    let entries = [
        WorldEntry::Usual,
        WorldEntry::Rotation((90.0, 0.0)),
        WorldEntry::Exact(PreparedSpawn {
            position: DVec3::new(0.5, 64.0, 0.5),
            rotation: (90.0, 0.0),
        }),
    ];
    let _ = (queue, entries);
}
