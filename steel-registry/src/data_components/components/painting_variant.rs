//! Vanilla `minecraft:painting/variant` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::FromNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::painting_variant::{PaintingVariant, PaintingVariantRef, PaintingVariantValue};
use crate::{REGISTRY, RegistryExt, RegistryHolder};

/// Registry-owned painting variant stored on a painting item.
///
#[derive(Debug, Clone, PartialEq)]
pub struct PaintingVariantComponent {
    variant: RegistryHolder<PaintingVariant>,
}

impl PaintingVariantComponent {
    #[must_use]
    pub const fn new(variant: PaintingVariantRef) -> Self {
        Self {
            variant: RegistryHolder::reference(variant),
        }
    }

    #[must_use]
    pub const fn direct(variant: PaintingVariantValue) -> Self {
        Self {
            variant: RegistryHolder::direct(variant),
        }
    }

    #[must_use]
    pub const fn variant(&self) -> &RegistryHolder<PaintingVariant> {
        &self.variant
    }
}

impl WriteTo for PaintingVariantComponent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.variant.write(writer)
    }
}

impl ReadFrom for PaintingVariantComponent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        RegistryHolder::read(data).map(|variant| Self { variant })
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
        self.variant.hash_component(hasher);
    }
}

impl PaintingVariantComponent {
    pub(crate) fn try_to_persistent_nbt(&self) -> Result<NbtTag> {
        let Some(variant) = self.variant.as_reference() else {
            return Err(Error::other(
                "Direct painting variant holder is not persistent",
            ));
        };
        Ok(NbtTag::String(variant.key.to_string().into()))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag as _;
    use simdnbt::borrow::read_tag;
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

        let nbt = component
            .try_to_persistent_nbt()
            .expect("reference is persistent");
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
    fn direct_stream_holder_round_trips_but_is_not_persistent() {
        use crate::painting_variant::PaintingVariantValue;
        use steel_utils::Identifier;

        let component = PaintingVariantComponent::direct(PaintingVariantValue {
            width: -1,
            height: 32,
            asset_id: Identifier::vanilla_static("custom"),
            title: None,
            author: None,
        });
        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("direct variant should encode");
        assert_eq!(
            PaintingVariantComponent::read(&mut Cursor::new(network.as_slice()))
                .expect("direct variant should decode"),
            component
        );
        assert!(component.try_to_persistent_nbt().is_err());
    }
}
