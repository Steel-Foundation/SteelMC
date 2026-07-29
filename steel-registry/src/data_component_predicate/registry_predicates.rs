use super::*;

/// Predicate over armor trim material and pattern holders.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimPredicate {
    material: Option<RegistryHolderSet<TrimMaterial>>,
    pattern: Option<RegistryHolderSet<TrimPattern>>,
}

impl TrimPredicate {
    #[must_use]
    pub const fn new(
        material: Option<RegistryHolderSet<TrimMaterial>>,
        pattern: Option<RegistryHolderSet<TrimPattern>>,
    ) -> Self {
        Self { material, pattern }
    }

    #[must_use]
    pub const fn material(&self) -> Option<&RegistryHolderSet<TrimMaterial>> {
        self.material.as_ref()
    }

    #[must_use]
    pub const fn pattern(&self) -> Option<&RegistryHolderSet<TrimPattern>> {
        self.pattern.as_ref()
    }
}

impl DataComponentPredicateCodec for TrimPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            material: decode_optional(compound, "material", RegistryHolderSet::from_owned_nbt)?,
            pattern: decode_optional(compound, "pattern", RegistryHolderSet::from_owned_nbt)?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(material) = &self.material {
            compound.insert("material", material.clone().to_nbt_tag());
        }
        if let Some(pattern) = &self.pattern {
            compound.insert("pattern", pattern.clone().to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for TrimPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(material) = &self.material {
            push_hash_entry(&mut entries, "material", material);
        }
        if let Some(pattern) = &self.pattern {
            push_hash_entry(&mut entries, "pattern", pattern);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(TrimPredicate, "steel:data_component_predicate/trim");

/// Predicate over a jukebox-playable song holder.
#[derive(Debug, Clone, PartialEq)]
pub struct JukeboxPlayablePredicate(Option<RegistryHolderSet<JukeboxSong>>);

impl JukeboxPlayablePredicate {
    #[must_use]
    pub const fn new(song: Option<RegistryHolderSet<JukeboxSong>>) -> Self {
        Self(song)
    }

    #[must_use]
    pub const fn song(&self) -> Option<&RegistryHolderSet<JukeboxSong>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for JukeboxPlayablePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "song", RegistryHolderSet::from_owned_nbt).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(song) = &self.0 {
            compound.insert("song", song.clone().to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for JukeboxPlayablePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(song) = &self.0 {
            push_hash_entry(&mut entries, "song", song);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(
    JukeboxPlayablePredicate,
    "steel:data_component_predicate/jukebox_playable"
);

/// Predicate over registered villager variants.
#[derive(Debug, Clone, PartialEq)]
pub struct VillagerTypePredicate(RegistryHolderSet<VillagerType>);

impl VillagerTypePredicate {
    #[must_use]
    pub const fn new(villager_types: RegistryHolderSet<VillagerType>) -> Self {
        Self(villager_types)
    }

    #[must_use]
    pub const fn villager_types(&self) -> &RegistryHolderSet<VillagerType> {
        &self.0
    }
}

impl DataComponentPredicateCodec for VillagerTypePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        RegistryHolderSet::from_owned_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.clone().to_nbt_tag()
    }
}

impl HashComponent for VillagerTypePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    VillagerTypePredicate,
    "steel:data_component_predicate/villager_variant"
);
