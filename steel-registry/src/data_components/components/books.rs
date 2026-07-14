//! Writable and written book item components.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{PrefixedRead, PrefixedWrite, ReadFrom, WriteTo};
use text_components::TextComponent;

const MAX_NETWORK_STRING_LENGTH: usize = 32_767;
const MAX_NETWORK_STRING_BYTES: usize = MAX_NETWORK_STRING_LENGTH * 3;

/// Raw text paired with the optional server-filtered projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filterable<T> {
    raw: T,
    filtered: Option<T>,
}

impl<T> Filterable<T> {
    #[must_use]
    pub const fn new(raw: T, filtered: Option<T>) -> Self {
        Self { raw, filtered }
    }

    #[must_use]
    pub const fn pass_through(raw: T) -> Self {
        Self::new(raw, None)
    }

    #[must_use]
    pub const fn raw(&self) -> &T {
        &self.raw
    }

    #[must_use]
    pub const fn filtered(&self) -> Option<&T> {
        self.filtered.as_ref()
    }

    #[must_use]
    pub fn get(&self, filter_enabled: bool) -> &T {
        if filter_enabled {
            self.filtered.as_ref().unwrap_or(&self.raw)
        } else {
            &self.raw
        }
    }
}

/// Editable pages in a writable book.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WritableBookContent {
    pages: Vec<Filterable<String>>,
}

impl WritableBookContent {
    pub const PAGE_EDIT_LENGTH: usize = 1024;
    pub const MAX_PAGES: usize = 100;

    #[must_use]
    pub const fn empty() -> Self {
        Self { pages: Vec::new() }
    }

    pub fn new(pages: Vec<Filterable<String>>) -> Result<Self> {
        if pages.len() > Self::MAX_PAGES {
            return Err(Error::other(format!(
                "Got {} pages, but maximum is {}",
                pages.len(),
                Self::MAX_PAGES
            )));
        }
        if pages.iter().any(|page| {
            string_too_long(page.raw(), Self::PAGE_EDIT_LENGTH)
                || page
                    .filtered()
                    .is_some_and(|value| string_too_long(value, Self::PAGE_EDIT_LENGTH))
        }) {
            return Err(Error::other(
                "Writable book page is longer than 1024 characters",
            ));
        }
        Ok(Self { pages })
    }

    #[must_use]
    pub fn pages(&self) -> &[Filterable<String>] {
        &self.pages
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !self.pages.is_empty() {
            compound.insert(
                "pages",
                NbtTag::List(NbtList::Compound(
                    self.pages.iter().map(filterable_string_nbt).collect(),
                )),
            );
        }
        NbtTag::Compound(compound)
    }
}

impl WriteTo for WritableBookContent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_bounded_count(self.pages.len(), Self::MAX_PAGES, writer)?;
        for page in &self.pages {
            write_filterable_string(page, Self::PAGE_EDIT_LENGTH, writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for WritableBookContent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_bounded_count(data, Self::MAX_PAGES)?;
        let mut pages = Vec::with_capacity(count);
        for _ in 0..count {
            pages.push(read_filterable_string(data, Self::PAGE_EDIT_LENGTH)?);
        }
        Self::new(pages)
    }
}

impl ToNbtTag for WritableBookContent {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for WritableBookContent {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let pages = match compound.get("pages") {
            Some(tag) => {
                let values = tag.list()?.to_owned().as_nbt_tags();
                if values.len() > Self::MAX_PAGES {
                    return None;
                }
                values
                    .iter()
                    .map(|tag| filterable_string_from_nbt(tag, Self::PAGE_EDIT_LENGTH))
                    .collect::<Option<Vec<_>>>()?
            }
            None => Vec::new(),
        };
        Self::new(pages).ok()
    }
}

impl HashComponent for WritableBookContent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(1);
        if !self.pages.is_empty() {
            push_hash_entry(&mut entries, "pages", &FilterableStringList(&self.pages));
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Signed book metadata and rendered pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenBookContent {
    title: Filterable<String>,
    author: String,
    generation: i32,
    pages: Vec<Filterable<TextComponent>>,
    resolved: bool,
}

impl WrittenBookContent {
    pub const TITLE_MAX_LENGTH: usize = 32;
    pub const MAX_GENERATION: i32 = 3;
    pub const PAGE_LENGTH: usize = 32_767;

    pub fn new(
        title: Filterable<String>,
        author: String,
        generation: i32,
        pages: Vec<Filterable<TextComponent>>,
        resolved: bool,
    ) -> Result<Self> {
        if string_too_long(title.raw(), Self::TITLE_MAX_LENGTH)
            || title
                .filtered()
                .is_some_and(|value| string_too_long(value, Self::TITLE_MAX_LENGTH))
        {
            return Err(Error::other(
                "Written book title is longer than 32 characters",
            ));
        }
        if !(0..=Self::MAX_GENERATION).contains(&generation) {
            return Err(Error::other(format!(
                "Book generation must be in 0..={}, got {generation}",
                Self::MAX_GENERATION
            )));
        }
        if string_too_long(&author, MAX_NETWORK_STRING_LENGTH)
            || author.len() > MAX_NETWORK_STRING_BYTES
        {
            return Err(Error::other(
                "Written book author exceeds the network string limit",
            ));
        }
        Ok(Self {
            title,
            author,
            generation,
            pages,
            resolved,
        })
    }

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            title: Filterable::pass_through(String::new()),
            author: String::new(),
            generation: 0,
            pages: Vec::new(),
            resolved: true,
        }
    }

    #[must_use]
    pub const fn title(&self) -> &Filterable<String> {
        &self.title
    }

    #[must_use]
    pub fn author(&self) -> &str {
        &self.author
    }

    #[must_use]
    pub const fn generation(&self) -> i32 {
        self.generation
    }

    #[must_use]
    pub fn pages(&self) -> &[Filterable<TextComponent>] {
        &self.pages
    }

    #[must_use]
    pub const fn resolved(&self) -> bool {
        self.resolved
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert(
            "title",
            NbtTag::Compound(filterable_string_nbt(&self.title)),
        );
        compound.insert("author", self.author.clone());
        if self.generation != 0 {
            compound.insert("generation", self.generation);
        }
        if !self.pages.is_empty() {
            compound.insert(
                "pages",
                NbtTag::List(NbtList::Compound(
                    self.pages.iter().map(filterable_component_nbt).collect(),
                )),
            );
        }
        if self.resolved {
            compound.insert("resolved", true);
        }
        NbtTag::Compound(compound)
    }
}

impl WriteTo for WrittenBookContent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_filterable_string(&self.title, Self::TITLE_MAX_LENGTH, writer)?;
        write_network_string(&self.author, MAX_NETWORK_STRING_LENGTH, writer)?;
        VarInt(self.generation).write(writer)?;
        write_count(self.pages.len(), writer)?;
        for page in &self.pages {
            write_filterable_component(page, writer)?;
        }
        self.resolved.write(writer)
    }
}

impl ReadFrom for WrittenBookContent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let title = read_filterable_string(data, Self::TITLE_MAX_LENGTH)?;
        let author = read_network_string(data, MAX_NETWORK_STRING_LENGTH)?;
        let generation = VarInt::read(data)?.0;
        let count = read_count(data)?;
        let mut pages = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            pages.push(read_filterable_component(data)?);
        }
        Self::new(title, author, generation, pages, bool::read(data)?)
    }
}

impl ToNbtTag for WrittenBookContent {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for WrittenBookContent {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let title =
            filterable_string_from_nbt(&compound.get("title")?.to_owned(), Self::TITLE_MAX_LENGTH)?;
        let author = compound.get("author")?.string()?.to_string();
        let generation = match compound.get("generation") {
            Some(tag) => tag.codec_i32()?,
            None => 0,
        };
        let pages = match compound.get("pages") {
            Some(tag) => tag
                .list()?
                .to_owned()
                .as_nbt_tags()
                .iter()
                .map(filterable_component_from_nbt)
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        let resolved = match compound.get("resolved") {
            Some(tag) => tag.codec_bool()?,
            None => false,
        };
        Self::new(title, author, generation, pages, resolved).ok()
    }
}

impl HashComponent for WrittenBookContent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(5);
        push_hash_entry(&mut entries, "title", &FilterableStringHash(&self.title));
        push_hash_entry(&mut entries, "author", self.author.as_str());
        if self.generation != 0 {
            push_hash_entry(&mut entries, "generation", &self.generation);
        }
        if !self.pages.is_empty() {
            push_hash_entry(&mut entries, "pages", &FilterableComponentList(&self.pages));
        }
        if self.resolved {
            push_hash_entry(&mut entries, "resolved", &true);
        }
        hash_entries(hasher, &mut entries);
    }
}

fn filterable_string_nbt(value: &Filterable<String>) -> NbtCompound {
    let mut compound = NbtCompound::new();
    compound.insert("raw", value.raw.clone());
    if let Some(filtered) = &value.filtered {
        compound.insert("filtered", filtered.clone());
    }
    compound
}

fn filterable_string_from_nbt(tag: &NbtTag, max_length: usize) -> Option<Filterable<String>> {
    if let Some(compound) = tag.compound()
        && let Some(raw) = compound.get("raw")
    {
        let raw = raw.string()?.to_string();
        let filtered = match compound.get("filtered") {
            Some(tag) => Some(tag.string()?.to_string()),
            None => None,
        };
        return (!string_too_long(&raw, max_length)
            && filtered
                .as_ref()
                .is_none_or(|value| !string_too_long(value, max_length)))
        .then_some(Filterable::new(raw, filtered));
    }
    let raw = tag.string()?.to_string();
    (!string_too_long(&raw, max_length)).then_some(Filterable::pass_through(raw))
}

fn filterable_component_nbt(value: &Filterable<TextComponent>) -> NbtCompound {
    let mut compound = NbtCompound::new();
    compound.insert("raw", value.raw.to_codec_nbt());
    if let Some(filtered) = &value.filtered {
        compound.insert("filtered", filtered.to_codec_nbt());
    }
    compound
}

fn filterable_component_from_nbt(tag: &NbtTag) -> Option<Filterable<TextComponent>> {
    if let Some(compound) = tag.compound()
        && let Some(raw) = compound.get("raw")
    {
        let raw = restricted_component_from_nbt(raw)?;
        let filtered = match compound.get("filtered") {
            Some(tag) => Some(restricted_component_from_nbt(tag)?),
            None => None,
        };
        return Some(Filterable::new(raw, filtered));
    }
    restricted_component_from_nbt(tag).map(Filterable::pass_through)
}

fn restricted_component_from_nbt(tag: &NbtTag) -> Option<TextComponent> {
    let component = TextComponent::from_nbt(tag)?;
    let encoded = serde_json::to_string(&component).ok()?;
    (encoded.encode_utf16().count() <= WrittenBookContent::PAGE_LENGTH).then_some(component)
}

fn write_filterable_string(
    value: &Filterable<String>,
    max_length: usize,
    writer: &mut impl Write,
) -> Result<()> {
    write_network_string(&value.raw, max_length, writer)?;
    value.filtered.is_some().write(writer)?;
    if let Some(filtered) = &value.filtered {
        write_network_string(filtered, max_length, writer)?;
    }
    Ok(())
}

fn read_filterable_string(
    data: &mut Cursor<&[u8]>,
    max_length: usize,
) -> Result<Filterable<String>> {
    let raw = read_network_string(data, max_length)?;
    let filtered = if bool::read(data)? {
        Some(read_network_string(data, max_length)?)
    } else {
        None
    };
    Ok(Filterable::new(raw, filtered))
}

fn write_filterable_component(
    value: &Filterable<TextComponent>,
    writer: &mut impl Write,
) -> Result<()> {
    write_component_network(&value.raw, writer)?;
    value.filtered.is_some().write(writer)?;
    if let Some(filtered) = &value.filtered {
        write_component_network(filtered, writer)?;
    }
    Ok(())
}

fn read_filterable_component(data: &mut Cursor<&[u8]>) -> Result<Filterable<TextComponent>> {
    let raw = TextComponent::read(data)?;
    let filtered = if bool::read(data)? {
        Some(TextComponent::read(data)?)
    } else {
        None
    };
    Ok(Filterable::new(raw, filtered))
}

fn write_component_network(component: &TextComponent, writer: &mut impl Write) -> Result<()> {
    let mut encoded = Vec::new();
    component.to_codec_nbt().write(&mut encoded);
    writer.write_all(&encoded)
}

fn write_network_string(value: &str, max_length: usize, writer: &mut impl Write) -> Result<()> {
    if string_too_long(value, max_length) || value.len() > max_length.saturating_mul(3) {
        return Err(Error::other(format!(
            "String exceeds the {max_length}-character network limit"
        )));
    }
    value.write_prefixed::<VarInt>(writer)
}

fn read_network_string(data: &mut Cursor<&[u8]>, max_length: usize) -> Result<String> {
    let value = String::read_prefixed_bound::<VarInt>(data, max_length.saturating_mul(3))?;
    if string_too_long(&value, max_length) {
        return Err(Error::other(format!(
            "String exceeds the {max_length}-character network limit"
        )));
    }
    Ok(value)
}

fn string_too_long(value: &str, max_length: usize) -> bool {
    value.encode_utf16().count() > max_length
}

fn write_bounded_count(count: usize, max: usize, writer: &mut impl Write) -> Result<()> {
    if count > max {
        return Err(Error::other(format!(
            "Collection size {count} exceeds {max}"
        )));
    }
    write_count(count, writer)
}

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(count).map_err(|_| Error::other("Collection is too large"))?;
    VarInt(count).write(writer)
}

fn read_bounded_count(data: &mut Cursor<&[u8]>, max: usize) -> Result<usize> {
    let count = read_count(data)?;
    if count > max {
        return Err(Error::other(format!(
            "Collection size {count} exceeds {max}"
        )));
    }
    Ok(count)
}

fn read_count(data: &mut Cursor<&[u8]>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count).map_err(|_| Error::other(format!("Negative collection size: {count}")))
}

struct FilterableStringHash<'a>(&'a Filterable<String>);

impl HashComponent for FilterableStringHash<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "raw", self.0.raw.as_str());
        if let Some(filtered) = &self.0.filtered {
            push_hash_entry(&mut entries, "filtered", filtered.as_str());
        }
        hash_entries(hasher, &mut entries);
    }
}

struct FilterableComponentHash<'a>(&'a Filterable<TextComponent>);

impl HashComponent for FilterableComponentHash<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "raw", &self.0.raw);
        if let Some(filtered) = &self.0.filtered {
            push_hash_entry(&mut entries, "filtered", filtered);
        }
        hash_entries(hasher, &mut entries);
    }
}

struct FilterableStringList<'a>(&'a [Filterable<String>]);

impl HashComponent for FilterableStringList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for value in self.0 {
            hasher.put_component_hash(&FilterableStringHash(value));
        }
        hasher.end_list();
    }
}

struct FilterableComponentList<'a>(&'a [Filterable<TextComponent>]);

impl HashComponent for FilterableComponentList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for value in self.0 {
            hasher.put_component_hash(&FilterableComponentHash(value));
        }
        hasher.end_list();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

fn hash_entries(hasher: &mut ComponentHasher, entries: &mut [HashEntry]) {
    sort_map_entries(entries);
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::ToNbtTag as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use text_components::TextComponent;

    use super::{Filterable, WritableBookContent, WrittenBookContent};
    use crate::data_components::vanilla_components::WRITABLE_BOOK_CONTENT;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse<T: simdnbt::FromNbtTag>(tag: simdnbt::owned::NbtTag) -> Option<T> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        T::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn writable_pages_use_full_filterable_persistence_and_bounded_network() {
        let value = WritableBookContent::new(vec![Filterable::new(
            "raw".to_owned(),
            Some("filtered".to_owned()),
        )])
        .expect("valid writable book");
        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt), Some(value.clone()));
        let mut network = Vec::new();
        value.write(&mut network).expect("book should encode");
        assert_eq!(
            WritableBookContent::read(&mut Cursor::new(network.as_slice()))
                .expect("book should decode"),
            value
        );
        assert!(
            WritableBookContent::new(vec![Filterable::pass_through("x".repeat(1025))]).is_err()
        );
    }

    #[test]
    fn written_book_round_trips_text_pages_and_validates_generation() {
        let value = WrittenBookContent::new(
            Filterable::pass_through("Title".to_owned()),
            "Author".to_owned(),
            2,
            vec![Filterable::pass_through(TextComponent::plain("Page"))],
            true,
        )
        .expect("valid written book");
        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt), Some(value.clone()));
        let mut network = Vec::new();
        value.write(&mut network).expect("book should encode");
        assert_eq!(
            WrittenBookContent::read(&mut Cursor::new(network.as_slice()))
                .expect("book should decode"),
            value
        );
        assert!(
            WrittenBookContent::new(
                Filterable::pass_through(String::new()),
                String::new(),
                4,
                Vec::new(),
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn extracted_writable_book_starts_with_empty_pages() {
        init_test_registry();
        let item = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("writable_book"))
            .expect("writable book should be registered");
        assert_eq!(
            item.components.get(WRITABLE_BOOK_CONTENT),
            Some(WritableBookContent::empty())
        );
    }
}
