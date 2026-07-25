use super::{
    ComponentHasher, DataComponentPredicateCodec, Debug, HashComponent, HashEntry, IntBounds,
    NbtCompound, NbtList, NbtTag, decode_optional, hash_entries, push_hash_entry,
};

/// Generic collection predicate shared by container, firework, book, and attribute checks.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionPredicate<P> {
    contains: Option<Vec<P>>,
    counts: Option<Vec<CollectionCountPredicate<P>>>,
    size: Option<IntBounds>,
}

impl<P> CollectionPredicate<P> {
    #[must_use]
    pub const fn new(
        contains: Option<Vec<P>>,
        counts: Option<Vec<CollectionCountPredicate<P>>>,
        size: Option<IntBounds>,
    ) -> Self {
        Self {
            contains,
            counts,
            size,
        }
    }

    #[must_use]
    pub const fn contains(&self) -> Option<&Vec<P>> {
        self.contains.as_ref()
    }

    #[must_use]
    pub const fn counts(&self) -> Option<&Vec<CollectionCountPredicate<P>>> {
        self.counts.as_ref()
    }

    #[must_use]
    pub const fn size(&self) -> Option<&IntBounds> {
        self.size.as_ref()
    }

    pub(super) fn from_nbt_with(
        tag: &NbtTag,
        decode: impl Fn(&NbtTag) -> Option<P> + Copy,
    ) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode_optional(compound, "contains", |tag| decode_list(tag, decode))?,
            decode_optional(compound, "count", |tag| {
                decode_list(tag, |tag| {
                    CollectionCountPredicate::from_nbt_with(tag, decode)
                })
            })?,
            decode_optional(compound, "size", IntBounds::from_owned_nbt)?,
        ))
    }

    pub(super) fn to_nbt_with(&self, encode: impl Fn(&P) -> NbtTag + Copy) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(contains) = &self.contains {
            compound.insert("contains", encode_list(contains, encode));
        }
        if let Some(counts) = &self.counts {
            compound.insert(
                "count",
                encode_list(counts, |entry| entry.to_nbt_with(encode)),
            );
        }
        if let Some(size) = &self.size {
            compound.insert("size", size.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }

    pub(super) fn hash_with(&self, hasher: &mut ComponentHasher, hash: impl Fn(&P) -> i32 + Copy) {
        let mut entries = Vec::new();
        if let Some(contains) = &self.contains {
            let mut value_hasher = ComponentHasher::new();
            hash_list_with(contains, &mut value_hasher, hash);
            crate::item_predicate::push_prehashed_entry(&mut entries, "contains", value_hasher);
        }
        if let Some(counts) = &self.counts {
            let mut value_hasher = ComponentHasher::new();
            value_hasher.start_list();
            for entry in counts {
                let mut entry_hasher = ComponentHasher::new();
                entry.hash_with(&mut entry_hasher, hash);
                value_hasher.put_raw_bytes(&(entry_hasher.finish() as u32).to_le_bytes());
            }
            value_hasher.end_list();
            crate::item_predicate::push_prehashed_entry(&mut entries, "count", value_hasher);
        }
        if let Some(size) = &self.size {
            push_hash_entry(&mut entries, "size", size);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// One element predicate and the accepted number of matching elements.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionCountPredicate<P> {
    test: P,
    count: IntBounds,
}

impl<P> CollectionCountPredicate<P> {
    #[must_use]
    pub const fn new(test: P, count: IntBounds) -> Self {
        Self { test, count }
    }

    #[must_use]
    pub const fn test(&self) -> &P {
        &self.test
    }

    #[must_use]
    pub const fn count(&self) -> IntBounds {
        self.count
    }

    fn from_nbt_with(tag: &NbtTag, decode: impl Fn(&NbtTag) -> Option<P>) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode(compound.get("test")?)?,
            IntBounds::from_owned_nbt(compound.get("count")?)?,
        ))
    }

    fn to_nbt_with(&self, encode: impl Fn(&P) -> NbtTag) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("test", encode(&self.test));
        compound.insert("count", self.count.as_nbt_tag());
        NbtTag::Compound(compound)
    }

    fn hash_with(&self, hasher: &mut ComponentHasher, hash: impl Fn(&P) -> i32) {
        let mut entries = Vec::new();
        let mut key_hasher = ComponentHasher::new();
        "test".hash_component(&mut key_hasher);
        entries.push(HashEntry::from_hashes(
            key_hasher.finish() as u32,
            hash(&self.test) as u32,
        ));
        push_hash_entry(&mut entries, "count", &self.count);
        hash_entries(hasher, &mut entries);
    }
}

pub(super) fn decode_list<T>(
    tag: &NbtTag,
    decode: impl Fn(&NbtTag) -> Option<T>,
) -> Option<Vec<T>> {
    tag.list()?.as_nbt_tags().iter().map(decode).collect()
}

pub(super) fn encode_list<T>(values: &[T], encode: impl Fn(&T) -> NbtTag) -> NbtTag {
    NbtTag::List(NbtList::from(values.iter().map(encode).collect::<Vec<_>>()))
}

pub(super) fn hash_list_with<T>(
    values: &[T],
    hasher: &mut ComponentHasher,
    hash: impl Fn(&T) -> i32,
) {
    hasher.start_list();
    for value in values {
        hasher.put_raw_bytes(&(hash(value) as u32).to_le_bytes());
    }
    hasher.end_list();
}

pub(super) fn collection_field_nbt<P>(
    collection: Option<&CollectionPredicate<P>>,
    name: &str,
    encode: impl Fn(&P) -> NbtTag + Copy,
) -> NbtTag {
    let mut compound = NbtCompound::new();
    if let Some(collection) = collection {
        compound.insert(name, collection.to_nbt_with(encode));
    }
    NbtTag::Compound(compound)
}

pub(super) fn hash_optional_collection_field<P>(
    collection: Option<&CollectionPredicate<P>>,
    name: &str,
    hasher: &mut ComponentHasher,
    hash: impl Fn(&P) -> i32 + Copy,
) {
    let mut entries = Vec::new();
    if let Some(collection) = collection {
        let mut value_hasher = ComponentHasher::new();
        collection.hash_with(&mut value_hasher, hash);
        crate::item_predicate::push_prehashed_entry(&mut entries, name, value_hasher);
    }
    hash_entries(hasher, &mut entries);
}

pub(super) fn hash_nbt_codec<T: DataComponentPredicateCodec>(
    value: &T,
    hasher: &mut ComponentHasher,
) {
    value.to_nbt_value().hash_component(hasher);
}

pub(super) fn owned_string(tag: &NbtTag) -> Option<String> {
    tag.string()?.to_owned().try_into_string().ok()
}
