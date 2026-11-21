use crate::loot::LootTable;
use steel_utils::random::{RandomSource, legacy_random::LegacyRandom};

use super::{OperationError, OperationResult};
use crate::world::World;

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use steel_utils::random::{Random, RandomSplitter};

use super::random_stream::{RandomLease, RandomRequest};

/// Queued acquisition of the exact stream selected for one loot operation.
/// Dropping this future cancels its queue position without drawing randomness.
pub struct LootRandomRequest {
    selection: RandomSelection,
}

enum RandomSelection {
    Seeded(Option<RandomSource>),
    Shared(RandomRequest),
    Failed(Option<OperationError>),
}

/// Owned RNG or logical reservation that can survive between server-job ticks.
/// Shared streams remain reserved until this value is dropped, including during
/// container placement; no mutex guard survives an individual draw.
pub struct OwnedLootRandom {
    source: OwnedSource,
}

enum OwnedSource {
    Seeded(RandomSource),
    Shared(RandomLease),
}

impl Future for LootRandomRequest {
    type Output = OperationResult<OwnedLootRandom>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.selection {
            RandomSelection::Seeded(random) => {
                let Some(random) = random.take() else {
                    panic!("loot random request polled after completion");
                };
                Poll::Ready(Ok(OwnedLootRandom {
                    source: OwnedSource::Seeded(random),
                }))
            }
            RandomSelection::Shared(request) => Pin::new(request).poll(cx).map(|lease| {
                Ok(OwnedLootRandom {
                    source: OwnedSource::Shared(lease),
                })
            }),
            RandomSelection::Failed(error) => {
                let Some(error) = error.take() else {
                    panic!("loot random request polled after completion");
                };
                Poll::Ready(Err(error))
            }
        }
    }
}

macro_rules! forward_draws {
    ($($name:ident($($argument:ident: $ty:ty),*) -> $result:ty;)+) => {$(
        fn $name(&mut self, $($argument: $ty),*) -> $result {
            match &mut self.source {
                OwnedSource::Seeded(random) => random.$name($($argument),*),
                OwnedSource::Shared(random) => random.$name($($argument),*),
            }
        }
    )+};
}

impl Random for OwnedLootRandom {
    fn fork(&mut self) -> Self {
        match &mut self.source {
            OwnedSource::Seeded(random) => Self {
                source: OwnedSource::Seeded(random.fork()),
            },
            OwnedSource::Shared(random) => Self {
                source: OwnedSource::Shared(random.fork()),
            },
        }
    }

    forward_draws! {
        next_i32() -> i32;
        next_i32_bounded(bound: i32) -> i32;
        next_i64() -> i64;
        next_f32() -> f32;
        next_f64() -> f64;
        next_bool() -> bool;
        next_gaussian() -> f64;
        next_positional() -> RandomSplitter;
    }
}

impl World {
    /// Enqueues stream ownership in request order, without waiting on the game tick.
    /// A nonzero seed owns an independent legacy stream; otherwise the table's
    /// named domain sequence takes precedence over the world's live stream.
    pub fn request_loot_random(&self, table: Option<&LootTable>, seed: i64) -> LootRandomRequest {
        let selection = if seed != 0 {
            RandomSelection::Seeded(Some(RandomSource::Legacy(LegacyRandom::from_seed(
                seed as u64,
            ))))
        } else if let Some(key) = table.and_then(|table| table.random_sequence.as_ref()) {
            match self.random_sequences.read().as_ref() {
                Some(sequences) => RandomSelection::Shared(sequences.request(key)),
                None => RandomSelection::Failed(Some(OperationError::RandomUnavailable(
                    "world is not bound to its domain sequences",
                ))),
            }
        } else {
            RandomSelection::Shared(self.loot_random.request())
        };
        LootRandomRequest { selection }
    }

    /// Selects the saved seed, domain sequence, or live world stream in vanilla order.
    /// Generation and container placement must both complete inside the callback.
    /// `None` represents vanilla's unregistered empty fallback table.
    pub fn with_loot_random<T>(
        &self,
        table: Option<&LootTable>,
        seed: i64,
        use_random: impl FnOnce(&mut RandomSource) -> OperationResult<T>,
    ) -> OperationResult<T> {
        if seed != 0 {
            return use_random(&mut RandomSource::Legacy(LegacyRandom::from_seed(
                seed as u64,
            )));
        }
        if let Some(key) = table.and_then(|table| table.random_sequence.as_ref()) {
            let sequences = self.random_sequences.read().as_ref().cloned().ok_or(
                OperationError::RandomUnavailable("world is not bound to its domain sequences"),
            )?;
            return sequences.with_random(key, use_random);
        }
        self.loot_random.try_with_random(use_random)
    }
}

#[cfg(test)]
#[path = "world_random_tests.rs"]
mod tests;
