//! Vanilla `minecraft:recipes` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtList, NbtTag, read_tag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::vanilla_nbt_heap_size;
use steel_utils::serial::{ReadFrom, WriteTo};

const DEFAULT_NBT_QUOTA: u64 = 2_097_152;

/// Ordered recipe resource keys carried by knowledge books.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recipes {
    keys: Vec<Identifier>,
}

impl Recipes {
    #[must_use]
    pub const fn empty() -> Self {
        Self { keys: Vec::new() }
    }

    #[must_use]
    pub const fn new(keys: Vec<Identifier>) -> Self {
        Self { keys }
    }

    #[must_use]
    pub fn keys(&self) -> &[Identifier] {
        &self.keys
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        if self.keys.is_empty() {
            return NbtTag::List(NbtList::Empty);
        }
        NbtTag::List(NbtList::String(
            self.keys.iter().map(|key| key.to_string().into()).collect(),
        ))
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let list = tag.list()?;
        let values = list.as_nbt_tags();
        if values.is_empty() {
            return Some(Self::empty());
        }
        let keys = values
            .iter()
            .map(|key| Identifier::from_str(&key.string()?.to_string()).ok())
            .collect::<Option<Vec<_>>>()?;
        Some(Self::new(keys))
    }
}

impl WriteTo for Recipes {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut encoded = Vec::new();
        self.to_nbt_tag_ref().write(&mut encoded);
        writer.write_all(&encoded)
    }
}

impl ReadFrom for Recipes {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let tag =
            read_tag(data).map_err(|error| Error::other(format!("Invalid NBT: {error:?}")))?;
        let Some(heap_size) = vanilla_nbt_heap_size(&tag) else {
            return Err(Error::other("NBT contains malformed modified UTF-8"));
        };
        if heap_size > DEFAULT_NBT_QUOTA {
            return Err(Error::other(format!(
                "NBT exceeds Vanilla's {DEFAULT_NBT_QUOTA}-byte heap quota"
            )));
        }
        Self::from_owned_nbt(&tag)
            .ok_or_else(|| Error::other("Recipes network value is not a list of recipe keys"))
    }
}

impl ToNbtTag for Recipes {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for Recipes {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for Recipes {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for key in &self.keys {
            hasher.put_component_hash(key);
        }
        hasher.end_list();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtList, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::Recipes;
    use crate::data_components::vanilla_components::RECIPES;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse(tag: NbtTag) -> Option<Recipes> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        Recipes::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn recipe_keys_round_trip_persistent_and_derived_network_codecs() {
        let recipes = Recipes::new(vec![
            Identifier::vanilla_static("oak_planks"),
            Identifier::new_static("steel", "example"),
        ]);
        let nbt = recipes.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(recipes.clone()));
        assert_eq!(recipes.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        recipes.write(&mut network).expect("recipes should encode");
        assert_eq!(
            Recipes::read(&mut Cursor::new(network.as_slice())).expect("recipes should decode"),
            recipes
        );
    }

    #[test]
    fn empty_and_abbreviated_recipe_key_lists_match_identifier_codec_rules() {
        let empty = Recipes::empty();
        assert_eq!(empty.clone().to_nbt_tag(), NbtTag::List(NbtList::Empty));
        assert_eq!(
            parse(NbtTag::List(NbtList::String(Vec::new()))),
            Some(empty)
        );
        assert_eq!(
            parse(NbtTag::List(NbtList::String(vec!["stick".into()])))
                .expect("abbreviated key should decode")
                .keys(),
            &[Identifier::vanilla_static("stick")]
        );
    }

    #[test]
    fn recipe_key_codec_does_not_require_a_registered_recipe() {
        let unknown = Identifier::new_static("steel", "not_registered");
        let recipes = Recipes::new(vec![unknown.clone()]);
        assert_eq!(
            parse(recipes.clone().to_nbt_tag())
                .expect("resource keys are registry-independent")
                .keys(),
            &[unknown]
        );
    }

    #[test]
    fn extracted_knowledge_book_keeps_its_empty_recipe_list() {
        init_test_registry();
        let knowledge_book = REGISTRY
            .items
            .by_key(&Identifier::vanilla_static("knowledge_book"))
            .expect("knowledge book should be registered");
        assert_eq!(
            knowledge_book.components.get(RECIPES),
            Some(Recipes::empty())
        );
    }
}
