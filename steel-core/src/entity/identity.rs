use std::sync::atomic::{AtomicU64, Ordering};

static LAST_ENTITY_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Opaque generation counter for one runtime construction of an entity.
///
/// This generation is process-local and is never serialized or sent over the protocol.
/// Unlike an entity's numeric ID or UUID, it changes when an entity is reconstructed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityGeneration(u64);

impl EntityGeneration {
    pub(super) fn next() -> Self {
        let Ok(previous) =
            LAST_ENTITY_GENERATION.try_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                last.checked_add(1)
            })
        else {
            panic!("exhausted entity generations");
        };
        Self(previous + 1)
    }
}
