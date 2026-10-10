//! Vanilla `minecraft:sign_text_front` / `minecraft:sign_text_back` item components.
//!
//! Mirrors vanilla's `SignText.CODEC`/`STREAM_CODEC`: `messages` is a fixed
//! 4-entry list, `filtered_messages` is only present when it differs from
//! `messages` (`lenientOptionalFieldOf`), `color` defaults to black and
//! `has_glowing_text` defaults to false (`optionalAlwaysPresentFieldOf`).

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::TextComponent;

use crate::DyeColor;

/// Number of text lines on one side of a sign.
pub const SIGN_LINES: usize = 4;

/// Pre-filled text for one side of a sign, carried as an item component
/// (`minecraft:sign_text_front`/`minecraft:sign_text_back`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignText {
    pub messages: [TextComponent; SIGN_LINES],
    pub filtered_messages: Option<[TextComponent; SIGN_LINES]>,
    pub color: DyeColor,
    pub has_glowing_text: bool,
}

impl SignText {
    #[must_use]
    pub const fn new(
        messages: [TextComponent; SIGN_LINES],
        filtered_messages: Option<[TextComponent; SIGN_LINES]>,
        color: DyeColor,
        has_glowing_text: bool,
    ) -> Self {
        Self {
            messages,
            filtered_messages,
            color,
            has_glowing_text,
        }
    }
}

impl WriteTo for SignText {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.messages.write(writer)?;
        self.filtered_messages.write(writer)?;
        self.color.write(writer)?;
        self.has_glowing_text.write(writer)
    }
}

impl ReadFrom for SignText {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            messages: <[TextComponent; SIGN_LINES]>::read(data)?,
            filtered_messages: Option::<[TextComponent; SIGN_LINES]>::read(data)?,
            color: DyeColor::read(data)?,
            has_glowing_text: bool::read(data)?,
        })
    }
}

fn messages_to_nbt(messages: &[TextComponent; SIGN_LINES]) -> NbtTag {
    NbtTag::List(NbtList::from(
        messages
            .iter()
            .map(TextComponent::to_codec_nbt)
            .collect::<Vec<_>>(),
    ))
}

fn messages_from_nbt(tag: simdnbt::borrow::NbtTag) -> Option<[TextComponent; SIGN_LINES]> {
    let tags = tag.list()?.to_owned().as_nbt_tags();
    let messages = tags
        .iter()
        .map(TextComponent::from_nbt)
        .collect::<Option<Vec<_>>>()?;
    <[TextComponent; SIGN_LINES]>::try_from(messages).ok()
}

impl ToNbtTag for SignText {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("messages", messages_to_nbt(&self.messages));
        // `lenientOptionalFieldOf`: only serialize `filtered_messages` when it
        // differs from `messages` (matches `filteredMessagesForSerialization`).
        if let Some(filtered) = &self.filtered_messages
            && filtered != &self.messages
        {
            compound.insert("filtered_messages", messages_to_nbt(filtered));
        }
        compound.insert("color", self.color.serialized_name());
        compound.insert("has_glowing_text", self.has_glowing_text);
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for SignText {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let messages = messages_from_nbt(compound.get("messages")?)?;
        // `SignText::load`: absent `filtered_messages` falls back to `messages`.
        let filtered_messages = compound
            .get("filtered_messages")
            .and_then(messages_from_nbt)
            .unwrap_or_else(|| messages.clone());
        let color = compound
            .get("color")
            .and_then(DyeColor::from_nbt_tag)
            .unwrap_or(DyeColor::Black);
        let has_glowing_text = compound
            .get("has_glowing_text")
            .and_then(|tag| tag.byte())
            .is_some_and(|value| value != 0);
        Some(Self {
            messages,
            filtered_messages: Some(filtered_messages),
            color,
            has_glowing_text,
        })
    }
}

impl HashComponent for SignText {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(4);
        entries.push(messages_hash_entry("messages", &self.messages));
        // The hash mirrors the persistent codec: `filtered_messages` is only
        // present in the map when it differs from `messages`.
        if let Some(filtered) = &self.filtered_messages
            && filtered != &self.messages
        {
            entries.push(messages_hash_entry("filtered_messages", filtered));
        }
        push_hash_entry(&mut entries, "color", self.color.serialized_name());
        push_hash_entry(&mut entries, "has_glowing_text", &self.has_glowing_text);

        hasher.start_map();
        for entry in entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn messages_hash_entry(key: &str, messages: &[TextComponent; SIGN_LINES]) -> HashEntry {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value_hasher.start_list();
    for message in messages {
        message.hash_component(&mut value_hasher);
    }
    value_hasher.end_list();
    HashEntry::new(key_hasher, value_hasher)
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

    use simdnbt::FromNbtTag as _;
    use simdnbt::ToNbtTag as _;
    use simdnbt::borrow::read_tag;
    use simdnbt::owned::NbtTag;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use text_components::TextComponent;

    use super::SignText;
    use crate::DyeColor;

    fn parse(tag: NbtTag) -> Option<SignText> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        SignText::from_nbt_tag(borrowed.as_tag())
    }

    fn empty_messages() -> [TextComponent; 4] {
        std::array::from_fn(|_| TextComponent::plain(""))
    }

    #[test]
    fn round_trips_both_codecs_without_filtered_messages() {
        let value = SignText::new(empty_messages(), None, DyeColor::Black, false);

        let nbt = value.clone().to_nbt_tag();
        // Absent `filtered_messages` on decode falls back to `messages`.
        let expected = SignText::new(
            empty_messages(),
            Some(empty_messages()),
            DyeColor::Black,
            false,
        );
        assert_eq!(parse(nbt), Some(expected));

        let mut network = Vec::new();
        value.write(&mut network).expect("sign text should encode");
        assert_eq!(
            SignText::read(&mut Cursor::new(network.as_slice())).expect("sign text should decode"),
            value
        );
    }

    #[test]
    fn filtered_messages_equal_to_messages_are_not_persisted() {
        let value = SignText::new(
            empty_messages(),
            Some(empty_messages()),
            DyeColor::Black,
            false,
        );

        let nbt = value.clone().to_nbt_tag();
        let NbtTag::Compound(compound) = &nbt else {
            panic!("sign text should encode as a compound");
        };
        assert!(!compound.contains("filtered_messages"));
        assert_eq!(parse(nbt), Some(value));
    }

    #[test]
    fn round_trips_with_differing_filtered_messages() {
        let value = SignText::new(
            empty_messages(),
            Some(std::array::from_fn(|_| TextComponent::plain("filtered"))),
            DyeColor::Red,
            true,
        );

        let nbt = value.clone().to_nbt_tag();
        let NbtTag::Compound(compound) = &nbt else {
            panic!("sign text should encode as a compound");
        };
        assert!(compound.contains("filtered_messages"));
        assert_eq!(parse(nbt), Some(value.clone()));

        let mut network = Vec::new();
        value.write(&mut network).expect("sign text should encode");
        assert_eq!(
            SignText::read(&mut Cursor::new(network.as_slice())).expect("sign text should decode"),
            value
        );
    }

    #[test]
    fn persistent_codec_defaults_color_and_glow_when_absent() {
        let mut compound = simdnbt::owned::NbtCompound::new();
        compound.insert(
            "messages",
            simdnbt::owned::NbtList::String(vec!["".into(), "".into(), "".into(), "".into()]),
        );
        let parsed = parse(NbtTag::Compound(compound)).expect("minimal sign text should decode");
        assert_eq!(parsed.color, DyeColor::Black);
        assert!(!parsed.has_glowing_text);
    }
}
