use bitflags::bitflags;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

bitflags! {
    /// Properties that control a ticket's loading, simulation, and persistence behavior.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TicketTypeFlags: u32 {
        const PERSIST = 1 << 0;
        const LOADING = 1 << 1;
        const SIMULATION = 1 << 2;
        const KEEP_DIMENSION_ACTIVE = 1 << 3;
        const CAN_EXPIRE_IF_UNLOADED = 1 << 4;
    }
}

/// A registered chunk ticket kind.
#[derive(Debug)]
pub struct TicketType {
    pub key: Identifier,
    timeout: i64,
    flags: TicketTypeFlags,
}

impl TicketType {
    /// Vanilla's sentinel for a ticket that never expires based on elapsed ticks.
    pub const NO_TIMEOUT: i64 = 0;

    #[must_use]
    pub const fn new(key: Identifier, timeout: i64, flags: TicketTypeFlags) -> Self {
        Self {
            key,
            timeout,
            flags,
        }
    }

    #[must_use]
    pub const fn timeout(&self) -> i64 {
        self.timeout
    }

    #[must_use]
    pub const fn flags(&self) -> TicketTypeFlags {
        self.flags
    }

    #[must_use]
    pub const fn persist(&self) -> bool {
        self.flags.contains(TicketTypeFlags::PERSIST)
    }

    #[must_use]
    pub const fn does_load(&self) -> bool {
        self.flags.contains(TicketTypeFlags::LOADING)
    }

    #[must_use]
    pub const fn does_simulate(&self) -> bool {
        self.flags.contains(TicketTypeFlags::SIMULATION)
    }

    #[must_use]
    pub const fn should_keep_dimension_active(&self) -> bool {
        self.flags.contains(TicketTypeFlags::KEEP_DIMENSION_ACTIVE)
    }

    #[must_use]
    pub const fn can_expire_if_unloaded(&self) -> bool {
        self.flags.contains(TicketTypeFlags::CAN_EXPIRE_IF_UNLOADED)
    }

    #[must_use]
    pub const fn has_timeout(&self) -> bool {
        self.timeout != Self::NO_TIMEOUT
    }
}

pub type TicketTypeRef = &'static TicketType;

/// Registry for vanilla and extension-defined chunk ticket kinds.
pub struct TicketTypeRegistry {
    ticket_types_by_id: Vec<TicketTypeRef>,
    ticket_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl TicketTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            ticket_types_by_id: Vec::new(),
            ticket_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    TicketTypeRegistry,
    TicketTypeRef,
    ticket_types_by_id,
    ticket_types_by_key,
    allows_registering,
    "Cannot register duplicate ticket type key: {}"
);

crate::impl_registry!(
    TicketTypeRegistry,
    TicketType,
    ticket_types_by_id,
    ticket_types_by_key,
    ticket_types
);

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;

    use super::{TicketType, TicketTypeFlags, TicketTypeRegistry};

    static TEST_TICKET: TicketType = TicketType::new(
        Identifier::new_static("test", "ticket"),
        TicketType::NO_TIMEOUT,
        TicketTypeFlags::LOADING,
    );

    #[test]
    #[should_panic(expected = "Cannot register duplicate ticket type key")]
    fn rejects_duplicate_keys() {
        let mut registry = TicketTypeRegistry::new();
        registry.register(&TEST_TICKET);
        registry.register(&TEST_TICKET);
    }
}
