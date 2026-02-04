//! UUID extension trait for Minecraft-compatible NBT serialization.
//!
//! Vanilla Minecraft stores UUIDs as int arrays with 4 elements,
//! where each i32 represents 4 bytes of the UUID in big-endian order.

use uuid::Uuid;

/// Extension trait for UUID to support Minecraft's NBT int array format.
pub trait UuidExt {
    /// Converts a UUID to an int array for NBT storage (vanilla format).
    ///
    /// The UUID is split into 4 big-endian i32 values, matching
    /// vanilla's `UUIDUtil.uuidToIntArray()`.
    fn to_int_array(&self) -> [i32; 4];

    /// Parses a UUID from an int array (vanilla NBT format).
    ///
    /// Returns `None` if the slice doesn't have exactly 4 elements.
    /// Matches vanilla's `UUIDUtil.uuidFromIntArray()`.
    fn from_int_array(arr: &[i32]) -> Option<Uuid>;
}

impl UuidExt for Uuid {
    fn to_int_array(&self) -> [i32; 4] {
        let bytes = self.as_bytes();
        [
            i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            i32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            i32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        ]
    }

    fn from_int_array(arr: &[i32]) -> Option<Uuid> {
        if arr.len() != 4 {
            return None;
        }
        let b0 = arr[0].to_be_bytes();
        let b1 = arr[1].to_be_bytes();
        let b2 = arr[2].to_be_bytes();
        let b3 = arr[3].to_be_bytes();
        Some(Uuid::from_bytes([
            b0[0], b0[1], b0[2], b0[3], b1[0], b1[1], b1[2], b1[3], b2[0], b2[1], b2[2], b2[3],
            b3[0], b3[1], b3[2], b3[3],
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_roundtrip() {
        let uuid = Uuid::new_v4();
        let arr = uuid.to_int_array();
        let recovered = Uuid::from_int_array(&arr).expect("Failed to recover UUID");
        assert_eq!(uuid, recovered);
    }

    #[test]
    fn test_uuid_from_invalid_array() {
        assert!(Uuid::from_int_array(&[1, 2, 3]).is_none());
        assert!(Uuid::from_int_array(&[1, 2, 3, 4, 5]).is_none());
        assert!(Uuid::from_int_array(&[]).is_none());
    }
}
