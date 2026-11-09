//! Network encoding of paletted container entries.
//!
//! Entries are packed like vanilla `SimpleBitStorage`: `64 / bits` entries per long, low
//! bits first, no entry spanning two longs, longs written big-endian.

use std::cell::RefCell;
use std::io::{Error, ErrorKind, Result};

use super::ToGlobalId;

/// Largest packed payload: a 16×16×16 section at 16 bits per entry.
pub(super) const MAX_PACKED_BYTES: usize = super::BlockPalette::VOLUME.div_ceil(64 / 16) * 8;
/// Global ids are `u16`-backed, so a table indexed by global id has this many slots.
const INDEX_TABLE_LEN: usize = 1 << 16;
/// Marks an index-table slot whose global id is not in the palette being encoded.
const EMPTY_SLOT: u16 = u16::MAX;

thread_local! {
    /// Palette index per global id, reused across encodes on this thread. It takes 128 KiB on
    /// each thread that encodes a section, allocated on first use.
    ///
    /// Only the slots of the palette being encoded hold an index; every other slot holds
    /// [`EMPTY_SLOT`], so a value missing from its palette is detected instead of being
    /// encoded with an index left over from an earlier palette.
    static PALETTE_INDEX: RefCell<Box<[u16]>> =
        RefCell::new(vec![EMPTY_SLOT; INDEX_TABLE_LEN].into_boxed_slice());
}

/// Packs each value's global id into `buffer` and returns the packed bytes.
///
/// # Errors
/// Returns [`ErrorKind::InvalidData`] if a global id does not fit in `bits`.
pub(super) fn pack_global_ids<'buffer, V: ToGlobalId + Copy>(
    values: &[V],
    bits: u8,
    buffer: &'buffer mut [u8; MAX_PACKED_BYTES],
) -> Result<&'buffer [u8]> {
    // Checked once up front so the packing loop has no per-entry branch.
    let all_ids = values
        .iter()
        .fold(0, |all_ids, value| all_ids | value.to_global_id());
    if all_ids >> u32::from(bits) != 0 {
        return Err(invalid_data(
            "container holds a global id wider than the global palette",
        ));
    }
    pack(values, bits, |value| Some(value.to_global_id()), buffer)
}

/// Packs each value's index in `palette` into `buffer` and returns the packed bytes.
///
/// The thread's index table is released before this returns, so writing the bytes can
/// encode other containers on the same thread.
///
/// # Errors
/// Returns [`ErrorKind::InvalidData`] if a value is missing from `palette`, which means the
/// container's palette invariant is broken.
pub(super) fn pack_palette_indices<'buffer, V: ToGlobalId + Copy>(
    values: &[V],
    palette: &[(V, u16)],
    bits: u8,
    buffer: &'buffer mut [u8; MAX_PACKED_BYTES],
) -> Result<&'buffer [u8]> {
    PALETTE_INDEX.with_borrow_mut(move |table| {
        let table = PaletteIndexTable::fill(table, palette)?;
        pack(values, bits, |value| table.index_of(value), buffer)
    })
}

/// The thread's index table filled with one palette, reset to empty when dropped (including
/// on unwind) so the next palette starts from a clean table.
struct PaletteIndexTable<'a, V: ToGlobalId> {
    slots: &'a mut [u16],
    palette: &'a [(V, u16)],
}

impl<'a, V: ToGlobalId> PaletteIndexTable<'a, V> {
    fn fill(slots: &'a mut [u16], palette: &'a [(V, u16)]) -> Result<Self> {
        // Built before filling so an early return still resets the slots already written.
        let table = Self { slots, palette };
        for (index, (value, _)) in palette.iter().enumerate() {
            let index = u16::try_from(index)
                .ok()
                .filter(|index| *index != EMPTY_SLOT)
                .ok_or_else(|| invalid_data("palette has too many entries to index"))?;
            let slot = table
                .slots
                .get_mut(value.to_global_id() as usize)
                .ok_or_else(|| invalid_data("palette value has an out-of-range global id"))?;
            debug_assert_eq!(*slot, EMPTY_SLOT, "palette entries must be distinct");
            *slot = index;
        }
        Ok(table)
    }

    fn index_of(&self, value: V) -> Option<u32> {
        let index = *self.slots.get(value.to_global_id() as usize)?;
        (index != EMPTY_SLOT).then_some(u32::from(index))
    }
}

impl<V: ToGlobalId> Drop for PaletteIndexTable<'_, V> {
    fn drop(&mut self) {
        for (value, _) in self.palette {
            if let Some(slot) = self.slots.get_mut(value.to_global_id() as usize) {
                *slot = EMPTY_SLOT;
            }
        }
    }
}

/// Packs `values` into `buffer`, failing if a value has no entry.
fn pack<'buffer, V: Copy>(
    values: &[V],
    bits: u8,
    lookup: impl FnMut(V) -> Option<u32>,
    buffer: &'buffer mut [u8; MAX_PACKED_BYTES],
) -> Result<&'buffer [u8]> {
    // A width known at compile time lets each long be filled by a fixed-length loop.
    macro_rules! pack_with_width {
        ($($width:literal)*) => {
            match bits {
                $($width => pack_longs::<V, $width>(values, lookup, buffer),)*
                _ => Err(invalid_data("unsupported bits per entry")),
            }
        };
    }
    pack_with_width!(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
}

/// Packs `values` into the front of `buffer`, failing if a value has no entry.
fn pack_longs<'buffer, V: Copy, const BITS: usize>(
    values: &[V],
    mut lookup: impl FnMut(V) -> Option<u32>,
    buffer: &'buffer mut [u8; MAX_PACKED_BYTES],
) -> Result<&'buffer [u8]> {
    let values_per_long = 64 / BITS;
    let len = values.len().div_ceil(values_per_long) * 8;
    let packed = buffer
        .get_mut(..len)
        .ok_or_else(|| invalid_data("container is too large to pack"))?;
    // Accumulate misses instead of branching per entry; the whole container is rejected.
    let mut missing = false;
    for (entries, long_bytes) in values
        .chunks(values_per_long)
        .zip(packed.as_chunks_mut::<8>().0)
    {
        let mut long = 0u64;
        for (slot, value) in entries.iter().enumerate() {
            let id = lookup(*value);
            missing |= id.is_none();
            long |= u64::from(id.unwrap_or(0)) << (slot * BITS);
        }
        *long_bytes = long.to_be_bytes();
    }
    if missing {
        return Err(invalid_data(
            "container holds a value its palette cannot encode",
        ));
    }
    Ok(packed)
}

fn invalid_data(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{MAX_PACKED_BYTES, pack_global_ids};

    /// Packs values at every width from 1 to 16 bits, including 9 to 15, which no section
    /// uses with vanilla's registries, and reads them back the way vanilla's
    /// `SimpleBitStorage` does: `64 / bits` values per big-endian long, low bits first.
    #[test]
    fn packs_every_width_like_simple_bit_storage() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for bits in 1..=16u8 {
            let mask = (1u64 << bits) - 1;
            let values: Vec<u16> = (0..4096)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state & mask) as u16
                })
                .collect();
            let mut buffer = [0; MAX_PACKED_BYTES];
            let packed =
                pack_global_ids(&values, bits, &mut buffer).expect("every value fits the width");

            let values_per_long = 64 / usize::from(bits);
            let (longs, rest) = packed.as_chunks::<8>();
            assert!(
                rest.is_empty(),
                "{bits} bits: packed bytes should be whole longs"
            );
            assert_eq!(
                longs.len(),
                values.len().div_ceil(values_per_long),
                "{bits} bits"
            );
            for (index, &value) in values.iter().enumerate() {
                let long = u64::from_be_bytes(longs[index / values_per_long]);
                let entry = (long >> (index % values_per_long * usize::from(bits))) & mask;
                assert_eq!(entry, u64::from(value), "{bits} bits, value {index}");
            }
        }
    }
}
