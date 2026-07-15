//! Vanilla `minecraft:pot_decorations` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::items::ItemRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt, vanilla_items};

/// The back, left, right, and front decorations of a decorated pot.
#[derive(Debug, Clone, PartialEq)]
pub struct PotDecorations {
    back: Option<ItemRef>,
    left: Option<ItemRef>,
    right: Option<ItemRef>,
    front: Option<ItemRef>,
}

impl PotDecorations {
    pub const MAX_DECORATIONS: usize = 4;
    pub const EMPTY: Self = Self {
        back: None,
        left: None,
        right: None,
        front: None,
    };

    /// Constructs the component from Vanilla's ordered, at-most-four item list.
    pub fn from_ordered(items: &[ItemRef]) -> Result<Self> {
        if items.len() > Self::MAX_DECORATIONS {
            return Err(Error::other(format!(
                "Got {} pot decorations, but maximum is {}",
                items.len(),
                Self::MAX_DECORATIONS
            )));
        }
        Ok(Self {
            back: decoration(items.first().copied()),
            left: decoration(items.get(1).copied()),
            right: decoration(items.get(2).copied()),
            front: decoration(items.get(3).copied()),
        })
    }

    #[must_use]
    pub const fn back(&self) -> Option<ItemRef> {
        self.back
    }

    #[must_use]
    pub const fn left(&self) -> Option<ItemRef> {
        self.left
    }

    #[must_use]
    pub const fn right(&self) -> Option<ItemRef> {
        self.right
    }

    #[must_use]
    pub const fn front(&self) -> Option<ItemRef> {
        self.front
    }

    #[must_use]
    pub fn ordered(&self) -> [ItemRef; Self::MAX_DECORATIONS] {
        [
            self.back.unwrap_or(&vanilla_items::BRICK),
            self.left.unwrap_or(&vanilla_items::BRICK),
            self.right.unwrap_or(&vanilla_items::BRICK),
            self.front.unwrap_or(&vanilla_items::BRICK),
        ]
    }
}

fn decoration(item: Option<ItemRef>) -> Option<ItemRef> {
    item.filter(|item| *item != &*vanilla_items::BRICK)
}

impl WriteTo for PotDecorations {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(Self::MAX_DECORATIONS as i32).write(writer)?;
        for item in self.ordered() {
            let id = i32::try_from(item.id())
                .map_err(|_| Error::other(format!("Item id is too large: {}", item.id())))?;
            VarInt(id).write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for PotDecorations {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = VarInt::read(data)?.0;
        let count =
            usize::try_from(count).map_err(|_| Error::other("Negative pot decoration count"))?;
        if count > Self::MAX_DECORATIONS {
            return Err(Error::other(format!(
                "Got {count} pot decorations, but maximum is {}",
                Self::MAX_DECORATIONS
            )));
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let id = VarInt::read(data)?.0;
            let id =
                usize::try_from(id).map_err(|_| Error::other(format!("Negative item id: {id}")))?;
            let item = REGISTRY
                .items
                .by_id(id)
                .ok_or_else(|| Error::other(format!("Unknown item id: {id}")))?;
            items.push(item);
        }
        Self::from_ordered(&items)
    }
}

impl ToNbtTag for PotDecorations {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::List(NbtList::String(
            self.ordered()
                .into_iter()
                .map(|item| item.key.to_string().into())
                .collect(),
        ))
    }
}

impl FromNbtTag for PotDecorations {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let values = tag.list()?.to_owned().as_nbt_tags();
        if values.len() > Self::MAX_DECORATIONS {
            return None;
        }
        let items = values
            .iter()
            .map(|value| {
                let key = Identifier::from_str(&value.string()?.to_string()).ok()?;
                REGISTRY.items.by_key(&key)
            })
            .collect::<Option<Vec<_>>>()?;
        Self::from_ordered(&items).ok()
    }
}

impl HashComponent for PotDecorations {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for item in self.ordered() {
            hasher.put_component_hash(&item.key.to_string());
        }
        hasher.end_list();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::PotDecorations;
    use crate::data_components::vanilla_components::POT_DECORATIONS;
    use crate::test_support::init_test_registry;
    use crate::vanilla_items;

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<PotDecorations> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        PotDecorations::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn ordered_sides_round_trip_both_codecs_and_hash_as_four_items() {
        init_test_registry();
        let decorations = PotDecorations::from_ordered(&[
            &vanilla_items::ANGLER_POTTERY_SHERD,
            &vanilla_items::BRICK,
            &vanilla_items::ARCHER_POTTERY_SHERD,
        ])
        .expect("three decorations should fit");
        assert_eq!(
            decorations.back(),
            Some(&*vanilla_items::ANGLER_POTTERY_SHERD)
        );
        assert_eq!(decorations.left(), None);
        assert_eq!(decorations.front(), None);

        let nbt = decorations.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(decorations.clone()));
        assert_eq!(decorations.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        decorations
            .write(&mut network)
            .expect("decorations should encode");
        assert_eq!(
            PotDecorations::read(&mut Cursor::new(network.as_slice()))
                .expect("decorations should decode"),
            decorations
        );
    }

    #[test]
    fn extracted_decorated_pot_uses_four_bricks_as_empty_sides() {
        init_test_registry();
        assert_eq!(
            vanilla_items::DECORATED_POT.components.get(POT_DECORATIONS),
            Some(PotDecorations::EMPTY)
        );
    }

    #[test]
    fn both_codecs_reject_more_than_four_items() {
        init_test_registry();
        let items = [&*vanilla_items::BRICK; 5];
        assert!(PotDecorations::from_ordered(&items).is_err());

        let mut network = vec![5];
        network.extend([0; 5]);
        assert!(PotDecorations::read(&mut Cursor::new(network.as_slice())).is_err());
    }
}
