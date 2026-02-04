use std::io::{Cursor, Error, Write};

use crate::serial::{ReadFrom, WriteTo};

/// A variable-length unsigned integer.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarUint(pub u32);

impl VarUint {
    const MAX_SIZE: usize = 5;

    /// Returns the exact number of bytes this `VarUInt` will write when
    /// [`WriteTo::write`] is called, assuming no error occurs.
    #[must_use]
    pub const fn written_size(self) -> usize {
        (32 - self.0.leading_zeros() as usize).max(1).div_ceil(7)
    }

    /// Writes a `VarUint` to a writer.
    ///
    /// # Errors
    /// - If the writer fails to write.
    pub fn write(self, writer: &mut impl Write) -> Result<(), Error> {
        let mut val = self.0;
        loop {
            let mut byte = (val & 0x7F) as u8;
            val >>= 7;
            if val != 0 {
                byte |= 0x80;
            }
            byte.write(writer)?;
            if val == 0 {
                break;
            }
        }
        Ok(())
    }

    /// Reads a `VarUint` from a cursor.
    ///
    /// # Errors
    /// - If the `VarUint` is too long.
    pub fn read(read: &mut Cursor<&[u8]>) -> Result<u32, Error> {
        let mut val = 0;
        for i in 0..Self::MAX_SIZE {
            let byte = u8::read(read)?;
            val |= (u32::from(byte) & 0x7F) << (i * 7);
            if byte & 0x80 == 0 {
                return Ok(val);
            }
        }
        Err(Error::other("Malformed VarUint"))
    }
}
