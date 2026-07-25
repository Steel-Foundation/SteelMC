use super::{
    BorrowedNbtTag, Component, ComponentData, ComponentHasher, ComponentPatchEntry,
    DataComponentPatch, DataComponentType, DowncastType, EmbeddedNbtCodec, FromNbtTag,
    HashComponent, HashEntry, Identifier, NbtCompound, OwnedNbtTag, Result, ToNbtTag,
    sort_map_entries,
};

impl DataComponentPatch {
    /// Computes Vanilla's `HashOps` value for the persistent patch codec.
    pub fn compute_persistent_hash(&self) -> Result<i32> {
        use crate::{REGISTRY, RegistryExt};

        let mut entries = Vec::new();
        for (key, patch_entry) in &self.entries {
            let Some(component) = REGISTRY.data_components.by_key(key) else {
                continue;
            };
            if !component.is_persistent() {
                continue;
            }

            let (encoded_key, value_hash) = match patch_entry {
                ComponentPatchEntry::Set(data) => (key.to_string(), component.compute_hash(data)?),
                ComponentPatchEntry::Removed => (format!("!{key}"), ().compute_hash()),
            };
            entries.push(hash_entry(encoded_key.compute_hash(), value_hash));
        }
        sort_map_entries(&mut entries);

        let mut hasher = ComponentHasher::new();
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
        Ok(hasher.finish())
    }

    /// Iterates over removed component keys.
    pub fn iter_removed(&self) -> impl Iterator<Item = &Identifier> {
        self.entries.iter().filter_map(|(k, v)| {
            if matches!(v, ComponentPatchEntry::Removed) {
                Some(k)
            } else {
                None
            }
        })
    }

    fn encode_nbt(&self, validate: bool) -> (OwnedNbtTag, Vec<std::io::Error>) {
        use crate::{REGISTRY, RegistryExt};

        let mut compound = NbtCompound::new();
        let mut errors = Vec::new();

        for (key, entry) in &self.entries {
            let Some(component) = REGISTRY.data_components.by_key(key) else {
                continue;
            };
            if !component.is_persistent() {
                continue;
            }
            match entry {
                ComponentPatchEntry::Set(data) => {
                    let encoded = if validate {
                        component.validate_persistent_encoding(data)
                    } else {
                        component.write_nbt(data)
                    };
                    match encoded {
                        Ok(nbt) => {
                            compound.insert(key.to_string(), nbt);
                        }
                        Err(error) => errors.push(std::io::Error::other(format!(
                            "failed to encode component {key}: {error}"
                        ))),
                    }
                }
                ComponentPatchEntry::Removed => {
                    compound.insert(format!("!{key}"), NbtCompound::new());
                }
            }
        }

        (OwnedNbtTag::Compound(compound), errors)
    }

    /// Strictly encodes this component patch through its persistent codecs.
    ///
    /// This is the equivalent of Vanilla encoding an untrusted stack through
    /// `ItemStack.CODEC` before accepting it into server state.
    pub fn try_to_nbt_tag_ref(&self) -> Result<OwnedNbtTag> {
        let (tag, errors) = self.encode_nbt(true);
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(tag),
        }
    }

    /// Converts this component patch to NBT without consuming it.
    ///
    /// Save-time encoding mirrors Vanilla's `TagValueOutput`: invalid fields
    /// are reported and omitted from the partial result rather than aborting
    /// the owner save.
    #[must_use]
    pub fn to_nbt_tag_ref(&self) -> OwnedNbtTag {
        let (tag, errors) = self.encode_nbt(false);
        for error in errors {
            log::warn!("Item component serialization error: {error}");
        }
        tag
    }
}

pub(super) fn hash_entry(key_hash: i32, value_hash: i32) -> HashEntry {
    let key_hash = key_hash as u32;
    let value_hash = value_hash as u32;
    HashEntry {
        key_hash: i64::from(key_hash),
        value_hash: i64::from(value_hash),
        key_bytes: key_hash.to_le_bytes(),
        value_bytes: value_hash.to_le_bytes(),
    }
}
impl ToNbtTag for DataComponentPatch {
    fn to_nbt_tag(self) -> OwnedNbtTag {
        self.to_nbt_tag_ref()
    }
}

impl EmbeddedNbtCodec for &DataComponentPatch {
    type Error = std::io::Error;

    fn encode_embedded_nbt(self) -> Result<OwnedNbtTag> {
        self.try_to_nbt_tag_ref()
    }
}

impl FromNbtTag for DataComponentPatch {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        use crate::{REGISTRY, RegistryExt};

        let compound = tag.compound()?;
        let mut patch = Self::new();

        for (key, value) in compound.iter() {
            let key_str = key.to_str();

            if let Some(stripped) = key_str.strip_prefix('!') {
                let id = stripped.parse::<Identifier>().ok()?;
                let entry = REGISTRY.data_components.by_key(&id)?;
                if !entry.is_persistent() || value.compound().is_none() {
                    return None;
                }
                patch.entries.insert(id, ComponentPatchEntry::Removed);
            } else {
                let id = key_str.parse::<Identifier>().ok()?;
                let entry = REGISTRY.data_components.by_key(&id)?;
                if !entry.is_persistent() {
                    return None;
                }
                let component_data = entry.read_nbt(value)?;
                patch
                    .entries
                    .insert(id, ComponentPatchEntry::Set(component_data));
            }
        }

        Some(patch)
    }
}

/// Attempts to extract a typed component from `ComponentData`.
#[must_use]
pub fn component_try_into<T: Component + DowncastType>(
    data: &ComponentData,
    _component: DataComponentType<T>,
) -> Option<&T> {
    data.downcast_ref::<T>()
}
