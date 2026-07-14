//! Vanilla `minecraft:painting/variant` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::painting_variant::PaintingVariantRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Registry-owned painting variant stored on a painting item.
///
/// Vanilla's persistent codec is registry-fixed even though its stream codec
/// can carry a direct definition. Steel rejects that stream-only branch so a
/// decoded item stack always remains persistable.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintingVariantComponent {
    variant: PaintingVariantRef,
}

impl PaintingVariantComponent {
    #[must_use]
    pub const fn new(variant: PaintingVariantRef) -> Self {
        Self { variant }
    }

    #[must_use]
    pub const fn variant(&self) -> PaintingVariantRef {
        self.variant
    }
}

impl WriteTo for PaintingVariantComponent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self.variant.try_id().ok_or_else(|| {
            Error::other(format!("Unknown painting variant: {}", self.variant.key))
        })?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Painting variant id out of range: {id}")))?;
        let encoded_id = id
            .checked_add(1)
            .ok_or_else(|| Error::other("Painting variant id exceeds protocol range"))?;
        VarInt(encoded_id).write(writer)
    }
}

impl ReadFrom for PaintingVariantComponent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let encoded_id = VarInt::read(data)?.0;
        if encoded_id == 0 {
            return Err(Error::other(
                "Direct painting variant holders cannot be stored in item stacks",
            ));
        }
        let id = encoded_id
            .checked_sub(1)
            .and_then(|id| usize::try_from(id).ok())
            .ok_or_else(|| {
                Error::other(format!("Invalid painting variant holder id: {encoded_id}"))
            })?;
        REGISTRY
            .painting_variants
            .by_id(id)
            .map(Self::new)
            .ok_or_else(|| {
                Error::other(format!("Unknown painting variant holder id: {encoded_id}"))
            })
    }
}

impl ToNbtTag for PaintingVariantComponent {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::String(self.variant.key.to_string().into())
    }
}

impl FromNbtTag for PaintingVariantComponent {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let key = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        REGISTRY.painting_variants.by_key(&key).map(Self::new)
    }
}

impl HashComponent for PaintingVariantComponent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.variant.key.to_string().hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::PaintingVariantComponent;
    use crate::test_support::init_test_registry;
    use crate::vanilla_painting_variants;

    #[test]
    fn registry_reference_round_trips_both_codecs() {
        init_test_registry();
        let component = PaintingVariantComponent::new(&vanilla_painting_variants::KEBAB);

        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("variant should encode");
        assert_eq!(
            PaintingVariantComponent::read(&mut Cursor::new(network.as_slice()))
                .expect("variant should decode"),
            component
        );

        let nbt = component.clone().to_nbt_tag();
        assert_eq!(component.compute_hash(), nbt.compute_hash());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("variant NBT should parse");
        assert_eq!(
            PaintingVariantComponent::from_nbt_tag(borrowed.as_tag()),
            Some(component)
        );
    }

    #[test]
    fn direct_stream_holder_is_rejected() {
        let mut network = Vec::new();
        VarInt(0)
            .write(&mut network)
            .expect("direct holder discriminator should encode");
        assert!(PaintingVariantComponent::read(&mut Cursor::new(network.as_slice())).is_err());
    }
}
