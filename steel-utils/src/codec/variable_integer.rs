use std::io::{Cursor, Error, Write};

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::{
    FrontVec,
    serial::{ReadFrom, WriteTo},
};

/// A variable-length integer.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarInt(pub i32);

impl VarInt {
    /// The maximum number of bytes a `VarInt` can be.
    pub const MAX_SIZE: usize = 5;

    /// Returns the exact number of bytes this `VarInt` will write when
    /// [`WriteTo::write`] is called, assuming no error occurs.
    #[must_use]
    pub const fn written_size(val: i32) -> usize {
        match val {
            0 => 1,
            n => (31 - n.leading_zeros() as usize) / 7 + 1,
        }
    }

    /// Reads a `VarInt` from an async reader.
    ///
    /// # Errors
    /// - If the `VarInt` is too long.
    pub async fn read_async(read: &mut (impl AsyncRead + Unpin)) -> Result<i32, Error> {
        let mut val = 0;
        for i in 0..Self::MAX_SIZE {
            let byte = read
                .read_u8()
                .await
                .map_err(|err| Error::new(err.kind(), "VarInt"))?;
            val |= (i32::from(byte) & 0x7F) << (i * 7);
            if byte & 0x80 == 0 {
                return Ok(val);
            }
        }
        Err(Error::other("VarInt"))
    }

    // We could just get the written size in place,
    // but in our use case its already calculated
    /// Sets the `VarInt` in front of a `FrontVec`.
    ///
    /// # Panics
    /// - If the `VarInt` fails to write to the buffer.
    pub fn set_in_front(self, vec: &mut FrontVec, varint_size: usize) {
        // No heap allocation :)
        let mut buf = [0; Self::MAX_SIZE];
        self.write(&mut Cursor::new(&mut buf[..]))
            .expect("writing to a buffer should not fail");
        vec.set_in_front(&buf[..varint_size]);
    }
}

impl ReadFrom for VarInt {
    fn read(read: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let mut val = 0;
        for i in 0..Self::MAX_SIZE {
            let byte = u8::read(read)?;
            val |= (i32::from(byte) & 0x7F) << (i * 7);
            if byte & 0x80 == 0 {
                return Ok(Self(val));
            }
        }
        Err(Error::other("VarInt to long"))
    }
}

impl WriteTo for VarInt {
    fn write(&self, writer: &mut impl Write) -> Result<(), Error> {
        let mut val = self.0 as u32;
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

impl From<usize> for VarInt {
    fn from(value: usize) -> Self {
        Self(value as _)
    }
}

#[expect(
    clippy::cast_sign_loss,
    reason = "VarInt values used as lengths are always non-negative"
)]
impl From<VarInt> for usize {
    fn from(value: VarInt) -> usize {
        value.0 as _
    }
}

impl From<i32> for VarInt {
    fn from(value: i32) -> Self {
        Self(value as _)
    }
}

impl From<VarInt> for i32 {
    fn from(value: VarInt) -> i32 {
        value.0
    }
}

#[cfg(test)]
mod var_int_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_varint_read_write_negative() {
        let val = VarInt(-1);
        let mut buf = Vec::new();
        val.write(&mut buf).expect("write failed");

        // Expected VarInt encoding for -1 (0xFFFFFFFF)
        assert_eq!(buf, vec![0xff, 0xff, 0xff, 0xff, 0x0f]);

        let mut cursor = Cursor::new(buf.as_slice());
        let read_val = VarInt::read(&mut cursor).expect("read failed");
        assert_eq!(read_val, val);
    }
}
/// A variable-length long integer.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarLong(pub i64);

impl VarLong {
    /// The maximum number of bytes a `VarLong` can be.
    pub const MAX_SIZE: usize = 10;
}

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

impl From<i64> for VarLong {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<VarLong> for i64 {
    fn from(value: VarLong) -> i64 {
        value.0
    }
}

#[cfg(test)]
mod var_long_tests {
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
