//! Cross-chunk memo of `preliminary_surface_level` per quart column.
//!
//! Vanilla memoizes the level per `NoiseBasedAquifer` in a `Long2IntMap`, so
//! every chunk re-evaluates the flat router for its own ~121-column scan and
//! the columns `computeFluid` samples, most of which its neighbors already
//! evaluated. The level is only ever evaluated at quart-aligned coordinates,
//! where the column cache's grid and raw paths agree, so it is a pure function
//! of the quart column and one generator-wide memo gives identical results.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::density::DimensionNoises;
use crate::noise::aquifer::preliminary_surface_level;

/// Slot-index bits per axis: 512×512 quart columns, i.e. any 128×128-chunk
/// area maps without collisions.
const AXIS_BITS: u32 = 9;
const AXIS_MASK: i32 = (1 << AXIS_BITS) - 1;

/// Packed slot layout: `valid (1) | quart_x (24) | quart_z (24) | level (15)`.
const COORD_BITS: u32 = 24;
const LEVEL_BITS: u32 = 15;
const COORD_MASK: u64 = (1 << COORD_BITS) - 1;
const LEVEL_MASK: u64 = (1 << LEVEL_BITS) - 1;
const VALID: u64 = 1 << 63;
const COORD_MIN: i32 = -(1 << (COORD_BITS - 1));
const COORD_MAX: i32 = (1 << (COORD_BITS - 1)) - 1;
const LEVEL_MIN: i32 = -(1 << (LEVEL_BITS - 1));
const LEVEL_MAX: i32 = (1 << (LEVEL_BITS - 1)) - 1;

/// Generator-wide, lock-free memo of preliminary surface levels.
///
/// Each slot is one atomic word holding the column and its level, so a read
/// either sees a complete entry or misses. Racing writers store the same
/// value, and an evicted or unpackable column is simply recomputed.
pub struct PreliminarySurfaceStore {
    slots: Box<[AtomicU64]>,
}

impl Default for PreliminarySurfaceStore {
    fn default() -> Self {
        Self {
            slots: (0..1usize << (2 * AXIS_BITS))
                .map(|_| AtomicU64::new(0))
                .collect(),
        }
    }
}

impl PreliminarySurfaceStore {
    /// Preliminary surface level of the quart column containing block
    /// `(x, z)`, evaluated through `cache` on a miss.
    #[inline]
    pub fn level<N: DimensionNoises>(
        &self,
        noises: &N,
        cache: &mut N::ColumnCache,
        x: i32,
        z: i32,
    ) -> i32 {
        let quart_x = x >> 2;
        let quart_z = z >> 2;
        let Some(tag) = Self::tag(quart_x, quart_z) else {
            return preliminary_surface_level(noises, cache, x, z);
        };
        let slot =
            &self.slots[(((quart_x & AXIS_MASK) << AXIS_BITS) | (quart_z & AXIS_MASK)) as usize];

        let packed = slot.load(Ordering::Relaxed);
        if packed & !LEVEL_MASK == tag {
            return unpack_level(packed);
        }

        let level = preliminary_surface_level(noises, cache, x, z);
        if let Some(level_bits) = pack_level(level) {
            slot.store(tag | level_bits, Ordering::Relaxed);
        }
        level
    }

    /// Valid bit plus both coordinates, or `None` if a coordinate does not fit.
    #[inline]
    fn tag(quart_x: i32, quart_z: i32) -> Option<u64> {
        if !(COORD_MIN..=COORD_MAX).contains(&quart_x)
            || !(COORD_MIN..=COORD_MAX).contains(&quart_z)
        {
            return None;
        }
        Some(
            VALID
                | ((quart_x as u64 & COORD_MASK) << (COORD_BITS + LEVEL_BITS))
                | ((quart_z as u64 & COORD_MASK) << LEVEL_BITS),
        )
    }
}

/// Low 15 bits of `level`, or `None` if it does not fit.
#[inline]
fn pack_level(level: i32) -> Option<u64> {
    (LEVEL_MIN..=LEVEL_MAX)
        .contains(&level)
        .then_some(level as u64 & LEVEL_MASK)
}

/// Sign-extends the packed 15-bit level.
#[inline]
const fn unpack_level(packed: u64) -> i32 {
    (((packed & LEVEL_MASK) as i32) << (32 - LEVEL_BITS)) >> (32 - LEVEL_BITS)
}

#[cfg(test)]
mod tests {
    use steel_utils::random::{Random, xoroshiro::Xoroshiro};

    use super::{
        AXIS_BITS, LEVEL_MAX, LEVEL_MIN, PreliminarySurfaceStore, pack_level, unpack_level,
    };
    use crate::density_functions::overworld::{OverworldColumnCache, OverworldNoises};
    use crate::noise::aquifer::preliminary_surface_level;
    use crate::noise_parameters::get_noise_parameters;

    #[test]
    fn packed_levels_round_trip_through_colliding_slots() {
        let seed = 1;
        let splitter = Xoroshiro::from_seed(seed).next_positional();
        let noises = OverworldNoises::create(seed, &splitter, &get_noise_parameters());
        let store = PreliminarySurfaceStore::default();
        let mut cache = OverworldColumnCache::default();

        // Same slot, `1 << AXIS_BITS` quarts apart on each axis.
        let wrap = 4 << AXIS_BITS;
        let mut columns = vec![(0, 0), (wrap, 0), (0, -wrap), (-wrap, wrap)];
        // Negative and unaligned block coordinates.
        for x in (-3_000..3_000).step_by(373) {
            for z in (-3_000..3_000).step_by(419) {
                columns.push((x, z));
            }
        }

        // Two passes: the first misses and stores, the second reads back —
        // except where a colliding column evicted the entry in between.
        for _ in 0..2 {
            for &(x, z) in &columns {
                let expected = preliminary_surface_level(&noises, &mut cache, x, z);
                assert_eq!(
                    store.level(&noises, &mut cache, x, z),
                    expected,
                    "column ({x}, {z})"
                );
            }
        }
    }

    #[test]
    fn levels_keep_sign_and_reject_out_of_range() {
        for level in [LEVEL_MIN, -64, -1, 0, 63, 320, LEVEL_MAX] {
            let packed = pack_level(level).expect("in range");
            assert_eq!(unpack_level(packed), level);
        }
        assert_eq!(pack_level(LEVEL_MIN - 1), None);
        assert_eq!(pack_level(LEVEL_MAX + 1), None);
    }
}
