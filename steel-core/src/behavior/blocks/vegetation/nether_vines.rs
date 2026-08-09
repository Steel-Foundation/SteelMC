use rand::{Rng, RngExt};

/// Vanilla `NetherVines`.
/// Used for shared behavior for twisting and weeping vines
pub struct NetherVines {}

impl NetherVines {
    /// Vanilla `getBlocksToGrowWhenBonemealed()`
    pub fn get_blocks_to_grow_when_bonemealed(rng: &mut dyn Rng) -> i32 {
        let mut grow_probability = 1.0;

        let mut count = 0;

        while rng.random::<f64>() < grow_probability {
            grow_probability *= 0.826;
            count += 1;
        }
        count
    }
}
