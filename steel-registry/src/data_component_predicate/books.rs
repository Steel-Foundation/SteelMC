use super::{
    CollectionPredicate, ComponentHasher, DataComponentPredicateCodec, Debug, DowncastType,
    DowncastTypeKey, HashComponent, IntBounds, NbtCompound, NbtNumeric, NbtTag, TextComponent,
    collection_field_nbt, decode_optional, hash_entries, hash_optional_collection_field,
    owned_string, push_hash_entry,
};

/// Predicate for one writable-book page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritableBookPagePredicate(String);

impl WritableBookPagePredicate {
    #[must_use]
    pub const fn new(contents: String) -> Self {
        Self(contents)
    }

    #[must_use]
    pub fn contents(&self) -> &str {
        &self.0
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        tag.string()?.to_owned().try_into_string().ok().map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        NbtTag::String(self.0.clone().into())
    }
}

impl HashComponent for WritableBookPagePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

/// Predicate over writable-book pages.
#[derive(Debug, Clone, PartialEq)]
pub struct WritableBookPredicate(Option<CollectionPredicate<WritableBookPagePredicate>>);

impl WritableBookPredicate {
    #[must_use]
    pub const fn new(pages: Option<CollectionPredicate<WritableBookPagePredicate>>) -> Self {
        Self(pages)
    }

    #[must_use]
    pub const fn pages(&self) -> Option<&CollectionPredicate<WritableBookPagePredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for WritableBookPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "pages", |tag| {
            CollectionPredicate::from_nbt_with(tag, WritableBookPagePredicate::from_nbt_value)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(
            self.0.as_ref(),
            "pages",
            WritableBookPagePredicate::to_nbt_value,
        )
    }
}

impl HashComponent for WritableBookPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "pages",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    WritableBookPredicate,
    "steel:data_component_predicate/writable_book_content"
);

/// Predicate for one written-book page.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenBookPagePredicate(TextComponent);

impl WrittenBookPagePredicate {
    #[must_use]
    pub const fn new(contents: TextComponent) -> Self {
        Self(contents)
    }

    #[must_use]
    pub const fn contents(&self) -> &TextComponent {
        &self.0
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        TextComponent::from_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.to_codec_nbt()
    }
}

impl HashComponent for WrittenBookPagePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

/// Predicate over written-book metadata and pages.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenBookPredicate {
    pages: Option<CollectionPredicate<WrittenBookPagePredicate>>,
    author: Option<String>,
    title: Option<String>,
    generation: IntBounds,
    resolved: Option<bool>,
}

impl WrittenBookPredicate {
    #[must_use]
    pub const fn new(
        pages: Option<CollectionPredicate<WrittenBookPagePredicate>>,
        author: Option<String>,
        title: Option<String>,
        generation: IntBounds,
        resolved: Option<bool>,
    ) -> Self {
        Self {
            pages,
            author,
            title,
            generation,
            resolved,
        }
    }

    #[must_use]
    pub const fn pages(&self) -> Option<&CollectionPredicate<WrittenBookPagePredicate>> {
        self.pages.as_ref()
    }

    #[must_use]
    pub const fn author(&self) -> Option<&String> {
        self.author.as_ref()
    }

    #[must_use]
    pub const fn title(&self) -> Option<&String> {
        self.title.as_ref()
    }

    #[must_use]
    pub const fn generation(&self) -> IntBounds {
        self.generation
    }

    #[must_use]
    pub const fn resolved(&self) -> Option<bool> {
        self.resolved
    }
}

impl DataComponentPredicateCodec for WrittenBookPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            pages: decode_optional(compound, "pages", |tag| {
                CollectionPredicate::from_nbt_with(tag, WrittenBookPagePredicate::from_nbt_value)
            })?,
            author: decode_optional(compound, "author", owned_string)?,
            title: decode_optional(compound, "title", owned_string)?,
            generation: compound
                .get("generation")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
            resolved: decode_optional(compound, "resolved", NbtNumeric::codec_bool)?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(pages) = &self.pages {
            compound.insert(
                "pages",
                pages.to_nbt_with(WrittenBookPagePredicate::to_nbt_value),
            );
        }
        if let Some(author) = &self.author {
            compound.insert("author", author.as_str());
        }
        if let Some(title) = &self.title {
            compound.insert("title", title.as_str());
        }
        if !self.generation.is_any() {
            compound.insert("generation", self.generation.as_nbt_tag());
        }
        if let Some(resolved) = self.resolved {
            compound.insert("resolved", resolved);
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for WrittenBookPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(pages) = &self.pages {
            let mut value_hasher = ComponentHasher::new();
            pages.hash_with(&mut value_hasher, HashComponent::compute_hash);
            crate::item_predicate::push_prehashed_entry(&mut entries, "pages", value_hasher);
        }
        if let Some(author) = &self.author {
            push_hash_entry(&mut entries, "author", author);
        }
        if let Some(title) = &self.title {
            push_hash_entry(&mut entries, "title", title);
        }
        if !self.generation.is_any() {
            push_hash_entry(&mut entries, "generation", &self.generation);
        }
        if let Some(resolved) = self.resolved {
            push_hash_entry(&mut entries, "resolved", &resolved);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(
    WrittenBookPredicate,
    "steel:data_component_predicate/written_book_content"
);
