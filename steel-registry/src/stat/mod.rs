pub mod custom;
mod registry;
pub mod vanilla_stat_types;

use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
// Re-export some core types
pub use registry::{StatType, StatTypeEntry, StatTypeEntryRef, StatTypeRegistry};

use crate::{REGISTRY, RegistryEntry, RegistryExt};
use std::io::{Cursor, Write};
use steel_utils::codec::VarInt;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Identifies a particular stat whose generic type is erased.
/// This stat can also be encoded to and decoded from the network.
///
/// This is analogous to Vanilla's `Stat<?>`.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Stat {
    stat_type: StatTypeEntryRef,
    value_id: usize,
}

impl Stat {
    /// Attempts to create a new erased stat from its type and value with type safety.
    /// This function panics if the stat type provided is unregistered
    /// with the [`StatTypeRegistry`].
    pub fn new<R: RegistryExt>(stat_type: StatType<R>, value: &'static R::Entry) -> Self {
        let stat_type_entry = REGISTRY
            .stat_types
            .by_key(stat_type.key())
            .expect("cannot create a stat with an unregistered stat type");

        Self {
            stat_type: stat_type_entry,
            value_id: value.id(),
        }
    }
}

impl Debug for Stat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StatTypeEntry")
            .field(&self.stat_type)
            .field(&self.value_id)
            .finish()
    }
}

impl WriteTo for Stat {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        // Encode the stat type's ID, followed by the value's ID (like item ID, block ID, etc.)
        VarInt(self.stat_type.id() as i32).write(writer)?;
        VarInt(self.value_id as i32).write(writer)?;

        Ok(())
    }
}

impl ReadFrom for Stat {
    fn read(data: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        // Decode the stat type's ID, followed by the value's ID (like item ID, block ID, etc.)
        let stat_type_id = VarInt::read(data)?.0 as usize;
        let stat_type = REGISTRY.stat_types.by_id(stat_type_id).ok_or_else(|| {
            std::io::Error::other(format!("Unknown stat type ID: {stat_type_id}"))
        })?;

        let read_value_id = VarInt::read(data)?.0;
        let value_id = usize::try_from(read_value_id).map_err(|error| {
            std::io::Error::other(format!("Invalid value ID {read_value_id}: {error}"))
        })?;

        // To ensure the integrity of the `ErasedStat` object, we must validate the value.
        if !(0..stat_type.registry_len()).contains(&value_id) {
            return Err(std::io::Error::other(format!(
                "Value ID {value_id} is out of bounds of registry associated with the value"
            )));
        }

        Ok(Self {
            stat_type,
            value_id,
        })
    }
}

impl Hash for Stat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.stat_type.key.hash(state);
        self.value_id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use crate::items::ItemRegistry;
    use crate::stat::registry::StatKeyRegistry;
    use crate::stat::{Stat, StatType, vanilla_stat_types};
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryEntry, vanilla_items};
    use std::io::Cursor;
    use steel_utils::Identifier;
    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom, WriteTo};

    const UNREGISTERED_STAT_TYPE: StatType<ItemRegistry> =
        StatType::new(Identifier::new_static("test", "unregistered"), None);

    #[test]
    fn network_encode_and_decode_stat() {
        init_test_registry();

        // Test if stat creation succeeds or fails appropriately.
        let stat = Stat::new(vanilla_stat_types::ITEM_USED, &vanilla_items::DIAMOND);
        let should_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Stat::new(UNREGISTERED_STAT_TYPE, &vanilla_items::DIAMOND)
        }));
        assert!(
            should_panic.is_err(),
            "creating a stat with an unregistered stat type should have failed"
        );

        // Try to encode the stat.
        let mut encoded = Vec::new();
        stat.write(&mut encoded)
            .expect("stat should have encoded successfully");

        // Now try to decode the stat.
        let mut reader = Cursor::new(&encoded[..]);
        let decoded = Stat::read(&mut reader).expect("stat should have decoded successfully");

        assert_eq!(decoded, stat);

        // Check if we are able to decode a stat whose item ID is invalid, so it is invalid.
        let stat_type_entry = REGISTRY
            .stat_types
            .by_stat_type(vanilla_stat_types::ITEM_BROKEN)
            .expect("ITEM_BROKEN should have been registered with the stat type registry");

        encoded.clear();
        VarInt(stat_type_entry.id() as i32)
            .write(&mut encoded)
            .expect("VarInt for stat type should have encoded successfully");
        VarInt(REGISTRY.items.len() as i32)
            .write(&mut encoded)
            .expect("VarInt for item ID should have encoded successfully");

        let mut reader = Cursor::new(&encoded[..]);
        assert!(
            Stat::read(&mut reader).is_err(),
            "stat should not have decoded successfully"
        );
    }
}
