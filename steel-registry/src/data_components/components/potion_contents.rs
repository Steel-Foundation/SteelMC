//! Vanilla `minecraft:potion_contents` item component.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{PrefixedRead as _, PrefixedWrite as _, ReadFrom, WriteTo};

use crate::RegistryReference;
use crate::mob_effect_instance::MobEffectInstance;
use crate::potion::Potion;

/// A registered base potion plus optional custom display and effect data.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PotionContents {
    potion: Option<RegistryReference<Potion>>,
    custom_color: Option<i32>,
    custom_effects: Vec<MobEffectInstance>,
    custom_name: Option<String>,
}

impl PotionContents {
    const MAX_NETWORK_STRING_LENGTH: usize = 32_767;

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            potion: None,
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        }
    }

    #[must_use]
    pub const fn new(
        potion: Option<RegistryReference<Potion>>,
        custom_color: Option<i32>,
        custom_effects: Vec<MobEffectInstance>,
        custom_name: Option<String>,
    ) -> Self {
        Self {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        }
    }

    #[must_use]
    pub const fn potion(&self) -> Option<RegistryReference<Potion>> {
        self.potion
    }

    #[must_use]
    pub const fn custom_color(&self) -> Option<i32> {
        self.custom_color
    }

    #[must_use]
    pub fn custom_effects(&self) -> &[MobEffectInstance] {
        &self.custom_effects
    }

    #[must_use]
    pub fn custom_name(&self) -> Option<&str> {
        self.custom_name.as_deref()
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(potion) = self.potion {
            compound.insert("potion", potion.to_nbt_tag());
        }
        if let Some(custom_color) = self.custom_color {
            compound.insert("custom_color", custom_color);
        }
        if !self.custom_effects.is_empty() {
            compound.insert(
                "custom_effects",
                NbtList::Compound(
                    self.custom_effects
                        .iter()
                        .map(|effect| match effect.to_nbt_tag_ref() {
                            NbtTag::Compound(compound) => compound,
                            _ => unreachable!("mob effect codec always produces a compound"),
                        })
                        .collect(),
                ),
            );
        }
        if let Some(custom_name) = &self.custom_name {
            compound.insert("custom_name", custom_name.clone());
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if tag.string().is_some() {
            return registry_reference_from_owned_nbt(tag)
                .map(|potion| Self::new(Some(potion), None, Vec::new(), None));
        }

        let compound = tag.compound()?;
        let potion = match compound.get("potion") {
            Some(tag) => Some(registry_reference_from_owned_nbt(tag)?),
            None => None,
        };
        let custom_color = match compound.get("custom_color") {
            Some(tag) => Some(tag.codec_i32()?),
            None => None,
        };
        let custom_effects = match compound.get("custom_effects") {
            Some(tag) => tag
                .list()?
                .as_nbt_tags()
                .iter()
                .map(MobEffectInstance::from_owned_nbt)
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        let custom_name = match compound.get("custom_name") {
            Some(tag) => Some(tag.string()?.to_string()),
            None => None,
        };
        Some(Self::new(potion, custom_color, custom_effects, custom_name))
    }
}

impl WriteTo for PotionContents {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.potion.is_some().write(writer)?;
        if let Some(potion) = self.potion {
            potion.write(writer)?;
        }
        self.custom_color.is_some().write(writer)?;
        if let Some(custom_color) = self.custom_color {
            custom_color.write(writer)?;
        }
        write_count(self.custom_effects.len(), writer)?;
        for effect in &self.custom_effects {
            effect.write(writer)?;
        }
        self.custom_name.is_some().write(writer)?;
        if let Some(custom_name) = &self.custom_name {
            write_network_string(custom_name, writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for PotionContents {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let potion = if bool::read(data)? {
            Some(RegistryReference::read(data)?)
        } else {
            None
        };
        let custom_color = if bool::read(data)? {
            Some(i32::read(data)?)
        } else {
            None
        };
        let count = read_count(data)?;
        let mut custom_effects = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            custom_effects.push(MobEffectInstance::read(data)?);
        }
        let custom_name = if bool::read(data)? {
            Some(read_network_string(data)?)
        } else {
            None
        };
        Ok(Self::new(potion, custom_color, custom_effects, custom_name))
    }
}

impl ToNbtTag for PotionContents {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for PotionContents {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for PotionContents {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(4);
        if let Some(potion) = &self.potion {
            push_hash_entry(&mut entries, "potion", potion);
        }
        if let Some(custom_color) = self.custom_color {
            push_hash_entry(&mut entries, "custom_color", &custom_color);
        }
        if !self.custom_effects.is_empty() {
            push_hash_entry(
                &mut entries,
                "custom_effects",
                &MobEffectList(&self.custom_effects),
            );
        }
        if let Some(custom_name) = &self.custom_name {
            push_hash_entry(&mut entries, "custom_name", custom_name);
        }
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

struct MobEffectList<'a>(&'a [MobEffectInstance]);

impl HashComponent for MobEffectList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for effect in self.0 {
            hasher.put_component_hash(effect);
        }
        hasher.end_list();
    }
}

fn registry_reference_from_owned_nbt(tag: &NbtTag) -> Option<RegistryReference<Potion>> {
    let key = tag.string()?.to_string().parse().ok()?;
    <Potion as crate::RegistryReferenceEntry>::reference_by_key(&key).map(RegistryReference::new)
}

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(count).map_err(|_| Error::other("Effect list is too large"))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count).map_err(|_| Error::other(format!("Negative effect count: {count}")))
}

fn write_network_string(value: &str, writer: &mut impl Write) -> Result<()> {
    if value.encode_utf16().count() > PotionContents::MAX_NETWORK_STRING_LENGTH
        || value.len() > PotionContents::MAX_NETWORK_STRING_LENGTH * 3
    {
        return Err(Error::other("Potion custom name exceeds the network limit"));
    }
    value.write_prefixed::<VarInt>(writer)
}

fn read_network_string(data: &mut Cursor<&[u8]>) -> Result<String> {
    let value =
        String::read_prefixed_bound::<VarInt>(data, PotionContents::MAX_NETWORK_STRING_LENGTH * 3)?;
    if value.encode_utf16().count() > PotionContents::MAX_NETWORK_STRING_LENGTH {
        return Err(Error::other("Potion custom name exceeds the network limit"));
    }
    Ok(value)
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::owned::NbtTag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::PotionContents;
    use crate::data_components::vanilla_components::POTION_CONTENTS;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt, RegistryReference, vanilla_mob_effects, vanilla_potions};

    fn parse(tag: NbtTag) -> Option<PotionContents> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        PotionContents::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn full_and_alternative_potion_codecs_round_trip() {
        init_test_registry();
        let value = PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::SWIFTNESS)),
            Some(0x12_34_56),
            vec![crate::MobEffectInstance::simple(
                vanilla_mob_effects::LUCK,
                200,
                1,
            )],
            Some("custom".to_owned()),
        );
        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        // MobEffectInstance contains Codec.BOOL fields while NbtOps represents
        // those values as bytes.
        assert_ne!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("potion contents should encode");
        assert_eq!(
            PotionContents::read(&mut Cursor::new(network.as_slice()))
                .expect("potion contents should decode"),
            value
        );

        assert_eq!(
            parse(NbtTag::String("minecraft:water".into())),
            Some(PotionContents::new(
                Some(RegistryReference::new(&vanilla_potions::WATER)),
                None,
                Vec::new(),
                None,
            ))
        );
    }

    #[test]
    fn extracted_potion_items_have_empty_contents() {
        init_test_registry();
        for name in [
            "potion",
            "splash_potion",
            "tipped_arrow",
            "lingering_potion",
        ] {
            let item = REGISTRY
                .items
                .by_key(&steel_utils::Identifier::vanilla(name.to_owned()))
                .expect("potion item should be registered");
            assert_eq!(
                item.components.get(POTION_CONTENTS),
                Some(PotionContents::empty())
            );
        }
    }
}
