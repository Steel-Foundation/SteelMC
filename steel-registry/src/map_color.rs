//! Vanilla map palette values used by saved map pixels.

const BRIGHTNESS_BITS: u32 = 2;
const BRIGHTNESS_MASK: u8 = (1 << BRIGHTNESS_BITS) - 1;

/// A map palette entry. Vanilla's palette is static rather than a dynamic registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapColor {
    /// Index in the map palette.
    pub id: u8,
    /// Unshaded RGB value.
    pub color: u32,
}

/// A brightness variant packed alongside a palette index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapBrightness {
    /// Two-bit brightness index.
    pub id: u8,
    /// RGB channel multiplier, divided by 255.
    pub modifier: u8,
}

impl MapColor {
    /// Encodes a palette entry and brightness for a saved map pixel.
    #[must_use]
    pub const fn packed_id(self, brightness: MapBrightness) -> u8 {
        (self.id << BRIGHTNESS_BITS) | (brightness.id & BRIGHTNESS_MASK)
    }
}
