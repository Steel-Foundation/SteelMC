/// One of Vanilla's fixed material colors used by filled-map pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapColor(u8);

impl MapColor {
    pub const NONE: Self = Self(0);
    pub const DIRT: Self = Self(10);
    pub const STONE: Self = Self(11);
    pub const WATER: Self = Self(12);
    pub const PLANT: Self = Self(7);
    pub const COLOR_YELLOW: Self = Self(18);

    #[must_use]
    pub const fn new(id: u8) -> Self {
        assert!(id <= 63, "map color ID must be between 0 and 63");
        Self(id)
    }

    #[must_use]
    pub const fn id(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn packed_id(self, brightness: MapBrightness) -> u8 {
        (self.0 << 2) | brightness as u8
    }
}

/// Vanilla's four brightness modifiers encoded in the low map-pixel bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MapBrightness {
    Low = 0,
    Normal = 1,
    High = 2,
    Lowest = 3,
}
