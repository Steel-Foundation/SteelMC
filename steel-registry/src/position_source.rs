use std::fmt::{self, Debug, Formatter};
use std::io::{Cursor, Error, Result, Write};

use rustc_hash::FxHashMap;
use steel_utils::codec::VarInt;
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{BlockPos, Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier};

use crate::{REGISTRY, RegistryExt};

/// Concrete network payload behavior for a registered position-source type.
pub trait PositionSourceCodec:
    DowncastType + Clone + Debug + PartialEq + Send + Sync + 'static
{
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self>;
    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()>;
}

trait ErasedPositionSource: ErasedType + Debug + Send + Sync {
    fn clone_source(&self) -> Box<dyn ErasedPositionSource>;
    fn source_eq(&self, other: &dyn ErasedPositionSource) -> bool;
}

impl<T: PositionSourceCodec> ErasedPositionSource for T {
    fn clone_source(&self) -> Box<dyn ErasedPositionSource> {
        Box::new(self.clone())
    }

    fn source_eq(&self, other: &dyn ErasedPositionSource) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }
}

type NetworkReader = fn(&mut Cursor<&[u8]>) -> Result<Box<dyn ErasedPositionSource>>;
type NetworkWriter = fn(&dyn ErasedPositionSource, &mut Vec<u8>) -> Result<()>;

/// A registered position-source discriminator and its network codec.
pub struct PositionSourceType {
    pub key: Identifier,
    expected_type_key: DowncastTypeKey,
    network_reader: NetworkReader,
    network_writer: NetworkWriter,
}

impl PositionSourceType {
    #[must_use]
    pub const fn of<T: PositionSourceCodec>(key: Identifier) -> Self {
        Self {
            key,
            expected_type_key: T::TYPE_KEY,
            network_reader: read_network::<T>,
            network_writer: write_network::<T>,
        }
    }
}

impl Debug for PositionSourceType {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PositionSourceType")
            .field("key", &self.key)
            .field("expected_type_key", &self.expected_type_key)
            .finish_non_exhaustive()
    }
}

pub type PositionSourceTypeRef = &'static PositionSourceType;

/// A registry-dispatched position source suitable for a particle network payload.
pub struct PositionSource {
    source_type: PositionSourceTypeRef,
    value: Box<dyn ErasedPositionSource>,
}

impl PositionSource {
    #[must_use]
    pub fn new<T: PositionSourceCodec>(source_type: PositionSourceTypeRef, value: T) -> Self {
        assert_eq!(
            source_type.expected_type_key,
            T::TYPE_KEY,
            "position source value does not match its registered type"
        );
        Self {
            source_type,
            value: Box::new(value),
        }
    }

    #[must_use]
    pub const fn source_type(&self) -> PositionSourceTypeRef {
        self.source_type
    }

    #[must_use]
    pub fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }
}

impl Clone for PositionSource {
    fn clone(&self) -> Self {
        Self {
            source_type: self.source_type,
            value: self.value.clone_source(),
        }
    }
}

impl Debug for PositionSource {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PositionSource")
            .field("source_type", &self.source_type.key)
            .field("value", &self.value)
            .finish()
    }
}

impl PartialEq for PositionSource {
    fn eq(&self, other: &Self) -> bool {
        self.source_type.key == other.source_type.key && self.value.source_eq(other.value.as_ref())
    }
}

impl WriteTo for PositionSource {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let (id, source_type) = REGISTRY
            .position_source_types
            .registered_entry_with_id(self.source_type)
            .ok_or_else(|| {
                Error::other(format!(
                    "Position source type is not the registered value for key: {}",
                    self.source_type.key
                ))
            })?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Position source type id out of range: {id}")))?;
        VarInt(id).write(writer)?;

        let mut payload = Vec::new();
        (source_type.network_writer)(self.value.as_ref(), &mut payload)?;
        writer.write_all(&payload)
    }
}

impl ReadFrom for PositionSource {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative position source type id: {id}")))?;
        let source_type = REGISTRY
            .position_source_types
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown position source type id: {id}")))?;
        let value = (source_type.network_reader)(data)?;
        Ok(Self { source_type, value })
    }
}

pub struct PositionSourceTypeRegistry {
    types_by_id: Vec<PositionSourceTypeRef>,
    types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl PositionSourceTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types_by_id: Vec::new(),
            types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "network dispatch requires exact registered position source type identity"
    )]
    fn registered_entry_with_id(
        &self,
        entry: PositionSourceTypeRef,
    ) -> Option<(usize, PositionSourceTypeRef)> {
        let id = self.types_by_key.get(&entry.key).copied()?;
        let registered = self.types_by_id.get(id).copied()?;
        std::ptr::eq(registered, entry).then_some((id, registered))
    }
}

crate::impl_standard_methods!(
    PositionSourceTypeRegistry,
    PositionSourceTypeRef,
    types_by_id,
    types_by_key,
    allows_registering,
    "Cannot register duplicate position source type key: {}"
);
crate::impl_registry!(
    PositionSourceTypeRegistry,
    PositionSourceType,
    types_by_id,
    types_by_key,
    position_source_types
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPositionSource {
    pos: BlockPos,
}

impl BlockPositionSource {
    #[must_use]
    pub const fn new(pos: BlockPos) -> Self {
        Self { pos }
    }

    #[must_use]
    pub const fn pos(&self) -> BlockPos {
        self.pos
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete position-source payload.
unsafe impl DowncastType for BlockPositionSource {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:position_source/block");
}

impl PositionSourceCodec for BlockPositionSource {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(BlockPos::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.pos.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityPositionSource {
    entity_id: i32,
    y_offset: f32,
}

impl EntityPositionSource {
    #[must_use]
    pub const fn new(entity_id: i32, y_offset: f32) -> Self {
        Self {
            entity_id,
            y_offset,
        }
    }

    #[must_use]
    pub const fn entity_id(&self) -> i32 {
        self.entity_id
    }

    #[must_use]
    pub const fn y_offset(&self) -> f32 {
        self.y_offset
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete position-source payload.
unsafe impl DowncastType for EntityPositionSource {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:position_source/entity");
}

impl PositionSourceCodec for EntityPositionSource {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(VarInt::read(data)?.0, f32::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        VarInt(self.entity_id).write(writer)?;
        self.y_offset.write(writer)
    }
}

fn read_network<T: PositionSourceCodec>(
    data: &mut Cursor<&[u8]>,
) -> Result<Box<dyn ErasedPositionSource>> {
    Ok(Box::new(T::read_network(data)?))
}

fn write_network<T: PositionSourceCodec>(
    value: &dyn ErasedPositionSource,
    writer: &mut Vec<u8>,
) -> Result<()> {
    let value = value.downcast_ref::<T>().ok_or_else(|| {
        Error::other(format!(
            "Position source payload does not match {}",
            T::TYPE_KEY
        ))
    })?;
    value.write_network(writer)
}

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;
    use steel_utils::serial::WriteTo;

    use crate::{test_support::init_test_registry, vanilla_position_source_types};

    use super::{
        EntityPositionSource, PositionSource, PositionSourceType, PositionSourceTypeRegistry,
    };

    static FORGED_BLOCK_SOURCE: PositionSourceType =
        PositionSourceType::of::<EntityPositionSource>(Identifier::vanilla_static("block"));

    #[test]
    fn position_source_write_rejects_noncanonical_same_key_codec() {
        init_test_registry();

        let source =
            PositionSource::new(&FORGED_BLOCK_SOURCE, EntityPositionSource::new(1234, 1.5));
        let mut encoded = Vec::new();
        let result = source.write(&mut encoded);

        assert!(result.is_err());
        assert!(encoded.is_empty());
    }

    #[test]
    #[should_panic(expected = "Cannot register duplicate position source type key")]
    fn position_source_type_registry_rejects_duplicate_keys() {
        let mut registry = PositionSourceTypeRegistry::new();
        registry.register(&vanilla_position_source_types::BLOCK);
        registry.register(&FORGED_BLOCK_SOURCE);
    }
}
