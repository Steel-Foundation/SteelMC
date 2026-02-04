use std::io::{Cursor, Error, Write};

use crate::serial::{ReadFrom, WriteTo};

/// A variable-length long integer.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarLong(pub i64);

impl VarLong {
    /// The maximum number of bytes a `VarLong` can be.
    pub const MAX_SIZE: usize = 10;
}

#[allow(missing_docs)]
impl ReadFrom for VarLong {
    fn read(read: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let mut val = 0i64;
        for i in 0..Self::MAX_SIZE {
            let byte = u8::read(read)?;
            val |= (i64::from(byte) & 0x7F) << (i * 7);
            if byte & 0x80 == 0 {
                return Ok(Self(val));
            }
        }
        Err(Error::other("VarLong too long"))
    }
}

#[allow(missing_docs)]
impl WriteTo for VarLong {
    fn write(&self, writer: &mut impl Write) -> Result<(), Error> {
        let mut val = self.0 as u64;
        loop {
            let b: u8 = val as u8 & 0x7F;
            val >>= 7;
            if val == 0 {
                b.write(writer)?;
                break;
            }
            (b | 0x80).write(writer)?;
        }
        Ok(())
    }
}

#[allow(missing_docs)]
impl From<i64> for VarLong {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

#[allow(missing_docs)]
impl From<VarLong> for i64 {
    fn from(value: VarLong) -> i64 {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_varlong_read_write() {
        let test_values = vec![
            0i64,
            1i64,
            127i64,
            128i64,
            255i64,
            2_147_483_647_i64,
            9_223_372_036_854_775_807_i64,
            -1i64,
            -2_147_483_648_i64,
        ];

        for val in test_values {
            let var_long = VarLong(val);
            let mut buf = Vec::new();
            var_long.write(&mut buf).expect("write failed");

            let mut cursor = Cursor::new(buf.as_slice());
            let read_val = VarLong::read(&mut cursor).expect("read failed");
            assert_eq!(read_val, var_long, "Failed for value {val}");
        }
    }
}
