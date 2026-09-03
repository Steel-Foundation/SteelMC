use std::io::{Cursor, Result, Write};

use crate::serial::{PrefixedRead, PrefixedWrite, ReadFrom, WriteTo};

use super::VarInt;

/// A simple bit set implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitSet(pub Box<[u64]>);

impl BitSet {
    /// Sets the bit at the given index.
    pub fn set(&mut self, index: usize, value: bool) {
        let u64_index = index / 64;
        let bit_index = index % 64;

        if u64_index >= self.0.len() {
            return;
        }

        if value {
            self.0[u64_index] |= 1 << bit_index;
        } else {
            self.0[u64_index] &= !(1 << bit_index);
        }
    }
}

impl ReadFrom for BitSet {
    // Matches Java's `BitSet.valueOf(byte[])`: little-endian bytes packed into words.
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let bytes = Vec::<u8>::read_prefixed::<VarInt>(data)?;
        let mut words = vec![0_u64; bytes.len().div_ceil(8)];
        for (index, byte) in bytes.iter().enumerate() {
            words[index / 8] |= u64::from(*byte) << ((index % 8) * 8);
        }
        Ok(Self(words.into_boxed_slice()))
    }
}

impl WriteTo for BitSet {
    // Matches Java's `BitSet.toByteArray()`: little-endian bytes per word, trimmed
    // to the last non-zero byte (not word) — vanilla's `ByteBufCodecs.BIT_SET` wire
    // format is a VarInt byte-length prefix followed by those bytes.
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut bytes = Vec::with_capacity(self.0.len() * 8);
        for word in &self.0 {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let trimmed_len = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        bytes.truncate(trimmed_len);
        bytes.write_prefixed::<VarInt>(writer)
    }
}

#[cfg(test)]
mod tests {
    use crate::serial::WriteTo;

    use super::BitSet;

    #[test]
    fn write_trims_empty_bit_set_to_zero_bytes() {
        let bit_set = BitSet(vec![0].into_boxed_slice());
        let mut data = Vec::new();

        bit_set.write(&mut data).expect("bit set should encode");

        assert_eq!(data, vec![0]);
    }

    #[test]
    fn write_trims_to_last_non_zero_byte_little_endian() {
        let bit_set = BitSet(vec![5, 0, 7, 0, 0].into_boxed_slice());
        let mut data = Vec::new();

        bit_set.write(&mut data).expect("bit set should encode");

        // 3 non-empty words -> 24 little-endian bytes, trimmed to the last
        // non-zero byte: word 0 = 5 (byte 0), word 2 = 7 (byte 16).
        let expected = vec![17_u8, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7];
        assert_eq!(data, expected);
    }

    #[test]
    fn round_trips_through_read() {
        use std::io::Cursor;

        use crate::serial::ReadFrom;

        let bit_set = BitSet(vec![0x0102_0304_0506_0708, 0, 0x0000_0000_0000_00ff].into_boxed_slice());
        let mut data = Vec::new();
        bit_set.write(&mut data).expect("bit set should encode");

        let mut cursor = Cursor::new(data.as_slice());
        let read_back = BitSet::read(&mut cursor).expect("bit set should decode");

        assert_eq!(read_back.0.as_ref(), bit_set.0.as_ref());
    }
}
