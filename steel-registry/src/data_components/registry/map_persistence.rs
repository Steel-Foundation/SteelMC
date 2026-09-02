use super::{
    BorrowedNbtTag, DataComponentMap, FromNbtTag, Identifier, NbtCompound, OwnedNbtTag, ToNbtTag,
};

impl DataComponentMap {
    /// Encodes the persistent components like Vanilla `DataComponentMap.CODEC`.
    ///
    /// Save-time encoding mirrors Vanilla `TagValueOutput`: transient components
    /// are skipped and invalid values are reported and omitted.
    #[must_use]
    pub fn to_nbt_tag_ref(&self) -> OwnedNbtTag {
        use crate::{REGISTRY, RegistryExt};

        let mut compound = NbtCompound::new();
        for (key, data) in self.iter() {
            let Some(component) = REGISTRY.data_components.by_key(key) else {
                continue;
            };
            if !component.is_persistent() {
                continue;
            }
            match component.write_nbt(data) {
                Ok(nbt) => {
                    compound.insert(key.to_string(), nbt);
                }
                Err(error) => {
                    log::warn!(
                        "Component map serialization error: failed to encode {key}: {error}"
                    );
                }
            }
        }
        OwnedNbtTag::Compound(compound)
    }
}

impl ToNbtTag for DataComponentMap {
    fn to_nbt_tag(self) -> OwnedNbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for DataComponentMap {
    /// Rejects unknown and transient component keys like Vanilla's persistent map codec.
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        use crate::{REGISTRY, RegistryExt};

        let compound = tag.compound()?;
        let mut map = Self::new();
        for (key, value) in compound.iter() {
            let id = key.to_str().parse::<Identifier>().ok()?;
            let entry = REGISTRY.data_components.by_key(&id)?;
            if !entry.is_persistent() {
                return None;
            }
            map.set_raw(id, entry.read_nbt(value)?);
        }
        Some(map)
    }
}
