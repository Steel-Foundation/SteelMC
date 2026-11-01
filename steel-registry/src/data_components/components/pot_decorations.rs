//! Vanilla `minecraft:pot_decorations` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::ItemStackTemplate;

/// The back, left, right, and front decorations of a decorated pot.
#[derive(Debug, Clone, PartialEq)]
pub struct PotDecorations {
    back: Option<ItemStackTemplate>,
    left: Option<ItemStackTemplate>,
    right: Option<ItemStackTemplate>,
    front: Option<ItemStackTemplate>,
}

impl PotDecorations {
    pub const EMPTY: Self = Self {
        back: None,
        left: None,
        right: None,
        front: None,
    };

    #[must_use]
    pub const fn new(
        back: Option<ItemStackTemplate>,
        left: Option<ItemStackTemplate>,
        right: Option<ItemStackTemplate>,
        front: Option<ItemStackTemplate>,
    ) -> Self {
        Self {
            back,
            left,
            right,
            front,
        }
    }

    #[must_use]
    pub const fn back(&self) -> Option<&ItemStackTemplate> {
        self.back.as_ref()
    }

    #[must_use]
    pub const fn left(&self) -> Option<&ItemStackTemplate> {
        self.left.as_ref()
    }

    #[must_use]
    pub const fn right(&self) -> Option<&ItemStackTemplate> {
        self.right.as_ref()
    }

    #[must_use]
    pub const fn front(&self) -> Option<&ItemStackTemplate> {
        self.front.as_ref()
    }
}

impl WriteTo for PotDecorations {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.back.write(writer)?;
        self.left.write(writer)?;
        self.right.write(writer)?;
        self.front.write(writer)
    }
}

impl ReadFrom for PotDecorations {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            back: Option::<ItemStackTemplate>::read(data)?,
            left: Option::<ItemStackTemplate>::read(data)?,
            right: Option::<ItemStackTemplate>::read(data)?,
            front: Option::<ItemStackTemplate>::read(data)?,
        })
    }
}

impl ToNbtTag for PotDecorations {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(back) = self.back {
            compound.insert("back", back.to_nbt_tag());
        }
        if let Some(left) = self.left {
            compound.insert("left", left.to_nbt_tag());
        }
        if let Some(right) = self.right {
            compound.insert("right", right.to_nbt_tag());
        }
        if let Some(front) = self.front {
            compound.insert("front", front.to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for PotDecorations {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            back: optional_side(compound.get("back"))?,
            left: optional_side(compound.get("left"))?,
            right: optional_side(compound.get("right"))?,
            front: optional_side(compound.get("front"))?,
        })
    }
}

#[expect(
    clippy::option_option,
    reason = "the outer option reports codec failure while the inner option represents an absent side"
)]
fn optional_side(
    tag: Option<simdnbt::borrow::NbtTag<'_, '_>>,
) -> Option<Option<ItemStackTemplate>> {
    match tag {
        Some(tag) => Some(Some(ItemStackTemplate::from_nbt_tag(tag)?)),
        None => Some(None),
    }
}

impl HashComponent for PotDecorations {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(4);
        if let Some(back) = &self.back {
            push_hash_entry(&mut entries, "back", back);
        }
        if let Some(left) = &self.left {
            push_hash_entry(&mut entries, "left", left);
        }
        if let Some(right) = &self.right {
            push_hash_entry(&mut entries, "right", right);
        }
        if let Some(front) = &self.front {
            push_hash_entry(&mut entries, "front", front);
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

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::PotDecorations;
    use crate::data_components::vanilla_components::POT_DECORATIONS;
    use crate::init_vanilla_registry;
    use crate::{ItemStackTemplate, vanilla_items};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<PotDecorations> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        PotDecorations::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn named_sides_round_trip_both_codecs_and_hash() {
        init_vanilla_registry();
        let decorations = PotDecorations::new(
            Some(ItemStackTemplate::new(&vanilla_items::ANGLER_POTTERY_SHERD)),
            None,
            Some(ItemStackTemplate::new(&vanilla_items::ARCHER_POTTERY_SHERD)),
            None,
        );
        assert!(decorations.back().is_some());
        assert!(decorations.left().is_none());

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
    fn extracted_decorated_pot_defaults_to_brick_on_every_side() {
        init_vanilla_registry();
        let decorations = vanilla_items::DECORATED_POT
            .components
            .get(POT_DECORATIONS)
            .expect("decorated pot should have a pot_decorations component");
        assert!(decorations.back().is_some());
        assert!(decorations.left().is_some());
        assert!(decorations.right().is_some());
        assert!(decorations.front().is_some());
    }
}
