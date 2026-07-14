//! Vanilla `minecraft:lore` item component.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{Cursor, Result as IoResult, Write};

use simdnbt::owned::{NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{PrefixedRead, PrefixedWrite, ReadFrom, WriteTo};
use text_components::TextComponent;
use text_components::format::{Color, Format};

/// Error returned when lore exceeds vanilla's 256-line limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemLoreTooLong {
    pub lines: usize,
}

impl Display for ItemLoreTooLong {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Got {} lines, but maximum is {}",
            self.lines,
            ItemLore::MAX_LINES
        )
    }
}

impl Error for ItemLoreTooLong {}

/// Item lore plus the vanilla display-styled projection of each line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLore {
    lines: Vec<TextComponent>,
    styled_lines: Vec<TextComponent>,
}

impl ItemLore {
    pub const MAX_LINES: usize = 256;

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            lines: Vec::new(),
            styled_lines: Vec::new(),
        }
    }

    pub fn new(lines: Vec<TextComponent>) -> Result<Self, ItemLoreTooLong> {
        if lines.len() > Self::MAX_LINES {
            return Err(ItemLoreTooLong { lines: lines.len() });
        }
        let styled_lines = lines.iter().map(styled_line).collect();
        Ok(Self {
            lines,
            styled_lines,
        })
    }

    #[must_use]
    pub fn lines(&self) -> &[TextComponent] {
        &self.lines
    }

    #[must_use]
    pub fn styled_lines(&self) -> &[TextComponent] {
        &self.styled_lines
    }

    pub fn with_line_added(&self, line: TextComponent) -> Result<Self, ItemLoreTooLong> {
        let mut lines = self.lines.clone();
        lines.push(line);
        Self::new(lines)
    }
}

impl Default for ItemLore {
    fn default() -> Self {
        Self::empty()
    }
}

fn styled_line(line: &TextComponent) -> TextComponent {
    let lore_style = Format::new().color(Color::DarkPurple).italic(true);
    let mut styled = line.clone();
    styled.format = styled.format.mix(&lore_style);
    styled
}

impl WriteTo for ItemLore {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        self.lines
            .write_prefixed_bound::<VarInt>(writer, Self::MAX_LINES)
    }
}

impl ReadFrom for ItemLore {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        let lines = Vec::<TextComponent>::read_prefixed_bound::<VarInt>(data, Self::MAX_LINES)?;
        Self::new(lines).map_err(std::io::Error::other)
    }
}

impl ToNbtTag for ItemLore {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::List(NbtList::from(
            self.lines
                .iter()
                .map(TextComponent::to_codec_nbt)
                .collect::<Vec<_>>(),
        ))
    }
}

impl FromNbtTag for ItemLore {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let list = tag.list()?;
        let tags = list.to_owned().as_nbt_tags();
        if tags.len() > Self::MAX_LINES {
            return None;
        }
        let lines = tags
            .iter()
            .map(TextComponent::from_nbt)
            .collect::<Option<Vec<_>>>()?;
        Self::new(lines).ok()
    }
}

impl HashComponent for ItemLore {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for line in &self.lines {
            line.hash_component(hasher);
        }
        hasher.end_list();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use simdnbt::{FromNbtTag, ToNbtTag};
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use text_components::TextComponent;
    use text_components::format::Color;

    use super::ItemLore;

    fn parse(tag: NbtTag) -> Option<ItemLore> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        ItemLore::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn styled_lines_apply_lore_defaults_without_overriding_explicit_style() {
        let mut explicit = TextComponent::plain("explicit");
        explicit.format.color = Some(Color::Aqua);
        explicit.format.italic = Some(false);
        let lore = ItemLore::new(vec![TextComponent::plain("default"), explicit])
            .expect("two lore lines should fit");

        assert_eq!(lore.styled_lines()[0].format.color, Some(Color::DarkPurple));
        assert_eq!(lore.styled_lines()[0].format.italic, Some(true));
        assert_eq!(lore.styled_lines()[1].format.color, Some(Color::Aqua));
        assert_eq!(lore.styled_lines()[1].format.italic, Some(false));
    }

    #[test]
    fn network_codec_round_trips_text_components() {
        let lore = ItemLore::new(vec![
            TextComponent::plain("first"),
            TextComponent::plain("second"),
        ])
        .expect("two lore lines should fit");
        let mut encoded = Vec::new();
        lore.write(&mut encoded).expect("lore should encode");

        assert_eq!(
            ItemLore::read(&mut Cursor::new(encoded.as_slice())).expect("lore should decode"),
            lore
        );
    }

    #[test]
    fn persistent_codec_round_trips_mixed_component_shapes() {
        let mut complex = NbtCompound::new();
        complex.insert("text", "second");
        complex.insert("bold", true);
        let tag = NbtTag::List(NbtList::from(vec![
            NbtTag::String("first".into()),
            NbtTag::Compound(complex),
        ]));
        let lore = parse(tag).expect("mixed component list should decode");

        assert_eq!(lore.lines().len(), 2);
        assert_eq!(parse(lore.clone().to_nbt_tag()), Some(lore));
    }

    #[test]
    fn persistent_codec_collapses_plain_lines_to_strings() {
        let lore = ItemLore::new(vec![
            TextComponent::plain("first"),
            TextComponent::plain("second"),
        ])
        .expect("two lore lines should fit");

        assert_eq!(
            lore.to_nbt_tag(),
            NbtTag::List(NbtList::String(vec!["first".into(), "second".into()]))
        );
    }
}
