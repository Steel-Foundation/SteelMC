use super::{DATA_LAYER_EDGE, DATA_LAYER_SIZE, MAX_LIGHT_LEVEL};

/// Error returned when packed light data has the wrong length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataLayerLengthError {
    /// Actual number of bytes provided.
    pub actual: usize,
}

/// Packed 4-bit light values for one 16x16x16 light section.
///
/// This mirrors vanilla's `DataLayer`: values are indexed as
/// `y << 8 | z << 4 | x`, with two light nibbles packed into each byte. A
/// homogeneous layer stores only a default value until bytes are needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLayer {
    data: Option<Box<[u8; DATA_LAYER_SIZE]>>,
    default_value: u8,
}

impl DataLayer {
    /// Creates an empty all-zero layer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            data: None,
            default_value: 0,
        }
    }

    /// Creates a homogeneous layer filled with `value`.
    #[must_use]
    pub const fn filled(value: u8) -> Self {
        Self {
            data: None,
            default_value: value & MAX_LIGHT_LEVEL,
        }
    }

    /// Creates a layer from packed vanilla bytes.
    pub fn from_bytes(bytes: Box<[u8]>) -> Result<Self, DataLayerLengthError> {
        let actual = bytes.len();
        let Ok(data) = bytes.try_into() else {
            return Err(DataLayerLengthError { actual });
        };

        Ok(Self::from_packed_data(data))
    }

    /// Creates a layer from already length-checked packed vanilla bytes.
    #[must_use]
    pub(crate) const fn from_packed_data(data: Box<[u8; DATA_LAYER_SIZE]>) -> Self {
        Self {
            data: Some(data),
            default_value: 0,
        }
    }

    /// Returns the light value at local section coordinates.
    #[must_use]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        debug_assert!(x < DATA_LAYER_EDGE);
        debug_assert!(y < DATA_LAYER_EDGE);
        debug_assert!(z < DATA_LAYER_EDGE);

        self.get_at_index(Self::index(x, y, z))
    }

    /// Sets the light value at local section coordinates.
    pub fn set(&mut self, x: usize, y: usize, z: usize, value: u8) {
        debug_assert!(x < DATA_LAYER_EDGE);
        debug_assert!(y < DATA_LAYER_EDGE);
        debug_assert!(z < DATA_LAYER_EDGE);

        self.set_at_index(Self::index(x, y, z), value);
    }

    /// Fills the layer with one homogeneous value.
    pub fn fill(&mut self, value: u8) {
        self.default_value = value & MAX_LIGHT_LEVEL;
        self.data = None;
    }

    /// Returns true when the layer is represented by one homogeneous value.
    #[must_use]
    pub const fn is_homogeneous(&self) -> bool {
        self.data.is_none()
    }

    /// Returns the homogeneous value when no packed data exists.
    #[must_use]
    pub const fn homogeneous_value(&self) -> Option<u8> {
        if self.data.is_none() {
            Some(self.default_value)
        } else {
            None
        }
    }

    /// Returns true when the layer is known to be filled with `value`.
    #[must_use]
    pub const fn is_filled_with(&self, value: u8) -> bool {
        self.data.is_none() && self.default_value == (value & MAX_LIGHT_LEVEL)
    }

    /// Returns true when this layer is an all-zero homogeneous layer.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_none() && self.default_value == 0
    }

    /// Returns true when all packed values are zero.
    #[must_use]
    pub fn is_all_zero(&self) -> bool {
        match &self.data {
            Some(data) => data.iter().all(|value| *value == 0),
            None => self.default_value == 0,
        }
    }

    /// Returns a deep copy of this layer.
    #[must_use]
    pub fn copy(&self) -> Self {
        self.clone()
    }

    /// Returns packed bytes without changing the layer representation.
    #[must_use]
    pub fn to_bytes(&self) -> Box<[u8; DATA_LAYER_SIZE]> {
        if let Some(data) = &self.data {
            Box::new(**data)
        } else {
            Box::new([Self::pack_filled(self.default_value); DATA_LAYER_SIZE])
        }
    }

    fn get_at_index(&self, index: usize) -> u8 {
        if let Some(data) = &self.data {
            let packed = data[Self::byte_index(index)];
            packed >> (4 * Self::nibble_index(index)) & MAX_LIGHT_LEVEL
        } else {
            self.default_value
        }
    }

    fn set_at_index(&mut self, index: usize, value: u8) {
        let data = self.data.get_or_insert_with(|| {
            Box::new([Self::pack_filled(self.default_value); DATA_LAYER_SIZE])
        });
        let byte_index = Self::byte_index(index);
        let shift = 4 * Self::nibble_index(index);
        let mask = !(MAX_LIGHT_LEVEL << shift);
        let value_to_set = (value & MAX_LIGHT_LEVEL) << shift;
        data[byte_index] = data[byte_index] & mask | value_to_set;
    }

    const fn index(x: usize, y: usize, z: usize) -> usize {
        y << 8 | z << 4 | x
    }

    const fn byte_index(index: usize) -> usize {
        index >> 1
    }

    const fn nibble_index(index: usize) -> usize {
        index & 1
    }

    const fn pack_filled(value: u8) -> u8 {
        let value = value & MAX_LIGHT_LEVEL;
        value | value << 4
    }
}

impl Default for DataLayer {
    fn default() -> Self {
        Self::new()
    }
}
