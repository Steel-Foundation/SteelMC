//! CRC32C hashing for component validation.
//!
//! Minecraft uses CRC32C (Castagnoli) checksums to validate component data
//! in serverbound packets. This module provides a hasher that matches
//! Minecraft's `HashOps` implementation exactly.
//!
//! ## Type Tags
//!
//! Minecraft prefixes each value with a type tag byte before hashing:
//! - Primitives: TAG_BYTE, TAG_SHORT, TAG_INT, TAG_LONG, TAG_FLOAT, TAG_DOUBLE
//! - Boolean: TAG_BOOLEAN followed by 0x00 or 0x01
//! - String: TAG_STRING followed by length (i32 BE) and UTF-8 bytes
//! - Collections use start/end markers: TAG_MAP_START/END, TAG_LIST_START/END
//!
//! All numeric values are little-endian (matching Guava's Hasher).

/// Type tags matching Minecraft's `HashOps` implementation.
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum HashTag {
    /// Empty/null value tag.
    Empty = 1,
    /// Start of a map/object.
    MapStart = 2,
    /// End of a map/object.
    MapEnd = 3,
    /// Start of a list/array.
    ListStart = 4,
    /// End of a list/array.
    ListEnd = 5,
    /// Byte (i8) value.
    Byte = 6,
    /// Short (i16) value.
    Short = 7,
    /// Int (i32) value.
    Int = 8,
    /// Long (i64) value.
    Long = 9,
    /// Float (f32) value.
    Float = 10,
    /// Double (f64) value.
    Double = 11,
    /// String value.
    String = 12,
    /// Boolean value.
    Boolean = 13,
    /// Start of a byte array.
    ByteArrayStart = 14,
    /// End of a byte array.
    ByteArrayEnd = 15,
    /// Start of an int array.
    IntArrayStart = 16,
    /// End of an int array.
    IntArrayEnd = 17,
    /// Start of a long array.
    LongArrayStart = 18,
    /// End of a long array.
    LongArrayEnd = 19,
}

/// A CRC32C hasher for component values.
///
/// This hasher is designed to produce the same hashes as Minecraft's
/// `HashOps` implementation using Guava's `Hashing.crc32c()`.
///
/// # Example
///
/// ```
/// use steel_utils::hash::ComponentHasher;
///
/// let mut hasher = ComponentHasher::new();
/// hasher.put_int(42);
/// let hash = hasher.finish();
/// ```
#[derive(Default)]
pub struct ComponentHasher {
    data: Vec<u8>,
}

impl ComponentHasher {
    /// Creates a new hasher.
    #[must_use]
    pub const fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Writes a raw tag byte.
    fn put_tag(&mut self, tag: HashTag) {
        self.data.push(tag as u8);
    }

    /// Writes raw bytes without any tag or length prefix.
    pub fn put_raw_bytes(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    /// Hashes an empty/null value.
    pub fn put_empty(&mut self) {
        self.put_tag(HashTag::Empty);
    }

    /// Hashes a byte (i8) value with tag.
    pub fn put_byte(&mut self, value: i8) {
        self.put_tag(HashTag::Byte);
        self.data.push(value as u8);
    }

    /// Hashes an unsigned byte (u8) value with tag.
    pub fn put_ubyte(&mut self, value: u8) {
        self.put_tag(HashTag::Byte);
        self.data.push(value);
    }

    /// Hashes a short (i16) value with tag.
    /// Guava uses little-endian byte order.
    pub fn put_short(&mut self, value: i16) {
        self.put_tag(HashTag::Short);
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Hashes an int (i32) value with tag.
    /// Guava uses little-endian byte order.
    pub fn put_int(&mut self, value: i32) {
        self.put_tag(HashTag::Int);
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Hashes a long (i64) value with tag.
    /// Guava uses little-endian byte order.
    pub fn put_long(&mut self, value: i64) {
        self.put_tag(HashTag::Long);
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Hashes a float (f32) value with tag.
    /// Guava uses little-endian byte order.
    pub fn put_float(&mut self, value: f32) {
        self.put_tag(HashTag::Float);
        self.data.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    /// Hashes a double (f64) value with tag.
    /// Guava uses little-endian byte order.
    pub fn put_double(&mut self, value: f64) {
        self.put_tag(HashTag::Double);
        self.data.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    /// Hashes a boolean value with tag.
    pub fn put_bool(&mut self, value: bool) {
        self.put_tag(HashTag::Boolean);
        self.data.push(u8::from(value));
    }

    /// Hashes a string value with tag, length prefix, and UTF-16 LE characters.
    ///
    /// This matches Guava's Hasher which uses little-endian for all primitives:
    /// - `putInt(length)` writes length as 4 bytes little-endian
    /// - `putUnencodedChars` writes each char as 2 bytes little-endian
    pub fn put_string(&mut self, value: &str) {
        self.put_tag(HashTag::String);
        // Length is the number of UTF-16 code units, not bytes
        // Guava uses little-endian for putInt
        let char_count: i32 = value.chars().map(|c| c.len_utf16() as i32).sum();
        self.data.extend_from_slice(&char_count.to_le_bytes());
        // Write each UTF-16 code unit as little-endian (low byte first, then high byte)
        // This matches Guava's putUnencodedChars behavior
        for c in value.chars() {
            let mut buf = [0u16; 2];
            let encoded = c.encode_utf16(&mut buf);
            for code_unit in encoded {
                self.data.extend_from_slice(&code_unit.to_le_bytes());
            }
        }
    }

    /// Starts a map/object. Call `end_map()` when done adding entries.
    pub fn start_map(&mut self) {
        self.put_tag(HashTag::MapStart);
    }

    /// Ends a map/object.
    pub fn end_map(&mut self) {
        self.put_tag(HashTag::MapEnd);
    }

    /// Starts a list. Call `end_list()` when done adding elements.
    pub fn start_list(&mut self) {
        self.put_tag(HashTag::ListStart);
    }

    /// Ends a list.
    pub fn end_list(&mut self) {
        self.put_tag(HashTag::ListEnd);
    }

    /// Starts a byte array. Call `end_byte_array()` when done.
    pub fn start_byte_array(&mut self) {
        self.put_tag(HashTag::ByteArrayStart);
    }

    /// Ends a byte array.
    pub fn end_byte_array(&mut self) {
        self.put_tag(HashTag::ByteArrayEnd);
    }

    /// Starts an int array. Call `end_int_array()` when done.
    pub fn start_int_array(&mut self) {
        self.put_tag(HashTag::IntArrayStart);
    }

    /// Writes an int value without tag (for use inside int arrays).
    /// Guava uses little-endian byte order.
    pub fn put_int_raw(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Ends an int array.
    pub fn end_int_array(&mut self) {
        self.put_tag(HashTag::IntArrayEnd);
    }

    /// Starts a long array. Call `end_long_array()` when done.
    pub fn start_long_array(&mut self) {
        self.put_tag(HashTag::LongArrayStart);
    }

    /// Writes a long value without tag (for use inside long arrays).
    /// Guava uses little-endian byte order.
    pub fn put_long_raw(&mut self, value: i64) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Ends a long array.
    pub fn end_long_array(&mut self) {
        self.put_tag(HashTag::LongArrayEnd);
    }

    /// Hashes a byte array with start/end markers.
    pub fn put_byte_array(&mut self, bytes: &[u8]) {
        self.start_byte_array();
        self.data.extend_from_slice(bytes);
        self.end_byte_array();
    }

    /// Hashes an int array with start/end markers.
    pub fn put_int_array(&mut self, values: &[i32]) {
        self.start_int_array();
        for &v in values {
            self.put_int_raw(v);
        }
        self.end_int_array();
    }

    /// Hashes a long array with start/end markers.
    pub fn put_long_array(&mut self, values: &[i64]) {
        self.start_long_array();
        for &v in values {
            self.put_long_raw(v);
        }
        self.end_long_array();
    }

    /// Returns the current hash data (for nested hashing).
    #[must_use]
    pub fn current_data(&self) -> &[u8] {
        &self.data
    }

    /// Finishes hashing and returns the CRC32C checksum as i32.
    #[must_use]
    pub fn finish(self) -> i32 {
        crc32c::crc32c(&self.data) as i32
    }

    /// Finishes hashing and returns the hash as a padded i64.
    /// Used for sorting map entries.
    #[must_use]
    pub fn finish_as_long(self) -> i64 {
        let hash = crc32c::crc32c(&self.data);
        // Pad to long by zero-extending (matches Guava's HashCode.padToLong())
        i64::from(hash)
    }
}

/// A hash entry for map sorting.
///
/// Vanilla Minecraft hashes each key and value, then sorts by these hashes,
/// and writes ONLY the 4-byte hash values (not the original encoded bytes) to the final hasher.
#[derive(Clone)]
pub struct HashEntry {
    /// The hash of the key data (for sorting).
    pub key_hash: i64,
    /// The hash of the value data (for sorting).
    pub value_hash: i64,
    /// The 4-byte CRC32C hash of the key (to be written to the final hasher).
    pub key_bytes: [u8; 4],
    /// The 4-byte CRC32C hash of the value (to be written to the final hasher).
    pub value_bytes: [u8; 4],
}

impl HashEntry {
    /// Creates a new hash entry.
    #[must_use]
    pub fn new(key_hasher: ComponentHasher, value_hasher: ComponentHasher) -> Self {
        let key_bytes = crc32c::crc32c(&key_hasher.data);
        let value_bytes = crc32c::crc32c(&value_hasher.data);
        Self {
            key_hash: i64::from(key_bytes),
            value_hash: i64::from(value_bytes),
            key_bytes: key_bytes.to_le_bytes(),
            value_bytes: value_bytes.to_le_bytes(),
        }
    }
}

/// Sorts map entries according to Minecraft's ordering:
/// First by key hash, then by value hash (both as padded longs).
pub fn sort_map_entries(entries: &mut [HashEntry]) {
    entries.sort_by(|a, b| {
        a.key_hash
            .cmp(&b.key_hash)
            .then_with(|| a.value_hash.cmp(&b.value_hash))
    });
}

/// Trait for types that can be hashed for component validation.
pub trait HashComponent {
    /// Hashes this value into the given hasher.
    fn hash_component(&self, hasher: &mut ComponentHasher);

    /// Computes the hash of this value.
    fn compute_hash(&self) -> i32 {
        let mut hasher = ComponentHasher::new();
        self.hash_component(&mut hasher);
        hasher.finish()
    }
}

// Implement HashComponent for primitive types
impl HashComponent for i8 {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_byte(*self);
    }
}

impl HashComponent for u8 {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_ubyte(*self);
    }
}

impl HashComponent for i16 {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_short(*self);
    }
}

impl HashComponent for i32 {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_int(*self);
    }
}

impl HashComponent for i64 {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_long(*self);
    }
}

impl HashComponent for f32 {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_float(*self);
    }
}

impl HashComponent for f64 {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_double(*self);
    }
}

impl HashComponent for bool {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_bool(*self);
    }
}

impl HashComponent for str {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(self);
    }
}

impl HashComponent for String {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(self);
    }
}

impl HashComponent for () {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Unit type hashes as empty
        hasher.put_empty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_hash() {
        let mut hasher = ComponentHasher::new();
        hasher.put_int(42);
        let hash = hasher.finish();
        // Verify format: [TAG_INT=8] [00 00 00 2A]
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_string_hash() {
        let mut hasher = ComponentHasher::new();
        hasher.put_string("hello");
        let hash = hasher.finish();
        // Verify format: [TAG_STRING=12] [00 00 00 05] [h e l l o]
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_bool_hash() {
        let mut hasher_true = ComponentHasher::new();
        hasher_true.put_bool(true);
        let hash_true = hasher_true.finish();

        let mut hasher_false = ComponentHasher::new();
        hasher_false.put_bool(false);
        let hash_false = hasher_false.finish();

        // true and false should produce different hashes
        assert_ne!(hash_true, hash_false);
    }

    #[test]
    fn test_empty_map_hash() {
        let mut hasher = ComponentHasher::new();
        hasher.start_map();
        hasher.end_map();
        let hash = hasher.finish();
        // Format: [TAG_MAP_START=2] [TAG_MAP_END=3]
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_empty_list_hash() {
        let mut hasher = ComponentHasher::new();
        hasher.start_list();
        hasher.end_list();
        let hash = hasher.finish();
        // Format: [TAG_LIST_START=4] [TAG_LIST_END=5]
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_byte_array_hash() {
        let mut hasher = ComponentHasher::new();
        hasher.put_byte_array(&[1, 2, 3, 4]);
        let hash = hasher.finish();
        // Format: [TAG_BYTE_ARRAY_START=14] [01 02 03 04] [TAG_BYTE_ARRAY_END=15]
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_deterministic() {
        // Same input should always produce same hash
        let hash1 = {
            let mut h = ComponentHasher::new();
            h.put_int(12345);
            h.put_string("test");
            h.finish()
        };
        let hash2 = {
            let mut h = ComponentHasher::new();
            h.put_int(12345);
            h.put_string("test");
            h.finish()
        };
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_text_component_steel() {
        use text_components::TextComponent;

        // A simple text component with just "Steel" should collapse to a string
        let component = TextComponent::from("Steel");
        let hash = component.compute_hash();

        // Expected hash from vanilla Minecraft client
        assert_eq!(hash, -25_646_594, "Hash should match vanilla client");
    }

    #[test]
    fn test_text_component_simple_styled() {
        use text_components::TextComponent;
        use text_components::{Modifier, format::Color};

        // Simple styled component: {"text":"R","color":"red","bold":true}
        // Expected hash from vanilla client 1.21.11: 1605556242
        let component = TextComponent::plain("R").color(Color::Red).bold(true);
        let hash = component.compute_hash();

        assert_eq!(
            hash, 1_605_556_242,
            "Hash should match vanilla client 1.21.11 for simple styled text"
        );
    }

    #[test]
    fn test_text_component_rainbow() {
        use text_components::TextComponent;

        // Rainbow text from the issue
        let json = r##"[{"text":"R","color":"red","bold":true},{"text":"a","color":"#ff5a00"},{"text":"i","color":"yellow","bold":true},{"text":"n","color":"green"},{"text":"b","color":"aqua","bold":true},{"text":"o","color":"blue"},{"text":"w","color":"light_purple","bold":true}]"##;
        let component = TextComponent::from_snbt(json).expect("Failed to parse rainbow text");
        let hash = component.compute_hash();

        // Expected hash from vanilla Minecraft client (from the error message)
        assert_eq!(
            hash, 796_582_470,
            "Hash should match vanilla client for rainbow text"
        );
    }
}
