//! Vanilla `minecraft:mob_visibility` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryHolderSet;
use crate::entity_type::EntityType;

/// Reduces how far the targeting entity types can see the wearer.
///
/// Vanilla: `ExtraCodecs.floatRange(0.0F, 10.0F).fieldOf("visibility")`.
#[derive(Debug, Clone, PartialEq)]
pub struct MobVisibility {
    targeting_entity_types: RegistryHolderSet<EntityType>,
    visibility: f32,
}

impl MobVisibility {
    pub const MIN_VISIBILITY: f32 = 0.0;
    pub const MAX_VISIBILITY: f32 = 10.0;

    #[must_use]
    pub const fn new(
        targeting_entity_types: RegistryHolderSet<EntityType>,
        visibility: f32,
    ) -> Self {
        Self {
            targeting_entity_types,
            visibility,
        }
    }

    #[must_use]
    pub const fn targeting_entity_types(&self) -> &RegistryHolderSet<EntityType> {
        &self.targeting_entity_types
    }

    #[must_use]
    pub const fn visibility(&self) -> f32 {
        self.visibility
    }
}

impl WriteTo for MobVisibility {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.targeting_entity_types.write(writer)?;
        self.visibility.write(writer)
    }
}

impl ReadFrom for MobVisibility {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            targeting_entity_types: RegistryHolderSet::read(data)?,
            visibility: f32::read(data)?,
        })
    }
}

impl ToNbtTag for MobVisibility {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert(
            "targeting_entity_types",
            self.targeting_entity_types.to_nbt_tag(),
        );
        compound.insert("visibility", self.visibility);
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for MobVisibility {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let compound = tag.compound()?;
        let targeting_entity_types =
            RegistryHolderSet::from_nbt_tag(compound.get("targeting_entity_types")?)?;
        let visibility = compound.get("visibility")?.codec_f32()?;
        (Self::MIN_VISIBILITY..=Self::MAX_VISIBILITY)
            .contains(&visibility)
            .then(|| Self::new(targeting_entity_types, visibility))
    }
}

impl HashComponent for MobVisibility {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.clone().to_nbt_tag().hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag, ToNbtTag as _};
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::MobVisibility;
    use crate::RegistryHolderSet;
    use crate::init_vanilla_registry;
    use crate::vanilla_entities;

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<MobVisibility> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        MobVisibility::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn round_trips_both_codecs() {
        init_vanilla_registry();
        let value = MobVisibility::new(
            RegistryHolderSet::Direct(vec![&vanilla_entities::SKELETON]),
            10.0,
        );

        assert_eq!(parse(value.clone().to_nbt_tag()), Some(value.clone()));

        let mut bytes = Vec::new();
        value.write(&mut bytes).expect("component should write");
        assert_eq!(
            MobVisibility::read(&mut Cursor::new(bytes.as_slice())).expect("component should read"),
            value
        );
    }

    #[test]
    fn persistent_codec_rejects_out_of_range_visibility() {
        init_vanilla_registry();
        let mut compound = simdnbt::owned::NbtCompound::new();
        compound.insert(
            "targeting_entity_types",
            simdnbt::owned::NbtTag::String("minecraft:skeleton".into()),
        );
        compound.insert("visibility", 11.0_f32);
        assert_eq!(parse(simdnbt::owned::NbtTag::Compound(compound)), None);
    }
}
