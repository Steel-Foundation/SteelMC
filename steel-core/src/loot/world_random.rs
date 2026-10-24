use crate::loot::LootTable;
use steel_utils::random::{RandomSource, legacy_random::LegacyRandom};

use super::{LootError, LootResult};
use crate::world::World;

impl World {
    // TODO: Route other gameplay loot callers through this selection, preserving
    // explicit seeds and sharing named sequences across repeated evaluations.
    // Death-loot contexts also need the credited player's luck.
    /// Selects the saved seed, domain sequence, or live world stream in vanilla order.
    /// Generation and container placement must both complete inside the callback.
    pub fn with_loot_random<T>(
        &self,
        table: &LootTable,
        seed: i64,
        use_random: impl FnOnce(&mut RandomSource) -> LootResult<T>,
    ) -> LootResult<T> {
        if seed != 0 {
            return use_random(&mut RandomSource::Legacy(LegacyRandom::from_seed(
                seed as u64,
            )));
        }
        if let Some(key) = &table.random_sequence {
            let sequences = self.random_sequences.read().as_ref().cloned().ok_or(
                LootError::RandomUnavailable("world is not bound to its domain sequences"),
            )?;
            return sequences.with_random(key, use_random);
        }
        let Some(mut random) = self.loot_random.try_lock() else {
            return Err(LootError::RandomUnavailable(
                "world stream is already in use",
            ));
        };
        use_random(&mut random)
    }
}
