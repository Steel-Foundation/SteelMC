use steel_utils::{BlockPos, SectionPos};

use crate::chunk::section::Sections;

use super::{
    DATA_LAYER_EDGE, DATA_LAYER_SIZE, DataLayer, LightLayer, LightSectionRange,
    LightSectionRangeError, MAX_LIGHT_LEVEL,
};

/// Error returned when a chunk light emptiness map has the wrong length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLightEmptinessMapLengthError {
    /// Expected section count.
    pub expected: usize,
    /// Actual section count.
    pub actual: usize,
}

/// Storage representation for one present light section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightSectionData {
    /// One light value applies to the whole section.
    Homogeneous(u8),
    /// Vanilla low-nibble-first packed light values.
    Packed(Box<[u8; DATA_LAYER_SIZE]>),
}

impl LightSectionData {
    /// Creates homogeneous section data, masking to the vanilla light range.
    #[must_use]
    pub const fn homogeneous(value: u8) -> Self {
        Self::Homogeneous(value & MAX_LIGHT_LEVEL)
    }

    /// Creates packed section data from a `DataLayer`.
    #[must_use]
    pub fn from_data_layer(layer: &DataLayer) -> Self {
        if let Some(value) = layer.homogeneous_value() {
            Self::homogeneous(value)
        } else {
            Self::Packed(layer.to_bytes())
        }
    }

    /// Returns the light value at local section coordinates.
    #[must_use]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        debug_assert!(x < DATA_LAYER_EDGE);
        debug_assert!(y < DATA_LAYER_EDGE);
        debug_assert!(z < DATA_LAYER_EDGE);

        match self {
            Self::Homogeneous(value) => *value,
            Self::Packed(data) => Self::get_from_packed(data, Self::index(x, y, z)),
        }
    }

    /// Sets the light value at local section coordinates.
    pub fn set(&mut self, x: usize, y: usize, z: usize, value: u8) {
        debug_assert!(x < DATA_LAYER_EDGE);
        debug_assert!(y < DATA_LAYER_EDGE);
        debug_assert!(z < DATA_LAYER_EDGE);

        let index = Self::index(x, y, z);
        match self {
            Self::Homogeneous(default_value) => {
                let mut data = Box::new([Self::pack_filled(*default_value); DATA_LAYER_SIZE]);
                Self::set_in_packed(&mut data, index, value);
                *self = Self::Packed(data);
            }
            Self::Packed(data) => Self::set_in_packed(data, index, value),
        }
    }

    /// Fills the whole section with one value.
    pub fn fill(&mut self, value: u8) {
        *self = Self::homogeneous(value);
    }

    /// Returns true when this section is represented by one homogeneous value.
    #[must_use]
    pub const fn is_homogeneous(&self) -> bool {
        matches!(self, Self::Homogeneous(_))
    }

    /// Returns true when this section is the visible empty section state.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Homogeneous(0))
    }

    /// Returns true when every packed light value is zero.
    #[must_use]
    pub fn is_all_zero(&self) -> bool {
        match self {
            Self::Homogeneous(value) => *value == 0,
            Self::Packed(data) => data.iter().all(|value| *value == 0),
        }
    }

    /// Converts this section into vanilla `DataLayer` representation.
    #[must_use]
    pub fn to_data_layer(&self) -> DataLayer {
        match self {
            Self::Homogeneous(value) => DataLayer::filled(*value),
            Self::Packed(data) => DataLayer::from_packed_data(Box::new(**data)),
        }
    }

    /// Returns packed bytes without changing the section representation.
    #[must_use]
    pub fn to_bytes(&self) -> Box<[u8; DATA_LAYER_SIZE]> {
        match self {
            Self::Homogeneous(value) => Box::new([Self::pack_filled(*value); DATA_LAYER_SIZE]),
            Self::Packed(data) => Box::new(**data),
        }
    }

    const fn get_from_packed(data: &[u8; DATA_LAYER_SIZE], index: usize) -> u8 {
        let packed = data[index >> 1];
        packed >> ((index & 1) << 2) & MAX_LIGHT_LEVEL
    }

    const fn set_in_packed(data: &mut [u8; DATA_LAYER_SIZE], index: usize, value: u8) {
        let byte_index = index >> 1;
        let shift = (index & 1) << 2;
        let mask = !(MAX_LIGHT_LEVEL << shift);
        let value = (value & MAX_LIGHT_LEVEL) << shift;
        data[byte_index] = data[byte_index] & mask | value;
    }

    const fn index(x: usize, y: usize, z: usize) -> usize {
        y << 8 | z << 4 | x
    }

    const fn pack_filled(value: u8) -> u8 {
        let value = value & MAX_LIGHT_LEVEL;
        value | value << 4
    }
}

/// Chunk-owned section presence and data state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightSection {
    /// No light data exists for this section.
    Missing,
    /// Externally visible light data.
    Visible(LightSectionData),
    /// Internal light data omitted from vanilla packet conversion.
    Internal(LightSectionData),
}

impl LightSection {
    /// Creates a missing light section.
    #[must_use]
    pub const fn missing() -> Self {
        Self::Missing
    }

    /// Creates a visible light section.
    #[must_use]
    pub const fn visible(data: LightSectionData) -> Self {
        Self::Visible(data)
    }

    /// Creates an internal-only light section.
    #[must_use]
    pub const fn internal(data: LightSectionData) -> Self {
        Self::Internal(data)
    }

    /// Returns visible data for external packet and world-light reads.
    #[must_use]
    pub const fn visible_data(&self) -> Option<&LightSectionData> {
        match self {
            Self::Visible(data) => Some(data),
            Self::Missing | Self::Internal(_) => None,
        }
    }

    /// Returns true when any light data is present, including internal data.
    #[must_use]
    pub const fn is_present(&self) -> bool {
        matches!(self, Self::Visible(_) | Self::Internal(_))
    }
}

/// Per-layer chunk-owned light storage.
#[derive(Debug)]
pub struct ChunkLightLayerStorage {
    layer: LightLayer,
    range: LightSectionRange,
    chunk_section_count: usize,
    sections: Box<[LightSection]>,
    emptiness_map: Option<Box<[bool]>>,
}

impl ChunkLightLayerStorage {
    /// Creates missing light sections for every light section in a chunk.
    #[must_use]
    pub fn new(layer: LightLayer, range: LightSectionRange, chunk_section_count: usize) -> Self {
        let sections = (0..range.section_count())
            .map(|_| LightSection::missing())
            .collect();
        Self {
            layer,
            range,
            chunk_section_count,
            sections,
            emptiness_map: None,
        }
    }

    /// Returns this storage's light layer.
    #[must_use]
    pub const fn layer(&self) -> LightLayer {
        self.layer
    }

    /// Returns the vertical light-section range.
    #[must_use]
    pub const fn range(&self) -> LightSectionRange {
        self.range
    }

    /// Returns all chunk light sections.
    #[must_use]
    pub fn sections(&self) -> &[LightSection] {
        &self.sections
    }

    /// Returns all chunk light sections mutably.
    #[must_use]
    pub fn sections_mut(&mut self) -> &mut [LightSection] {
        &mut self.sections
    }

    /// Returns the number of real chunk sections tracked by the emptiness map.
    #[must_use]
    pub const fn chunk_section_count(&self) -> usize {
        self.chunk_section_count
    }

    /// Returns a light section for a section Y coordinate.
    #[must_use]
    pub fn section(&self, section_y: i32) -> Option<&LightSection> {
        self.range
            .section_index(section_y)
            .and_then(|index| self.sections.get(index))
    }

    /// Returns a mutable light section for a section Y coordinate.
    pub fn section_mut(&mut self, section_y: i32) -> Option<&mut LightSection> {
        let index = self.range.section_index(section_y)?;
        self.sections.get_mut(index)
    }

    /// Returns the current section emptiness map, if known.
    #[must_use]
    pub fn emptiness_map(&self) -> Option<&[bool]> {
        self.emptiness_map.as_deref()
    }

    /// Returns the known emptiness for one real chunk section Y.
    #[must_use]
    pub fn section_empty(&self, section_y: i32) -> Option<bool> {
        let index = self.chunk_section_index(section_y)?;
        self.emptiness_map
            .as_deref()
            .and_then(|emptiness_map| emptiness_map.get(index).copied())
    }

    /// Returns the highest real chunk section known to contain blocks.
    #[must_use]
    pub(crate) fn highest_non_empty_section_y(&self) -> Option<i32> {
        let emptiness_map = self.emptiness_map.as_deref()?;
        for (index, empty) in emptiness_map.iter().copied().enumerate().rev() {
            if !empty {
                return self.range.chunk_section_y(index);
            }
        }

        None
    }

    /// Replaces the section emptiness map.
    pub fn set_emptiness_map(
        &mut self,
        emptiness_map: Box<[bool]>,
    ) -> Result<(), ChunkLightEmptinessMapLengthError> {
        let actual = emptiness_map.len();
        if actual != self.chunk_section_count {
            return Err(ChunkLightEmptinessMapLengthError {
                expected: self.chunk_section_count,
                actual,
            });
        }

        self.emptiness_map = Some(emptiness_map);
        Ok(())
    }

    /// Replaces the section emptiness map from current section counters.
    pub fn refresh_emptiness_map_from_sections(
        &mut self,
        sections: &Sections,
    ) -> Result<(), ChunkLightEmptinessMapLengthError> {
        self.set_emptiness_map(sections.section_emptiness_map())
    }

    /// Updates one known section emptiness entry, returning the previous value.
    pub fn set_section_empty(&mut self, section_y: i32, empty: bool) -> Option<bool> {
        let index = self.chunk_section_index(section_y)?;
        let emptiness_map = self.emptiness_map.as_deref_mut()?;
        let previous = *emptiness_map.get(index)?;
        emptiness_map[index] = empty;
        Some(previous)
    }

    /// Applies `ScalableLux`'s loaded-sky-data normalization.
    pub(crate) fn fill_loaded_missing_sky_sections_below_data_with_zero(&mut self) {
        if self.layer != LightLayer::Sky {
            return;
        }

        let mut below_loaded_data = false;
        for section in self.sections.iter_mut().rev() {
            if section.is_present() {
                below_loaded_data = true;
                continue;
            }

            if below_loaded_data {
                *section = LightSection::visible(LightSectionData::homogeneous(0));
            }
        }
    }

    /// Returns the visible light value for one block position.
    #[must_use]
    pub fn get_light_value(&self, block_pos: BlockPos) -> u8 {
        match self.layer {
            LightLayer::Sky => self.get_sky_light_value(block_pos),
            LightLayer::Block => self.get_block_light_value(block_pos),
        }
    }

    fn get_block_light_value(&self, block_pos: BlockPos) -> u8 {
        self.visible_section_value(block_pos).unwrap_or(0)
    }

    fn get_sky_light_value(&self, block_pos: BlockPos) -> u8 {
        if let Some(value) = self.visible_section_value(block_pos) {
            return value;
        }

        let section_y = SectionPos::block_to_section_coord(block_pos.y());
        let Some(highest_non_empty_section_y) = self.highest_non_empty_section_y() else {
            return MAX_LIGHT_LEVEL;
        };
        if section_y > highest_non_empty_section_y {
            return MAX_LIGHT_LEVEL;
        }

        let local_x = section_relative_coord(block_pos.x());
        let local_z = section_relative_coord(block_pos.z());
        let mut search_section_y = section_y.saturating_add(1).max(self.range.min_section_y());
        while search_section_y < self.range.max_section_y_exclusive() {
            if let Some(section) = self.section(search_section_y)
                && let Some(data) = section.visible_data()
            {
                return data.get(local_x, 0, local_z);
            }
            search_section_y += 1;
        }

        MAX_LIGHT_LEVEL
    }

    fn visible_section_value(&self, block_pos: BlockPos) -> Option<u8> {
        let section_y = SectionPos::block_to_section_coord(block_pos.y());
        let section = self.section(section_y)?;
        let data = section.visible_data()?;

        Some(data.get(
            section_relative_coord(block_pos.x()),
            section_relative_coord(block_pos.y()),
            section_relative_coord(block_pos.z()),
        ))
    }

    fn chunk_section_index(&self, section_y: i32) -> Option<usize> {
        let index = self.range.chunk_section_index(section_y)?;
        (index < self.chunk_section_count).then_some(index)
    }
}

const fn section_relative_coord(block_coord: i32) -> usize {
    (block_coord & 15) as usize
}

/// Chunk-owned block and sky light storage.
#[derive(Debug)]
pub struct ChunkLightData {
    /// Block light sections and section emptiness metadata.
    pub block: ChunkLightLayerStorage,
    /// Sky light sections and section emptiness metadata.
    pub sky: ChunkLightLayerStorage,
}

impl ChunkLightData {
    /// Creates empty light storage for one chunk.
    pub fn new(min_y: i32, height: i32) -> Result<Self, LightSectionRangeError> {
        let range = LightSectionRange::from_world_height(min_y, height)?;
        Ok(Self {
            block: ChunkLightLayerStorage::new(
                LightLayer::Block,
                range,
                range.chunk_section_count(),
            ),
            sky: ChunkLightLayerStorage::new(LightLayer::Sky, range, range.chunk_section_count()),
        })
    }

    /// Creates storage for world heights already accepted by chunk construction.
    ///
    /// Invalid world heights are fatal because chunk-owned light arrays cannot
    /// be indexed coherently without the vanilla padded light-section range.
    ///
    /// # Panics
    ///
    /// Panics when the supplied world height cannot form a valid light-section range.
    #[must_use]
    pub fn for_valid_world_height(min_y: i32, height: i32) -> Self {
        match Self::new(min_y, height) {
            Ok(data) => data,
            Err(error) => panic!("invalid world height for chunk light data: {error:?}"),
        }
    }

    /// Refreshes both layer emptiness maps from current chunk section counters.
    pub fn refresh_emptiness_maps_from_sections(
        &mut self,
        sections: &Sections,
    ) -> Result<(), ChunkLightEmptinessMapLengthError> {
        self.block.refresh_emptiness_map_from_sections(sections)?;
        self.sky.refresh_emptiness_map_from_sections(sections)
    }

    /// Updates one real chunk section's known emptiness in both light layers.
    pub fn set_section_empty(&mut self, section_y: i32, empty: bool) -> bool {
        let block_changed = self
            .block
            .set_section_empty(section_y, empty)
            .is_some_and(|previous| previous != empty);
        let sky_changed = self
            .sky
            .set_section_empty(section_y, empty)
            .is_some_and(|previous| previous != empty);
        block_changed || sky_changed
    }

    /// Returns the visible light value for one layer at a block position.
    #[must_use]
    pub fn get_light_value(&self, layer: LightLayer, block_pos: BlockPos) -> u8 {
        match layer {
            LightLayer::Sky => self.sky.get_light_value(block_pos),
            LightLayer::Block => self.block.get_light_value(block_pos),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_light_below_range_starts_search_at_min_light_section() {
        let Ok(range) = LightSectionRange::from_world_height(0, 16) else {
            panic!("valid test height should create a light section range");
        };
        let mut storage =
            ChunkLightLayerStorage::new(LightLayer::Sky, range, range.chunk_section_count());
        let Some(section) = storage.section_mut(range.min_section_y()) else {
            panic!("test range should include its minimum light section");
        };
        *section = LightSection::visible(LightSectionData::homogeneous(7));
        let Ok(()) = storage.set_emptiness_map(vec![false; range.chunk_section_count()].into())
        else {
            panic!("test emptiness map should match the range");
        };

        assert_eq!(storage.get_light_value(BlockPos::new(0, i32::MIN, 0)), 7);
    }
}
