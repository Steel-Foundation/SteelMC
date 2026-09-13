//! Steel-owned ticket types used by server internals.

use steel_utils::Identifier;

use crate::ticket_type::{TicketType, TicketTypeFlags, TicketTypeRegistry};

/// Keeps a requested chunk loading for the lifetime of its request lease.
pub static CHUNK_REQUEST: TicketType = TicketType::new(
    Identifier::new_static("steel", "chunk_request"),
    TicketType::NO_TIMEOUT,
    TicketTypeFlags::LOADING,
);

pub fn register_steel_ticket_types(registry: &mut TicketTypeRegistry) {
    registry.register(&CHUNK_REQUEST);
}

#[cfg(test)]
mod tests {
    use super::CHUNK_REQUEST;
    use crate::vanilla_ticket_types::UNKNOWN;
    use crate::{Registry, RegistryExt};

    #[test]
    fn chunk_request_is_registered_after_vanilla_types() {
        let registry = Registry::new_vanilla();

        assert_eq!(registry.ticket_types.len(), 10);
        assert_eq!(registry.ticket_types.by_id(8), Some(&UNKNOWN));
        assert_eq!(registry.ticket_types.by_id(9), Some(&CHUNK_REQUEST));
        assert!(CHUNK_REQUEST.does_load());
        assert!(!CHUNK_REQUEST.has_timeout());
    }
}
