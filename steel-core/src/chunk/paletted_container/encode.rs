//! Network encoding of paletted container entries.
//!
//! Entries are packed like vanilla `SimpleBitStorage`: `64 / bits` entries per long, low
//! bits first, no entry spanning two longs, longs written big-endian.

use std::io::{Error, ErrorKind, Result};

use super::{BITS_PER_WORD, BlockPalette};

const BYTES_PER_WORD: usize = size_of::<u64>();

/// Largest packed payload: a block section at the width of a global ID.
pub(super) const MAX_PACKED_BYTES: usize =
    BlockPalette::VOLUME.div_ceil(BITS_PER_WORD / u16::BITS as usize) * BYTES_PER_WORD;

/// Packs ids, rejecting any id >= limit.
pub(super) fn pack_u32_iter(
    ids: impl ExactSizeIterator<Item = u32>,
    bits: u8,
    limit: u32,
    buffer: &mut [u8; MAX_PACKED_BYTES],
) -> Result<&[u8]> {
    macro_rules! pack_with_width {
        ($($width:literal)*) => {
            match bits {
                $($width => pack_longs_iter::<$width>(ids, limit, buffer),)*
                _ => Err(invalid_data("unsupported bits per entry")),
            }
        };
    }
    pack_with_width!(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
}

/// Packs ids into big-endian words, one word at a time.
fn pack_longs_iter<const BITS: usize>(
    mut ids: impl ExactSizeIterator<Item = u32>,
    limit: u32,
    buffer: &mut [u8; MAX_PACKED_BYTES],
) -> Result<&[u8]> {
    let count = ids.len();
    let per_word = BITS_PER_WORD / BITS;
    let packed = packed_bytes(count, per_word, buffer)?;
    let mut max_id = 0;

    for (bytes, start) in packed
        .as_chunks_mut::<BYTES_PER_WORD>()
        .0
        .iter_mut()
        .zip((0..count).step_by(per_word))
    {
        let mut word = 0;
        for slot in 0..per_word.min(count - start) {
            let id = ids.next().ok_or_else(invalid_entry)?;
            max_id = max_id.max(id);
            word |= u64::from(id) << (slot * BITS);
        }

        *bytes = word.to_be_bytes();
    }

    if max_id >= limit {
        return Err(invalid_entry());
    }

    Ok(packed)
}

fn packed_bytes(
    count: usize,
    per_word: usize,
    buffer: &mut [u8; MAX_PACKED_BYTES],
) -> Result<&mut [u8]> {
    buffer
        .get_mut(..count.div_ceil(per_word) * BYTES_PER_WORD)
        .ok_or_else(|| invalid_data("container is too large to pack"))
}

fn invalid_entry() -> Error {
    invalid_data("container holds a value its palette cannot encode")
}

/// Reuses aligned words or repacks their indices with both widths dispatched once.
pub(super) fn pack_local<'a>(
    words: &[u64],
    count: usize,
    storage_bits: u8,
    packet_bits: u8,
    limit: u32,
    buffer: &'a mut [u8; MAX_PACKED_BYTES],
) -> Result<&'a [u8]> {
    if storage_bits == packet_bits {
        return copy_words(words, count, storage_bits, limit, buffer);
    }

    match (storage_bits, packet_bits) {
        (1, 4) => widen_to_nibbles::<1>(words, count, limit, buffer),
        (2, 4) => widen_to_nibbles::<2>(words, count, limit, buffer),
        (8, 5..=7) => pack_index_bytes(words, count, packet_bits, limit, buffer),
        _ => pack_remapped(
            words,
            count,
            storage_bits,
            packet_bits,
            limit,
            |index| index as u32,
            buffer,
        ),
    }
}

fn pack_index_bytes<'a>(
    words: &[u64],
    count: usize,
    packet_bits: u8,
    limit: u32,
    buffer: &'a mut [u8; MAX_PACKED_BYTES],
) -> Result<&'a [u8]> {
    let mut indices = [0u8; BlockPalette::VOLUME];
    let (chunks, _) = indices.as_chunks_mut::<BYTES_PER_WORD>();
    for (bytes, &word) in chunks.iter_mut().zip(words) {
        *bytes = word.to_le_bytes();
    }

    pack_u32_iter(
        indices[..count].iter().copied().map(u32::from),
        packet_bits,
        limit,
        buffer,
    )
}

/// translates four 16 bit palette indices in each input word to global id
pub(super) fn translate_words<'a>(
    words: &[u64],
    count: usize,
    mut to_id: impl FnMut(usize) -> u32,
    buffer: &'a mut [u8; MAX_PACKED_BYTES],
) -> Result<&'a [u8]> {
    const BITS: usize = u16::BITS as usize;
    const PER_WORD: usize = BITS_PER_WORD / BITS;

    let packed = packed_bytes(count, PER_WORD, buffer)?;
    let (full, tail) = packed
        .as_chunks_mut::<BYTES_PER_WORD>()
        .0
        .split_at_mut(count / PER_WORD);
    let mut combined_ids = 0;

    for (bytes, &indices) in full.iter_mut().zip(words) {
        let id_0 = to_id((indices as u16) as usize);
        let id_1 = to_id(((indices >> BITS) as u16) as usize);
        let id_2 = to_id(((indices >> (2 * BITS)) as u16) as usize);
        let id_3 = to_id((indices >> (3 * BITS)) as usize);
        combined_ids |= id_0 | id_1 | id_2 | id_3;

        let word = u64::from(id_0)
            | (u64::from(id_1) << BITS)
            | (u64::from(id_2) << (2 * BITS))
            | (u64::from(id_3) << (3 * BITS));
        *bytes = word.to_be_bytes();
    }

    if let Some(bytes) = tail.first_mut() {
        let indices = words[count / PER_WORD];
        let mut word = 0;

        for slot in 0..count % PER_WORD {
            let shift = slot * BITS;
            let id = to_id(((indices >> shift) as u16) as usize);
            combined_ids |= id;
            word |= u64::from(id) << shift;
        }

        *bytes = word.to_be_bytes();
    }

    if combined_ids > u32::from(u16::MAX) {
        return Err(invalid_entry());
    }

    Ok(packed)
}

/// repackages aligned indices into packet words optionally translating them first
pub(super) fn pack_remapped<'a>(
    words: &[u64],
    count: usize,
    storage_bits: u8,
    packet_bits: u8,
    limit: u32,
    mut map_index: impl FnMut(usize) -> u32,
    buffer: &'a mut [u8; MAX_PACKED_BYTES],
) -> Result<&'a [u8]> {
    let bits = usize::from(storage_bits);
    let mask = (1u64 << bits) - 1;

    pack_u32_iter(
        (0..count).map(|i| {
            let offset = i * bits;
            map_index(((words[offset / BITS_PER_WORD] >> (offset % BITS_PER_WORD)) & mask) as usize)
        }),
        packet_bits,
        limit,
        buffer,
    )
}

fn copy_words<'a>(
    words: &[u64],
    count: usize,
    bits: u8,
    limit: u32,
    buffer: &'a mut [u8; MAX_PACKED_BYTES],
) -> Result<&'a [u8]> {
    let bits = usize::from(bits);
    let per_word = BITS_PER_WORD / bits;
    let mask = (1u64 << bits) - 1;
    let packed = packed_bytes(count, per_word, buffer)?;
    let (output, _) = packed.as_chunks_mut::<BYTES_PER_WORD>();

    let (full, tail) = output.split_at_mut(count / per_word);
    if u64::from(limit) <= mask {
        let lanes = u64::MAX / mask;
        let high_bit = 1 << (bits - 1);
        let highs = high_bit * lanes;
        let lows = (u64::from(limit) & (high_bit - 1)) * lanes;

        if words.iter().enumerate().any(|(i, &word)| {
            let word = if i == full.len() {
                word & (u64::MAX >> (BITS_PER_WORD - count % per_word * bits))
            } else {
                word
            };

            // set every lane high bit before sub the low part of the limit (prevents borrow across lane)
            let low_ge = (word | highs).wrapping_sub(lows) & highs;
            if u64::from(limit) < high_bit {
                (word & highs) | low_ge != 0
            } else {
                word & low_ge != 0
            }
        }) {
            return Err(invalid_entry());
        }
    }

    for (bytes, &word) in full.iter_mut().zip(words) {
        *bytes = word.to_be_bytes();
    }

    if let Some(bytes) = tail.first_mut() {
        let padding = (per_word - count % per_word) * bits;
        *bytes = (words[count / per_word] & (u64::MAX >> padding)).to_be_bytes();
    }

    Ok(packed)
}

fn widen_to_nibbles<'a, const BITS: usize>(
    words: &[u64],
    count: usize,
    limit: u32,
    buffer: &'a mut [u8; MAX_PACKED_BYTES],
) -> Result<&'a [u8]> {
    const NIBBLES_PER_WORD: usize = BITS_PER_WORD / 4;
    const LOW_BITS_OF_PAIRS: u64 = 0x5555_5555;

    let packed = packed_bytes(count, NIBBLES_PER_WORD, buffer)?;
    for (i, bytes) in packed
        .as_chunks_mut::<BYTES_PER_WORD>()
        .0
        .iter_mut()
        .enumerate()
    {
        let offset = i * NIBBLES_PER_WORD * BITS;
        let entries = NIBBLES_PER_WORD.min(count - i * NIBBLES_PER_WORD);
        let mut word = (words[offset / BITS_PER_WORD] >> (offset % BITS_PER_WORD))
            & ((1 << (entries * BITS)) - 1);

        if BITS == 2 {
            // With two entries either code with the high bit set is invalid
            let high_bits = (word >> 1) & LOW_BITS_OF_PAIRS;
            if (limit == 2 && high_bits != 0) || (limit == 3 && word & high_bits != 0) {
                return Err(invalid_entry());
            }
        }

        if BITS == 1 {
            word = (word | (word << 8)) & 0x00ff_00ff;
            word = (word | (word << 4)) & 0x0f0f_0f0f;
            word = (word | (word << 2)) & 0x3333_3333;
            word = (word | (word << 1)) & LOW_BITS_OF_PAIRS;
        }

        word = (word | (word << 16)) & 0x0000_ffff_0000_ffff;
        word = (word | (word << 8)) & 0x00ff_00ff_00ff_00ff;
        word = (word | (word << 4)) & 0x0f0f_0f0f_0f0f_0f0f;
        word = (word | (word << 2)) & 0x3333_3333_3333_3333;
        *bytes = word.to_be_bytes();
    }

    Ok(packed)
}

fn invalid_data(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{MAX_PACKED_BYTES, pack_local, pack_u32_iter, translate_words};

    fn reference_words(values: &[u32], bits: u8) -> Vec<u64> {
        let bits = usize::from(bits);
        let per_word = 64 / bits;
        let mut words = vec![0u64; values.len().div_ceil(per_word)];
        for (index, &value) in values.iter().enumerate() {
            let shift = (index % per_word) * bits;
            words[index / per_word] |= u64::from(value) << shift;
        }
        words
    }

    fn reference_bytes(values: &[u32], bits: u8) -> Vec<u8> {
        reference_words(values, bits)
            .into_iter()
            .flat_map(u64::to_be_bytes)
            .collect()
    }

    #[test]
    fn local_paths_match_reference_words_and_validate_all_live_indices() {
        for (storage_bits, packet_bits, limit) in [
            (1, 1, 2),
            (1, 4, 2),
            (2, 2, 3),
            (2, 4, 2),
            (2, 4, 3),
            (2, 4, 4),
            (4, 3, 5),
            (4, 4, 5),
            (4, 4, 8),
            (4, 4, 13),
            (4, 4, 16),
            (8, 5, 30),
            (8, 6, 63),
            (8, 7, 126),
            (8, 8, 223),
            (8, 8, 256),
            (16, 4, 11),
        ] {
            let input_lanes = 64 / usize::from(storage_bits);
            for count in [1usize, 37, 64, 4096] {
                let values: Vec<u32> = (0..count)
                    .map(|i| (i * 79 + i / 13) as u32 % limit)
                    .collect();
                let mut words = reference_words(&values, storage_bits);
                let final_bits = count % input_lanes * usize::from(storage_bits);
                if final_bits != 0 {
                    let last = words
                        .last_mut()
                        .expect("partial word at the end of the input");
                    *last |= u64::MAX << final_bits;
                }
                let expected = reference_bytes(&values, packet_bits);
                let mut output = [0xff; MAX_PACKED_BYTES];
                assert_eq!(
                    pack_local(&words, count, storage_bits, packet_bits, limit, &mut output)
                        .expect("valid local indices should encode"),
                    expected
                );
                if limit < 1 << storage_bits {
                    for index in [0, count - 1] {
                        let shift = index % input_lanes * usize::from(storage_bits);
                        let mask = ((1u64 << storage_bits) - 1) << shift;
                        let word_index = index / input_lanes;
                        let valid_word = words[word_index];
                        words[word_index] = (valid_word & !mask) | (u64::from(limit) << shift);
                        assert!(
                            pack_local(
                                &words,
                                count,
                                storage_bits,
                                packet_bits,
                                limit,
                                &mut output,
                            )
                            .is_err(),
                            "{storage_bits} to {packet_bits} bits: invalid entry at {index} of {count}"
                        );
                        words[word_index] = valid_word;
                    }
                }
            }
        }
    }

    #[test]
    fn translates_global_words_and_rejects_invalid_indices_or_ids() {
        let words = [0x0003_0002_0001_0000, 0xffff_ffff_ffff_0004];
        let ids = [97, 500, 342, 1024, 32768];
        let mut buffer = [0xff; MAX_PACKED_BYTES];
        let expected = reference_bytes(&ids, 16);
        let translate = |index: usize| ids.get(index).copied().unwrap_or(u32::MAX);
        assert_eq!(
            translate_words(&words, 5, translate, &mut buffer).expect("every live global ID fits"),
            expected
        );
        assert!(translate_words(&words, 6, translate, &mut buffer).is_err());
        assert!(
            translate_words(
                &words,
                5,
                |index| {
                    if index == 4 {
                        1 << 16
                    } else {
                        translate(index)
                    }
                },
                &mut buffer
            )
            .is_err()
        );
    }

    /// Every wire width matches the scalar reference packer.
    #[test]
    fn packs_every_width_like_simple_bit_storage() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for bits in 1..=16u8 {
            let mask = (1u64 << bits) - 1;
            let values: Vec<u32> = (0..4096)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state & mask) as u32
                })
                .collect();
            let mut buffer = [0; MAX_PACKED_BYTES];
            let packed = pack_u32_iter(values.iter().copied(), bits, 1u32 << bits, &mut buffer)
                .expect("every value fits the width");

            assert_eq!(packed, reference_bytes(&values, bits), "{bits} bits");
        }
    }
}
