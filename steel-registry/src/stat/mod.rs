pub mod custom;
mod registry;
pub mod vanilla_stat_types;

// Re-export some core types
pub use registry::{
    StatType, StatTypeEntry, StatTypeEntryRef, StatTypeRef, StatTypeRegistry, StatValueRegistry,
    StatValueRegistryData, StatValueRegistryEntry,
};

use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};

use crate::{REGISTRY, RegistryEntry, RegistryExt};
use std::io::{Cursor, Write};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Identifies a particular stat whose generic type is erased.
/// This stat can also be encoded to and decoded from the network.
///
/// This is analogous to Vanilla's `Stat<?>`.
#[derive(Copy, Clone)]
pub struct Stat {
    stat_type_entry: StatTypeEntryRef,
    value: &'static dyn StatValueRegistryEntry,
}

impl Stat {
    /// Attempts to create a new erased stat from its type and value with type safety.
    /// This function panics if the stat type provided is unregistered
    /// with the [`StatTypeRegistry`].
    pub fn new<R: RegistryExt>(stat_type: StatTypeRef<R>, value: &'static R::Entry) -> Self
    where
        R::Entry: StatValueRegistryEntry,
    {
        let stat_type_entry = stat_type.stat_type_entry_ref();
        Self {
            stat_type_entry,
            value,
        }
    }

    /// Creates a new erased stat from its erased type and value.
    pub const fn from_erased(
        stat_type_entry: StatTypeEntryRef,
        value: &'static dyn StatValueRegistryEntry,
    ) -> Self {
        Self {
            stat_type_entry,
            value,
        }
    }

    /// Gets the type-erased stat type of this stat.
    #[must_use]
    pub const fn stat_type(&self) -> StatTypeEntryRef {
        self.stat_type_entry
    }

    /// Gets the type-erased stat value of this stat.
    #[must_use]
    pub const fn stat_value(&self) -> &'static dyn StatValueRegistryEntry {
        self.value
    }

    /// Gets the key of the stat type of this stat.
    #[must_use]
    pub const fn stat_type_key(&self) -> &Identifier {
        &self.stat_type_entry.key
    }

    /// Gets the key of the stat value of this stat.
    #[must_use]
    pub fn stat_value_key(&self) -> &Identifier {
        self.value.stat_value_key()
    }

    /// Gets the registry ID of the stat type of this stat.
    #[must_use]
    pub fn stat_type_id(&self) -> usize {
        self.stat_type_entry.id()
    }

    /// Gets the registry ID of the stat value of this stat.
    #[must_use]
    pub fn stat_value_id(&self) -> usize {
        self.value.stat_value_id()
    }
}

impl Display for Stat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let stat_type_identifier = self.stat_type_entry.key();
        let value_identifier = self.value.stat_value_key();

        write!(
            f,
            "{}.{}:{}.{}",
            stat_type_identifier.namespace,
            stat_type_identifier.path,
            value_identifier.namespace,
            value_identifier.path
        )
    }
}

impl Debug for Stat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StatTypeEntry")
            .field(&self.stat_type_entry.key())
            .field(&self.value.stat_value_key())
            .finish()
    }
}

impl PartialEq for Stat {
    fn eq(&self, other: &Self) -> bool {
        self.stat_type_entry == other.stat_type_entry
            && self.value.stat_value_key() == other.value.stat_value_key()
    }
}

impl Eq for Stat {}

impl WriteTo for Stat {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        // Encode the stat type's ID, followed by the value's ID (like item ID, block ID, etc.)
        VarInt(self.stat_type_entry.id() as i32).write(writer)?;
        VarInt(self.value.stat_value_id() as i32).write(writer)?;

        Ok(())
    }
}

impl ReadFrom for Stat {
    fn read(data: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        // Decode the stat type's ID, followed by the value's ID (like item ID, block ID, etc.)
        let stat_type_id = VarInt::read(data)?.0 as usize;
        let stat_type_entry = REGISTRY.stat_types.by_id(stat_type_id).ok_or_else(|| {
            std::io::Error::other(format!("Unknown stat type ID: {stat_type_id}"))
        })?;

        let read_value_id = VarInt::read(data)?.0;
        let value_id = usize::try_from(read_value_id).map_err(|error| {
            std::io::Error::other(format!("Invalid registry ID {read_value_id}: {error}"))
        })?;

        let value = stat_type_entry.value_from_id(value_id).ok_or_else(|| {
            std::io::Error::other(format!(
                "Unknown registry ID for {}: {stat_type_id}",
                stat_type_entry.key
            ))
        })?;

        Ok(Self {
            stat_type_entry,
            value,
        })
    }
}

impl Hash for Stat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.stat_type_entry.key.hash(state);
        self.value.stat_value_key().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use crate::items::ItemRegistry;
    use crate::stat::registry::StatValueRegistry;
    use crate::stat::{Stat, StatType, vanilla_stat_types};
    use crate::{REGISTRY, RegistryEntry, vanilla_items, init_vanilla_registry};
    use std::io::Cursor;
    use steel_utils::Identifier;
    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom, WriteTo};

    static UNREGISTERED_STAT_TYPE: StatType<ItemRegistry> =
        StatType::new(Identifier::new_static("test", "unregistered"));

    #[test]
    fn network_encode_and_decode_stat() {
        init_vanilla_registry();

        // Test if stat creation succeeds or fails appropriately.
        let stat = vanilla_stat_types::ITEM_USED.get(&vanilla_items::DIAMOND);
        let should_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            UNREGISTERED_STAT_TYPE.get(&vanilla_items::DIAMOND)
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
        encoded.clear();
        VarInt(vanilla_stat_types::ITEM_BROKEN.stat_type_entry_ref().id() as i32)
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
