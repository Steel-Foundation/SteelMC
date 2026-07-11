//! Brigadier-compatible command parsing, execution, and sender handling.

pub(crate) mod brigadier;
mod builtins;
pub(crate) mod execution;
mod pending_execution;
mod protocol;
mod registration;
mod request_queue;
pub mod sender;
pub(crate) mod storage;

pub(crate) use builtins::{
    create_registered_dispatcher, gamemode::handle_client_request, player_can_change_difficulty,
};
pub(crate) use pending_execution::{COMMAND_RESUMPTIONS_PER_TICK, PendingCommandExecutionQueue};
pub(crate) use protocol::{command_suggestions_packet, command_tree_packet};
pub use request_queue::CommandQueueFull;
pub(crate) use request_queue::{COMMAND_REQUESTS_PER_TICK, CommandRequest, CommandRequestQueue};

use steel_utils::entity_events::EntityStatus;

use self::{
    brigadier::CommandDispatcher as BrigadierCommandDispatcher,
    execution::{CommandSource, SteelCommandRuntime},
};
use crate::{player::Player, world::World};

pub(crate) type CommandDispatcher = BrigadierCommandDispatcher<CommandSource, SteelCommandRuntime>;

/// Projects Steel capabilities onto vanilla's shared gamemaster client affordance.
/// Packet handlers still authorize game-mode and difficulty requests independently.
pub(crate) fn client_permission_event(player: &Player, world: &World) -> EntityStatus {
    client_permission_event_for_capabilities(
        builtins::gamemode::player_can_use_client_switcher(player, world),
        builtins::player_can_change_difficulty(player, world),
    )
}

const fn client_permission_event_for_capabilities(
    can_change_game_mode: bool,
    can_change_difficulty: bool,
) -> EntityStatus {
    if can_change_game_mode || can_change_difficulty {
        EntityStatus::PermissionLevelGamemasters
    } else {
        EntityStatus::PermissionLevelAll
    }
}

#[cfg(test)]
mod tests {
    use super::client_permission_event_for_capabilities;
    use steel_utils::entity_events::EntityStatus;

    #[test]
    fn gamemaster_projection_enables_either_supported_client_capability() {
        assert_eq!(
            client_permission_event_for_capabilities(true, false),
            EntityStatus::PermissionLevelGamemasters
        );
        assert_eq!(
            client_permission_event_for_capabilities(false, true),
            EntityStatus::PermissionLevelGamemasters
        );
        assert_eq!(
            client_permission_event_for_capabilities(false, false),
            EntityStatus::PermissionLevelAll
        );
    }
}
