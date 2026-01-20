use std::io::{Error, Write};

use crate::serial::WriteTo;

/// An enum that represents one of two possible types (Left or Right).
/// When serialized, it writes only the inner value without any discriminant.
/// The discriminant must be managed externally (e.g., via a separate boolean field).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Or<L, R> {
    /// The left variant.
    Left(L),
    /// The right variant.
    Right(R),
}

impl<L, R> Or<L, R> {
    /// Creates a new `Or` with the left variant.
    pub fn left(value: L) -> Self {
        Self::Left(value)
    }

    /// Creates a new `Or` with the right variant.
    pub fn right(value: R) -> Self {
        Self::Right(value)
    }

    /// Returns `true` if this is a `Left` variant.
    pub fn is_left(&self) -> bool {
        matches!(self, Self::Left(_))
    }

    /// Returns `true` if this is a `Right` variant.
    pub fn is_right(&self) -> bool {
        matches!(self, Self::Right(_))
    }
}

#[allow(missing_docs)]
impl<L: WriteTo, R: WriteTo> WriteTo for Or<L, R> {
    fn write(&self, writer: &mut impl Write) -> Result<(), Error> {
        match self {
            Self::Left(value) => value.write(writer),
            Self::Right(value) => value.write(writer),
        }
    }
}

// Note: ReadFrom cannot be implemented without external context about which variant to read.
// The discriminant must be read separately before calling this.

#[allow(missing_docs)]
impl<L, R> From<L> for Or<L, R> {
    fn from(value: L) -> Self {
        Self::Left(value)
    }
}

// Implement WriteTo for unit type so it can be used as a placeholder in Or<T, ()>
impl WriteTo for () {
    fn write(&self, _writer: &mut impl Write) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_or_write_left() {
        let or_val = Or::<i32, i64>::Left(42);
        let mut buf = Vec::new();
        or_val.write(&mut buf).expect("write failed");

        // Should write as a 32-bit integer
        assert_eq!(buf.len(), 4);
    }

    #[test]
    fn test_or_write_right() {
        let or_val = Or::<i32, i64>::Right(12345i64);
        let mut buf = Vec::new();
        or_val.write(&mut buf).expect("write failed");

        // Should write as a 64-bit integer
        assert_eq!(buf.len(), 8);
    }

    #[test]
    fn test_or_helpers() {
        let left = Or::<i32, i64>::left(10);
        assert!(left.is_left());
        assert!(!left.is_right());

        let right = Or::<i32, i64>::right(20);
        assert!(!right.is_left());
        assert!(right.is_right());
    }
}
