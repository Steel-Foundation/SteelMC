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
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self(Vec::read_prefixed::<VarInt>(data)?.into_boxed_slice()))
    }
}

impl WriteTo for BitSet {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let trimmed_len = self
            .0
            .iter()
            .rposition(|word| *word != 0)
            .map_or(0, |index| index + 1);
        self.0[..trimmed_len].write_prefixed::<VarInt>(writer)
    }
}

#[cfg(test)]
mod tests {
    use crate::serial::WriteTo;

    use super::BitSet;

    #[test]
    fn write_trims_empty_bit_set_to_zero_longs() {
        let bit_set = BitSet(vec![0].into_boxed_slice());
        let mut data = Vec::new();

        bit_set.write(&mut data).expect("bit set should encode");

        assert_eq!(data, vec![0]);
    }

    #[test]
    fn write_trims_only_trailing_zero_longs() {
        let bit_set = BitSet(vec![5, 0, 7, 0, 0].into_boxed_slice());
        let mut data = Vec::new();

        bit_set.write(&mut data).expect("bit set should encode");

        let mut expected = vec![3];
        expected.extend_from_slice(&5_u64.to_be_bytes());
        expected.extend_from_slice(&0_u64.to_be_bytes());
        expected.extend_from_slice(&7_u64.to_be_bytes());
        assert_eq!(data, expected);
    }
}
