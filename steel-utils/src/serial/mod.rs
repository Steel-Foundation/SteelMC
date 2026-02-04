//! This module contains traits for serializing and deserializing data.
use std::io::{Cursor, Result, Write};

/// A module for reading prefixed data.
pub mod prefixed_read;
/// A module for writing prefixed data.
pub mod prefixed_write;
/// A module for reading data.
pub mod read;
/// A module for writing data.
pub mod write;

pub use write::OptionalNbt;

const DEFAULT_BOUND: usize = i16::MAX as _;

/// A trait for reading data from a cursor.
pub trait ReadFrom: Sized {
    /// Reads data from a cursor.
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self>;
}

/// A trait for writing data to a writer.
pub trait WriteTo {
    /// Writes data to a writer.
    fn write(&self, writer: &mut impl Write) -> Result<()>;
}

/// A trait for reading prefixed data from a cursor.
pub trait PrefixedRead: Sized {
    /// Reads prefixed data from a cursor with a bound.
    fn read_prefixed_bound<P: TryInto<usize> + ReadFrom>(
        data: &mut Cursor<&[u8]>,
        bound: usize,
    ) -> Result<Self>;

    /// Reads prefixed data from a cursor.
    fn read_prefixed<P: TryInto<usize> + ReadFrom>(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::read_prefixed_bound::<P>(data, DEFAULT_BOUND)
    }
}

/// A trait for writing prefixed data to a writer.
pub trait PrefixedWrite {
    /// Writes prefixed data to a writer with a bound.
    fn write_prefixed_bound<P: TryFrom<usize> + WriteTo>(
        &self,
        writer: &mut impl Write,
        bound: usize,
    ) -> Result<()>;

    /// Writes prefixed data to a writer.
    fn write_prefixed<P: TryFrom<usize> + WriteTo>(&self, writer: &mut impl Write) -> Result<()> {
        self.write_prefixed_bound::<P>(writer, DEFAULT_BOUND)
    }
}
