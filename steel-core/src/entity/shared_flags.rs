use bitflags::bitflags;

bitflags! {
    /// Vanilla base entity shared-flags metadata byte.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct EntitySharedFlags: u8 {
        const ON_FIRE = 1 << 0;
        const SHIFT_KEY_DOWN = 1 << 1;
        const SPRINTING = 1 << 3;
        const SWIMMING = 1 << 4;
        const INVISIBLE = 1 << 5;
        const GLOWING = 1 << 6;
        const FALL_FLYING = 1 << 7;
    }
}

impl EntitySharedFlags {
    #[must_use]
    pub(crate) const fn from_metadata_byte(byte: i8) -> Self {
        Self::from_bits_retain(byte as u8)
    }

    #[must_use]
    pub(crate) const fn metadata_byte(self) -> i8 {
        self.bits() as i8
    }
}
