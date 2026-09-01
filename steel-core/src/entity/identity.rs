use std::sync::atomic::{AtomicU64, Ordering};

static LAST_ENTITY_INSTANCE_ID: AtomicU64 = AtomicU64::new(0);

/// Opaque identity for one runtime construction of an entity.
///
/// This ID is process-local and is never serialized or sent over the protocol.
/// Unlike an entity's numeric ID or UUID, it changes when an entity is reconstructed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityInstanceId(u64);

impl EntityInstanceId {
    pub(super) fn next() -> Self {
        let Ok(previous) =
            LAST_ENTITY_INSTANCE_ID.try_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                last.checked_add(1)
            })
        else {
            panic!("exhausted entity instance IDs");
        };
        Self(previous + 1)
    }
}
