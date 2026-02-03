use std::io::{Cursor, Error, Write};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

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

    /// Writes a `VarInt` to an async writer.
    ///
    /// # Errors
    /// - If the writer fails to write.
    pub async fn write_async(self, write: &mut (impl AsyncWrite + Unpin)) -> Result<(), Error> {
        let mut val = self.0 as u32;
        loop {
            let b: u8 = (val as u8) & 0b0111_1111;
            val >>= 7;
            write
                .write_u8(if val == 0 { b } else { b | 0b1000_0000 })
                .await?;
            if val == 0 {
                break;
            }
        }
        Ok(())
    }

    // We could just get the written size in place,
    // but in our use case its already calculated
    /// Sets the `VarInt` in front of a `FrontVec`.
    ///
    /// # Panics
    /// - If the `VarInt` fails to write to the buffer.
    pub fn set_in_front(&self, vec: &mut FrontVec, varint_size: usize) {
        // No heap allocation :)
        let mut buf = [0; Self::MAX_SIZE];
        self.write(&mut Cursor::new(&mut buf[..]))
            .expect("writing to a buffer should not fail");
        vec.set_in_front(&buf[..varint_size]);
    }
}

#[allow(missing_docs)]
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

#[allow(missing_docs)]
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

#[allow(missing_docs)]
impl From<usize> for VarInt {
    fn from(value: usize) -> Self {
        Self(value as _)
    }
}

#[allow(missing_docs)]
#[allow(clippy::cast_sign_loss)]
impl From<VarInt> for usize {
    fn from(value: VarInt) -> usize {
        value.0 as _
    }
}

#[allow(missing_docs)]
impl From<i32> for VarInt {
    fn from(value: i32) -> Self {
        Self(value as _)
    }
}

#[allow(missing_docs)]
impl From<VarInt> for i32 {
    fn from(value: VarInt) -> i32 {
        value.0
    }
}

#[cfg(test)]
mod tests {
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
