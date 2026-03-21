use std::{
    array,
    io::{Cursor, Error, Read, Result},
    str::FromStr,
};

use uuid::Uuid;

use crate::{
    Identifier,
    codec::VarInt,
    serial::{PrefixedRead, ReadFrom},
};

impl ReadFrom for bool {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let byte = u8::read(data)?;
        Ok(byte == 1)
    }
}

impl ReadFrom for u8 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for u16 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for u32 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for u64 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for i8 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for i16 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for i32 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for i64 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for f32 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl ReadFrom for f64 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let mut buf = [0; size_of::<Self>()];
        data.read_exact(&mut buf)?;
        Ok(Self::from_be_bytes(buf))
    }
}

impl<T: ReadFrom> ReadFrom for Option<T> {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        if bool::read(data)? {
            Ok(Some(T::read(data)?))
        } else {
            Ok(None)
        }
    }
}

impl<T: ReadFrom, const N: usize> ReadFrom for [T; N] {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        array::try_from_fn(|_| T::read(data))
    }
}

impl ReadFrom for Uuid {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let most_significant_bits = u64::read(data)?;
        let least_significant_bits = u64::read(data)?;

        Ok(Uuid::from_u64_pair(
            most_significant_bits,
            least_significant_bits,
        ))
    }
}

impl ReadFrom for Identifier {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Identifier::from_str(&String::read_prefixed::<VarInt>(data)?).map_err(Error::other)
    }
}
