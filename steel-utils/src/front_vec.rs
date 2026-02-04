use std::{
    io::{self, Write},
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::AsyncWrite;

/// Its like a vec but with reserveable front space.
/// Its meant for our packet serialization,
/// you can just put the len of the packet in front without reallocating
/// keep in mind that calling multiple `set_in_front()` sets the data in reverse order compared to `extend_from_slice()`
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FrontVec {
    buf: Vec<u8>,
    front_space: usize,
}

impl FrontVec {
    /// Creates a new `FrontVec` with the given reserved front space and capacity.
    #[must_use]
    pub fn capacity(reserve: usize, capacity: usize) -> Self {
        let total = reserve + capacity;
        let mut buf = Vec::with_capacity(total);

        #[allow(clippy::uninit_vec)]
        unsafe {
            buf.set_len(reserve);
        };

        Self {
            buf,
            front_space: reserve,
        }
    }

    /// Creates a new `FrontVec` with the given reserved front space.
    #[must_use]
    pub fn new(reserve: usize) -> Self {
        let mut buf = Vec::with_capacity(reserve);

        #[allow(clippy::uninit_vec)]
        unsafe {
            buf.set_len(reserve);
        };

        Self {
            buf,
            front_space: reserve,
        }
    }

    /// Returns the length of the `FrontVec`.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.buf.len() - self.front_space
    }

    /// Returns whether the `FrontVec` is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        (self.buf.len() - self.front_space) == 0
    }

    /// Pushes a value to the back of the `FrontVec`.
    pub fn push(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Extends the `FrontVec` with a slice.
    pub fn extend_from_slice(&mut self, other: &[u8]) {
        self.buf.extend_from_slice(other);
    }

    /// Sets the data in the front of the `FrontVec`.
    ///
    /// # Panics
    /// - If there is not enough reserved space.
    #[track_caller]
    pub fn set_in_front(&mut self, other: &[u8]) {
        assert!(self.front_space >= other.len(), "Not enough reserved space");

        let new_start = self.front_space - other.len();
        self.buf[new_start..self.front_space].copy_from_slice(other);
        self.front_space = new_start;
    }

    /// Returns a slice of the `FrontVec`.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[self.front_space..self.buf.len()]
    }

    /// Returns a mutable slice of the `FrontVec`.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let len = self.buf.len();
        &mut self.buf[self.front_space..len]
    }
}

#[allow(missing_docs)]
impl Write for FrontVec {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[allow(missing_docs)]
impl AsyncWrite for FrontVec {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[allow(missing_docs)]
impl Deref for FrontVec {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[allow(missing_docs)]
impl DerefMut for FrontVec {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_space_reservation_and_write_safe() {
        let mut fv = FrontVec::capacity(4, 8);

        assert_eq!(fv.front_space, 4);
        assert_eq!(fv.len(), 0);
        assert_eq!(fv.as_slice(), &[] as &[u8]);

        fv.extend_from_slice(&[1, 2, 3]);
        assert_eq!(fv.as_slice(), &[1, 2, 3]);

        fv.set_in_front(&[0xAA, 0xBB]);
        assert_eq!(fv.as_slice(), &[0xAA, 0xBB, 1, 2, 3]);

        fv.set_in_front(&[0xCC]);
        assert_eq!(fv.as_slice(), &[0xCC, 0xAA, 0xBB, 1, 2, 3]);

        assert_eq!(fv.front_space, 1);
    }

    #[test]
    #[should_panic(expected = "Not enough reserved space")]
    fn set_in_front_panics_if_no_space() {
        let mut fv = FrontVec::capacity(2, 4);
        fv.set_in_front(&[1, 2, 3]);
    }
}
