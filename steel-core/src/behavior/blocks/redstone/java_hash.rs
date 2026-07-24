//! Small-map iteration helpers matching the target JDK's `HashMap`.

use steel_utils::BlockPos;

/// Sorts unique positions into Java `HashMap` iteration order while its table
/// remains at the initial 16 buckets.
///
/// Java walks buckets from low to high and preserves insertion order within a
/// collision chain. Stable sorting by the bucket therefore reproduces that
/// order exactly.
pub(super) fn sort_small_map_positions(positions: &mut [BlockPos]) {
    positions.sort_by_key(|pos| bucket(*pos));
}

#[must_use]
pub(super) const fn bucket(pos: BlockPos) -> u32 {
    let hash = pos
        .z()
        .wrapping_mul(31)
        .wrapping_add(pos.y())
        .wrapping_mul(31)
        .wrapping_add(pos.x()) as u32;
    (hash ^ (hash >> 16)) & 15
}
