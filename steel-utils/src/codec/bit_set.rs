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

#[allow(missing_docs)]
impl ReadFrom for BitSet {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self(Vec::read_prefixed::<VarInt>(data)?.into_boxed_slice()))
    }
}

#[allow(missing_docs)]
impl WriteTo for BitSet {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.0.write_prefixed::<VarInt>(writer)
    }
}
