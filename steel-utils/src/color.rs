//! Packed color values used by Vanilla codecs and network payloads.

use std::io::{Cursor, Result, Write};

use crate::serial::{ReadFrom, WriteTo};

/// A packed color interpreted through its red, green, and blue channels.
///
/// The upper byte is preserved because Vanilla's RGB codecs do not normalize
/// integer inputs, even though RGB consumers ignore that byte.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct RgbColor(i32);

impl RgbColor {
    /// Preserves a raw Vanilla packed integer as an RGB-semantic color.
    #[must_use]
    pub const fn new(raw: i32) -> Self {
        Self(raw)
    }

    /// Returns the unchanged packed integer.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Returns the red channel.
    #[must_use]
    pub const fn red(self) -> u8 {
        (self.0 as u32 >> 16) as u8
    }

    /// Returns the green channel.
    #[must_use]
    pub const fn green(self) -> u8 {
        (self.0 as u32 >> 8) as u8
    }

    /// Returns the blue channel.
    #[must_use]
    pub const fn blue(self) -> u8 {
        self.0 as u8
    }

    /// Replaces the ignored upper byte with an alpha channel.
    #[must_use]
    pub const fn with_alpha(self, alpha: u8) -> ArgbColor {
        ArgbColor::new(((alpha as u32) << 24 | (self.0 as u32 & 0x00ff_ffff)) as i32)
    }
}

impl WriteTo for RgbColor {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.0.write(writer)
    }
}

impl ReadFrom for RgbColor {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(i32::read(data)?))
    }
}

/// A packed color interpreted through its alpha, red, green, and blue channels.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ArgbColor(i32);

impl ArgbColor {
    /// Preserves a raw Vanilla packed integer as an ARGB-semantic color.
    #[must_use]
    pub const fn new(raw: i32) -> Self {
        Self(raw)
    }

    /// Returns the unchanged packed integer.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Returns the alpha channel.
    #[must_use]
    pub const fn alpha(self) -> u8 {
        (self.0 as u32 >> 24) as u8
    }

    /// Returns the red channel.
    #[must_use]
    pub const fn red(self) -> u8 {
        (self.0 as u32 >> 16) as u8
    }

    /// Returns the green channel.
    #[must_use]
    pub const fn green(self) -> u8 {
        (self.0 as u32 >> 8) as u8
    }

    /// Returns the blue channel.
    #[must_use]
    pub const fn blue(self) -> u8 {
        self.0 as u8
    }

    /// Returns the same packed bits with RGB semantics.
    #[must_use]
    pub const fn rgb(self) -> RgbColor {
        RgbColor::new(self.0)
    }
}

impl WriteTo for ArgbColor {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.0.write(writer)
    }
}

impl ReadFrom for ArgbColor {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(i32::read(data)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_preserves_ignored_upper_byte() {
        let color = RgbColor::new(0x7f12_3456);

        assert_eq!(color.raw(), 0x7f12_3456);
        assert_eq!(
            (color.red(), color.green(), color.blue()),
            (0x12, 0x34, 0x56)
        );
        assert_eq!(color.with_alpha(0xaa).raw(), 0xaa12_3456_u32 as i32);
    }

    #[test]
    fn argb_exposes_all_channels() {
        let color = ArgbColor::new(0xaabb_ccdd_u32 as i32);

        assert_eq!(
            (color.alpha(), color.red(), color.green(), color.blue()),
            (0xaa, 0xbb, 0xcc, 0xdd)
        );
    }
}
