use super::{
    BorrowedNbtTag, Component, ComponentData, Cursor, DowncastType, DowncastTypeKey, FromNbtTag,
    HashComponent, Identifier, OwnedNbtTag, ReadFrom, Result, ToNbtTag, WriteTo, read_tag,
};

pub type NetworkReader = fn(&mut Cursor<&[u8]>) -> Result<ComponentData>;

/// Writer function for serializing a component to network format.
pub type NetworkWriter = fn(&ComponentData, &mut Vec<u8>) -> Result<()>;

/// Reader function for deserializing a component from NBT format.
pub type NbtReader = fn(BorrowedNbtTag) -> Option<ComponentData>;

/// Writer function for serializing a component to NBT format.
pub type NbtWriter = fn(&ComponentData) -> Result<OwnedNbtTag>;

/// Function for hashing a component through its persistent codec shape.
pub(super) type ComponentHash = fn(&ComponentData) -> Result<i32>;
pub(super) type ComponentValidator = fn(&ComponentData) -> Result<()>;
pub(super) type PersistentCodecFns = (
    NbtReader,
    NbtWriter,
    ComponentHash,
    Option<ComponentValidator>,
);

/// Additional source-value validation required before persistent encoding.
pub(crate) trait ValidatePersistentComponent {
    fn validate_persistent(&self) -> Result<()>;
}

pub(super) fn hash_component<T: DowncastType + HashComponent>(data: &ComponentData) -> Result<i32> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    Ok(value.compute_hash())
}

pub(super) fn validate_component<T: DowncastType + ValidatePersistentComponent>(
    data: &ComponentData,
) -> Result<()> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.validate_persistent()
}

pub(super) fn read_typed_network<T: Component + ReadFrom>(
    cursor: &mut Cursor<&[u8]>,
) -> Result<ComponentData> {
    Ok(ComponentData::new(T::read(cursor)?))
}

pub(super) fn write_typed_network<T: DowncastType + WriteTo>(
    data: &ComponentData,
    writer: &mut Vec<u8>,
) -> Result<()> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.write(writer)
}

pub(super) fn read_typed_nbt<T: Component + FromNbtTag>(
    tag: BorrowedNbtTag,
) -> Option<ComponentData> {
    T::from_nbt_tag(tag).map(ComponentData::new)
}

pub(super) fn write_typed_nbt<T: DowncastType + ToNbtTag + Clone>(
    data: &ComponentData,
) -> Result<OwnedNbtTag> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    Ok(value.clone().to_nbt_tag())
}

struct NetworkCodecs {
    reader: NetworkReader,
    writer: NetworkWriter,
}

struct PersistentCodecs {
    reader: NbtReader,
    writer: NbtWriter,
    hash: ComponentHash,
    validator: Option<fn(&ComponentData) -> Result<()>>,
}

struct ComponentCodecs {
    expected_type_key: DowncastTypeKey,
    network: NetworkCodecs,
    persistent: Option<PersistentCodecs>,
}

/// Metadata for a registered component type.
///
/// Contains the component's key and all serialization functions needed
/// to read/write the component for network and persistent storage.
pub struct ComponentEntry {
    /// The component's identifier (e.g., "minecraft:damage")
    pub key: Identifier,
    codecs: ComponentCodecs,
    ignore_swap_animation: bool,
}

impl ComponentEntry {
    #[must_use]
    pub(super) fn implemented(
        key: Identifier,
        expected_type_key: DowncastTypeKey,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        persistent_codecs: Option<PersistentCodecFns>,
        ignore_swap_animation: bool,
    ) -> Self {
        Self {
            key,
            codecs: ComponentCodecs {
                expected_type_key,
                network: NetworkCodecs {
                    reader: network_reader,
                    writer: network_writer,
                },
                persistent: persistent_codecs.map(|(reader, writer, hash, validator)| {
                    PersistentCodecs {
                        reader,
                        writer,
                        hash,
                        validator,
                    }
                }),
            },
            ignore_swap_animation,
        }
    }

    /// Validates that a `ComponentData` value matches the concrete type for this component.
    ///
    /// Returns `true` if the data is valid for this component type, `false` otherwise.
    /// This prevents plugins from setting wrong types on vanilla components.
    #[must_use]
    pub fn validates(&self, data: &ComponentData) -> bool {
        data.type_key() == self.codecs.expected_type_key
    }

    /// Decodes this component's network value.
    pub fn read_network(&self, data: &mut Cursor<&[u8]>) -> Result<ComponentData> {
        let ComponentCodecs {
            network,
            expected_type_key,
            ..
        } = &self.codecs;
        let value = (network.reader)(data)?;
        if value.type_key() != *expected_type_key {
            return Err(std::io::Error::other(format!(
                "Network codec returned the wrong value type for {}",
                self.key
            )));
        }
        Ok(value)
    }

    /// Encodes this component's network value after validating its concrete type.
    pub fn write_network(&self, data: &ComponentData, writer: &mut Vec<u8>) -> Result<()> {
        if !self.validates(data) {
            return Err(std::io::Error::other(format!(
                "Component value type does not match {}",
                self.key
            )));
        }
        (self.codecs.network.writer)(data, writer)
    }

    /// Decodes this component's persistent NBT value.
    #[must_use]
    pub fn read_nbt(&self, tag: BorrowedNbtTag) -> Option<ComponentData> {
        let Some(persistent) = &self.codecs.persistent else {
            return None;
        };
        let value = (persistent.reader)(tag)?;
        (value.type_key() == self.codecs.expected_type_key).then_some(value)
    }

    /// Encodes this component's persistent NBT value after validating its concrete type.
    pub fn write_nbt(&self, data: &ComponentData) -> Result<OwnedNbtTag> {
        if !self.validates(data) {
            return Err(std::io::Error::other(format!(
                "Component value type does not match {}",
                self.key
            )));
        }
        let Some(persistent) = &self.codecs.persistent else {
            return Err(std::io::Error::other(format!(
                "Transient component {} has no persistent codec",
                self.key
            )));
        };
        (persistent.writer)(data)
    }

    /// Checks that a value accepted by the stream codec is also accepted by
    /// the persistent codec.
    pub fn validate_persistent_encoding(&self, data: &ComponentData) -> Result<OwnedNbtTag> {
        if let Some(validator) = self
            .codecs
            .persistent
            .as_ref()
            .and_then(|persistent| persistent.validator)
        {
            validator(data)?;
        }
        let tag = self.write_nbt(data)?;
        if self.read_nbt_owned(&tag).is_none() {
            return Err(std::io::Error::other(format!(
                "Persistent codec for component {} rejected its encoded value",
                self.key
            )));
        }
        Ok(tag)
    }

    /// Computes the vanilla `HashOps` value through this component's persistent codec.
    pub fn compute_hash(&self, data: &ComponentData) -> Result<i32> {
        if !self.validates(data) {
            return Err(std::io::Error::other(format!(
                "Component value type does not match {}",
                self.key
            )));
        }
        if !self.is_persistent() {
            return Err(std::io::Error::other(format!(
                "Transient component {} has no persistent hash codec",
                self.key
            )));
        }
        let Some(persistent) = &self.codecs.persistent else {
            return Err(std::io::Error::other(format!(
                "Transient component {} has no persistent hash codec",
                self.key
            )));
        };
        self.validate_persistent_encoding(data)?;
        (persistent.hash)(data)
    }

    /// Returns whether vanilla defines this as a persistent component.
    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        self.codecs.persistent.is_some()
    }

    /// Returns whether changes to this component are ignored for held-item swap animation.
    #[must_use]
    pub const fn ignore_swap_animation(&self) -> bool {
        self.ignore_swap_animation
    }

    /// Decodes an owned NBT value with this component's registered persistent codec.
    #[must_use]
    pub fn read_nbt_owned(&self, tag: &OwnedNbtTag) -> Option<ComponentData> {
        if !self.is_persistent() {
            return None;
        }
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        self.read_nbt(borrowed.as_tag())
    }
}

pub type ComponentEntryRef = &'static ComponentEntry;
