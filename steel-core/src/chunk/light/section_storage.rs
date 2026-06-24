use steel_utils::{SectionPos, codec::BitSet};

use super::LIGHT_SECTION_PADDING;

/// Error returned when a world height cannot produce a valid light-section range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightSectionRangeError {
    /// Minimum build height used to create the range.
    pub min_y: i32,
    /// Build height used to create the range.
    pub height: i32,
}

/// Inclusive/exclusive vertical range of light sections for a level.
///
/// Vanilla's `LevelLightEngine` exposes this as `getMinLightSection()`,
/// `getMaxLightSection()`, and `getLightSectionCount()`. The range is padded by
/// one section below and one section above the real chunk sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightSectionRange {
    min_section_y: i32,
    section_count: i32,
}

impl LightSectionRange {
    /// Creates the vanilla padded light-section range for a world height.
    pub const fn from_world_height(
        min_y: i32,
        height: i32,
    ) -> Result<Self, LightSectionRangeError> {
        if height <= 0 {
            return Err(LightSectionRangeError { min_y, height });
        }

        let Some(max_y) = min_y.checked_add(height - 1) else {
            return Err(LightSectionRangeError { min_y, height });
        };

        let min_chunk_section_y = SectionPos::block_to_section_coord(min_y);
        let max_chunk_section_y = SectionPos::block_to_section_coord(max_y);
        let section_count =
            max_chunk_section_y - min_chunk_section_y + 1 + LIGHT_SECTION_PADDING * 2;

        Ok(Self {
            min_section_y: min_chunk_section_y - LIGHT_SECTION_PADDING,
            section_count,
        })
    }

    /// Returns the first light section Y coordinate.
    #[must_use]
    pub const fn min_section_y(self) -> i32 {
        self.min_section_y
    }

    /// Returns the section Y coordinate one past the last light section.
    #[must_use]
    pub const fn max_section_y_exclusive(self) -> i32 {
        self.min_section_y + self.section_count
    }

    /// Returns the number of light sections in this range.
    #[must_use]
    pub const fn section_count(self) -> usize {
        self.section_count as usize
    }

    /// Returns the first real chunk section Y coordinate.
    #[must_use]
    pub const fn min_chunk_section_y(self) -> i32 {
        self.min_section_y + LIGHT_SECTION_PADDING
    }

    /// Returns the real chunk section Y coordinate one past the last section.
    #[must_use]
    pub const fn max_chunk_section_y_exclusive(self) -> i32 {
        self.max_section_y_exclusive() - LIGHT_SECTION_PADDING
    }

    /// Returns the number of real chunk sections inside this light range.
    #[must_use]
    pub const fn chunk_section_count(self) -> usize {
        (self.section_count - LIGHT_SECTION_PADDING * 2) as usize
    }

    /// Converts a packet light-section index to a section Y coordinate.
    #[must_use]
    pub const fn section_y(self, section_index: usize) -> Option<i32> {
        if section_index >= self.section_count() {
            return None;
        }

        Some(self.min_section_y + section_index as i32)
    }

    /// Converts a section Y coordinate to a packet light-section index.
    #[must_use]
    pub const fn section_index(self, section_y: i32) -> Option<usize> {
        if section_y < self.min_section_y || section_y >= self.max_section_y_exclusive() {
            return None;
        }

        Some((section_y - self.min_section_y) as usize)
    }

    /// Converts a real chunk-section index to a section Y coordinate.
    #[must_use]
    pub const fn chunk_section_y(self, section_index: usize) -> Option<i32> {
        if section_index >= self.chunk_section_count() {
            return None;
        }

        Some(self.min_chunk_section_y() + section_index as i32)
    }

    /// Converts a real chunk section Y coordinate to an emptiness-map index.
    #[must_use]
    pub const fn chunk_section_index(self, section_y: i32) -> Option<usize> {
        if section_y < self.min_chunk_section_y()
            || section_y >= self.max_chunk_section_y_exclusive()
        {
            return None;
        }

        Some((section_y - self.min_chunk_section_y()) as usize)
    }

    #[must_use]
    pub(super) fn empty_bit_set(self) -> BitSet {
        BitSet(vec![0; self.section_count().div_ceil(64)].into_boxed_slice())
    }
}
