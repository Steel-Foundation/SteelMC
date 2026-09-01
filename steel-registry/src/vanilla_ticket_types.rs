//! Ticket types hardcoded by vanilla 26.2 in `TicketType.java`.

use steel_utils::Identifier;

use crate::ticket_type::{TicketType, TicketTypeFlags, TicketTypeRegistry};

const LOADS: TicketTypeFlags = TicketTypeFlags::LOADING;
const LOADS_AND_SIMULATES: TicketTypeFlags = LOADS.union(TicketTypeFlags::SIMULATION);
const SIMULATES_AND_KEEPS_ACTIVE: TicketTypeFlags =
    TicketTypeFlags::SIMULATION.union(TicketTypeFlags::KEEP_DIMENSION_ACTIVE);
const PERSISTENT_LOADING_SIMULATION: TicketTypeFlags = TicketTypeFlags::PERSIST
    .union(TicketTypeFlags::LOADING)
    .union(TicketTypeFlags::SIMULATION)
    .union(TicketTypeFlags::KEEP_DIMENSION_ACTIVE);
const LOADING_SIMULATION: TicketTypeFlags = TicketTypeFlags::LOADING
    .union(TicketTypeFlags::SIMULATION)
    .union(TicketTypeFlags::KEEP_DIMENSION_ACTIVE);
const LOADING_THAT_EXPIRES_IF_UNLOADED: TicketTypeFlags =
    TicketTypeFlags::LOADING.union(TicketTypeFlags::CAN_EXPIRE_IF_UNLOADED);

pub static PLAYER_SPAWN: TicketType =
    TicketType::new(Identifier::vanilla_static("player_spawn"), 20, LOADS);
pub static SPAWN_SEARCH: TicketType =
    TicketType::new(Identifier::vanilla_static("spawn_search"), 1, LOADS);
pub static DRAGON: TicketType = TicketType::new(
    Identifier::vanilla_static("dragon"),
    TicketType::NO_TIMEOUT,
    LOADS_AND_SIMULATES,
);
pub static PLAYER_LOADING: TicketType = TicketType::new(
    Identifier::vanilla_static("player_loading"),
    TicketType::NO_TIMEOUT,
    LOADS,
);
pub static PLAYER_SIMULATION: TicketType = TicketType::new(
    Identifier::vanilla_static("player_simulation"),
    TicketType::NO_TIMEOUT,
    SIMULATES_AND_KEEPS_ACTIVE,
);
pub static FORCED: TicketType = TicketType::new(
    Identifier::vanilla_static("forced"),
    TicketType::NO_TIMEOUT,
    PERSISTENT_LOADING_SIMULATION,
);
pub static PORTAL: TicketType = TicketType::new(
    Identifier::vanilla_static("portal"),
    300,
    PERSISTENT_LOADING_SIMULATION,
);
pub static ENDER_PEARL: TicketType = TicketType::new(
    Identifier::vanilla_static("ender_pearl"),
    40,
    LOADING_SIMULATION,
);
pub static UNKNOWN: TicketType = TicketType::new(
    Identifier::vanilla_static("unknown"),
    1,
    LOADING_THAT_EXPIRES_IF_UNLOADED,
);

/// Registers the entries in vanilla's declaration order, which defines their IDs.
pub fn register_vanilla_ticket_types(registry: &mut TicketTypeRegistry) {
    registry.register(&PLAYER_SPAWN);
    registry.register(&SPAWN_SEARCH);
    registry.register(&DRAGON);
    registry.register(&PLAYER_LOADING);
    registry.register(&PLAYER_SIMULATION);
    registry.register(&FORCED);
    registry.register(&PORTAL);
    registry.register(&ENDER_PEARL);
    registry.register(&UNKNOWN);
}

#[cfg(test)]
mod tests {
    use super::register_vanilla_ticket_types;
    use crate::RegistryExt;
    use crate::ticket_type::TicketTypeRegistry;

    #[test]
    fn registry_matches_vanilla_order_timeouts_and_behavior() {
        let mut registry = TicketTypeRegistry::new();
        register_vanilla_ticket_types(&mut registry);

        let expected = [
            ("minecraft:player_spawn", 20, 2),
            ("minecraft:spawn_search", 1, 2),
            ("minecraft:dragon", 0, 6),
            ("minecraft:player_loading", 0, 2),
            ("minecraft:player_simulation", 0, 12),
            ("minecraft:forced", 0, 15),
            ("minecraft:portal", 300, 15),
            ("minecraft:ender_pearl", 40, 14),
            ("minecraft:unknown", 1, 18),
        ];

        assert_eq!(registry.len(), expected.len());

        for (id, (key, timeout, flags)) in expected.into_iter().enumerate() {
            let ticket_type = registry
                .by_id(id)
                .unwrap_or_else(|| panic!("missing vanilla ticket type at ID {id}"));

            assert_eq!(ticket_type.key.to_string(), key);
            assert_eq!(ticket_type.timeout(), timeout);
            assert_eq!(ticket_type.flags().bits(), flags);
        }
    }
}
