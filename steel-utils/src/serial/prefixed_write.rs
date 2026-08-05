use std::io::{Error, Result, Write};

use crate::serial::{PrefixedWrite, WriteTo};

impl PrefixedWrite for String {
    fn write_prefixed_bound<P: TryFrom<usize> + WriteTo>(
        &self,
        writer: &mut impl Write,
        bound: usize,
    ) -> Result<()> {
        if self.len() > bound {
            Err(Error::other("Too long"))?;
        }

        let len: P = self
            .len()
            .try_into()
            .map_err(|_| Error::other("This cant happen"))?;
        len.write(writer)?;

        writer.write_all(self.as_bytes())
    }
}

impl PrefixedWrite for str {
    fn write_prefixed_bound<P: TryFrom<usize> + WriteTo>(
        &self,
        writer: &mut impl Write,
        bound: usize,
    ) -> Result<()> {
        if self.len() > bound {
            Err(Error::other("Too long"))?;
        }

        let len: P = self
            .len()
            .try_into()
            .map_err(|_| Error::other("This cant happen"))?;
        len.write(writer)?;

        writer.write_all(self.as_bytes())
    }
}

impl<T: WriteTo> PrefixedWrite for Vec<T> {
    fn write_prefixed_bound<P: TryFrom<usize> + WriteTo>(
        &self,
        writer: &mut impl Write,
        bound: usize,
    ) -> Result<()> {
        if self.len() > bound {
            Err(Error::other("Too long"))?;
        }

        let len: P = self
            .len()
            .try_into()
            .map_err(|_| Error::other("This cant happen"))?;

        len.write(writer)?;

        for property in self {
            property.write(writer)?;
        }

        Ok(())
    }
}

impl<T: WriteTo> PrefixedWrite for [T] {
    fn write_prefixed_bound<P: TryFrom<usize> + WriteTo>(
        &self,
        writer: &mut impl Write,
        bound: usize,
    ) -> Result<()> {
        if self.len() > bound {
            Err(Error::other("Too long"))?;
        }

        let len: P = self
            .len()
            .try_into()
            .map_err(|_| Error::other("This cant happen"))?;

        len.write(writer)?;

        for property in self {
            property.write(writer)?;
        }

        Ok(())
    }
}

impl<T: WriteTo, const N: usize> PrefixedWrite for [T; N] {
    fn write_prefixed_bound<P: TryFrom<usize> + WriteTo>(
        &self,
        writer: &mut impl Write,
        bound: usize,
    ) -> Result<()> {
        if N > bound {
            Err(Error::other("Too long"))?;
        }

        P::try_from(N)
            .map_err(|_| Error::other("This cant happen"))?
            .write(writer)?;

        for i in self {
            i.write(writer)?;
        }
        Ok(())
    }
}

impl<T: PrefixedWrite> PrefixedWrite for Option<T> {
    fn write_prefixed_bound<P: TryFrom<usize> + WriteTo>(
        &self,
        writer: &mut impl Write,
        bound: usize,
    ) -> Result<()> {
        if let Some(value) = self {
            true.write(writer)?;
            value.write_prefixed_bound::<P>(writer, bound)
        } else {
            false.write(writer)
        }
    }
}
