#![allow(missing_docs)]
use std::io::{Cursor, Error, Read, Result};

use crate::serial::{PrefixedRead, ReadFrom};

impl PrefixedRead for String {
    fn read_prefixed_bound<P: TryInto<usize> + ReadFrom>(
        data: &mut Cursor<&[u8]>,
        bound: usize,
    ) -> Result<Self> {
        let len: usize = P::read(data)?
            .try_into()
            .map_err(|_| Error::other("Invalid Prefix"))?;

        if len > bound {
            Err(Error::other("To long"))?;
        }

        let mut buf = vec![0; len];
        data.read_exact(&mut buf)?;
        Ok(unsafe { String::from_utf8_unchecked(buf) })
    }
}

impl<T: ReadFrom> PrefixedRead for Vec<T> {
    fn read_prefixed_bound<P: TryInto<usize> + ReadFrom>(
        data: &mut Cursor<&[u8]>,
        bound: usize,
    ) -> Result<Self> {
        let len: usize = P::read(data)?
            .try_into()
            .map_err(|_| Error::other("Invalid Prefix"))?;

        if len > bound {
            Err(Error::other("To long"))?;
        }
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(T::read(data)?);
        }
        Ok(items)
    }
}

impl<T: PrefixedRead> PrefixedRead for Option<T> {
    fn read_prefixed_bound<P: TryInto<usize> + ReadFrom>(
        data: &mut Cursor<&[u8]>,
        bound: usize,
    ) -> Result<Self> {
        if bool::read(data)? {
            Ok(Some(T::read_prefixed_bound::<P>(data, bound)?))
        } else {
            Ok(None)
        }
    }
}
