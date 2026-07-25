use super::*;

/// Durability and current-damage bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamagePredicate {
    durability: IntBounds,
    damage: IntBounds,
}

impl DamagePredicate {
    #[must_use]
    pub const fn new(durability: IntBounds, damage: IntBounds) -> Self {
        Self { durability, damage }
    }

    #[must_use]
    pub const fn durability(&self) -> IntBounds {
        self.durability
    }

    #[must_use]
    pub const fn damage(&self) -> IntBounds {
        self.damage
    }
}

impl DataComponentPredicateCodec for DamagePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            compound
                .get("durability")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
            compound
                .get("damage")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
        ))
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !self.durability.is_any() {
            compound.insert("durability", self.durability.as_nbt_tag());
        }
        if !self.damage.is_any() {
            compound.insert("damage", self.damage.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for DamagePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_nbt_codec(self, hasher);
    }
}

impl_predicate_downcast_type!(DamagePredicate, "steel:data_component_predicate/damage");

/// One enchantment holder-set and accepted level range.
#[derive(Debug, Clone, PartialEq)]
pub struct EnchantmentPredicate {
    enchantments: Option<RegistryHolderSet<Enchantment>>,
    levels: IntBounds,
}

impl EnchantmentPredicate {
    #[must_use]
    pub const fn new(
        enchantments: Option<RegistryHolderSet<Enchantment>>,
        levels: IntBounds,
    ) -> Self {
        Self {
            enchantments,
            levels,
        }
    }

    #[must_use]
    pub const fn enchantments(&self) -> Option<&RegistryHolderSet<Enchantment>> {
        self.enchantments.as_ref()
    }

    #[must_use]
    pub const fn levels(&self) -> IntBounds {
        self.levels
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            decode_optional(compound, "enchantments", RegistryHolderSet::from_owned_nbt)?,
            compound
                .get("levels")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
        ))
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(enchantments) = &self.enchantments {
            compound.insert("enchantments", enchantments.clone().to_nbt_tag());
        }
        if !self.levels.is_any() {
            compound.insert("levels", self.levels.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

/// Applied-enchantment predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct EnchantmentsPredicate(Vec<EnchantmentPredicate>);

impl EnchantmentsPredicate {
    #[must_use]
    pub const fn new(enchantments: Vec<EnchantmentPredicate>) -> Self {
        Self(enchantments)
    }

    #[must_use]
    pub fn enchantments(&self) -> &[EnchantmentPredicate] {
        &self.0
    }
}

impl DataComponentPredicateCodec for EnchantmentsPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        decode_list(tag, EnchantmentPredicate::from_nbt_value).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        encode_list(&self.0, EnchantmentPredicate::to_nbt_value)
    }
}

impl HashComponent for EnchantmentsPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_nbt_codec(self, hasher);
    }
}

impl_predicate_downcast_type!(
    EnchantmentsPredicate,
    "steel:data_component_predicate/enchantments"
);

/// Stored-enchantment predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEnchantmentsPredicate(Vec<EnchantmentPredicate>);

impl StoredEnchantmentsPredicate {
    #[must_use]
    pub const fn new(enchantments: Vec<EnchantmentPredicate>) -> Self {
        Self(enchantments)
    }

    #[must_use]
    pub fn enchantments(&self) -> &[EnchantmentPredicate] {
        &self.0
    }
}

impl DataComponentPredicateCodec for StoredEnchantmentsPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        decode_list(tag, EnchantmentPredicate::from_nbt_value).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        encode_list(&self.0, EnchantmentPredicate::to_nbt_value)
    }
}

impl HashComponent for StoredEnchantmentsPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_nbt_codec(self, hasher);
    }
}

impl_predicate_downcast_type!(
    StoredEnchantmentsPredicate,
    "steel:data_component_predicate/stored_enchantments"
);

/// Accepted registered potion values.
#[derive(Debug, Clone, PartialEq)]
pub struct PotionsPredicate(RegistryHolderSet<Potion>);

impl PotionsPredicate {
    #[must_use]
    pub const fn new(potions: RegistryHolderSet<Potion>) -> Self {
        Self(potions)
    }

    #[must_use]
    pub const fn potions(&self) -> &RegistryHolderSet<Potion> {
        &self.0
    }
}

impl DataComponentPredicateCodec for PotionsPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        RegistryHolderSet::from_owned_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.clone().to_nbt_tag()
    }
}

impl HashComponent for PotionsPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    PotionsPredicate,
    "steel:data_component_predicate/potion_contents"
);

/// Partial custom-data NBT predicate.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomDataPredicate(NbtPredicate);

impl CustomDataPredicate {
    #[must_use]
    pub const fn new(value: NbtPredicate) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(&self) -> &NbtPredicate {
        &self.0
    }
}

impl DataComponentPredicateCodec for CustomDataPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        NbtPredicate::from_owned_nbt(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.to_nbt_tag_ref()
    }
}

impl HashComponent for CustomDataPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    CustomDataPredicate,
    "steel:data_component_predicate/custom_data"
);

/// Nested item predicates over container contents.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerPredicate(Option<CollectionPredicate<ItemPredicate>>);

impl ContainerPredicate {
    #[must_use]
    pub const fn new(items: Option<CollectionPredicate<ItemPredicate>>) -> Self {
        Self(items)
    }

    #[must_use]
    pub const fn items(&self) -> Option<&CollectionPredicate<ItemPredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for ContainerPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "items", |tag| {
            CollectionPredicate::from_nbt_with(tag, ItemPredicate::from_owned_nbt)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(self.0.as_ref(), "items", ItemPredicate::to_nbt_tag_ref)
    }
}

impl HashComponent for ContainerPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "items",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    ContainerPredicate,
    "steel:data_component_predicate/container"
);

/// Nested item predicates over bundle contents.
#[derive(Debug, Clone, PartialEq)]
pub struct BundlePredicate(Option<CollectionPredicate<ItemPredicate>>);

impl BundlePredicate {
    #[must_use]
    pub const fn new(items: Option<CollectionPredicate<ItemPredicate>>) -> Self {
        Self(items)
    }

    #[must_use]
    pub const fn items(&self) -> Option<&CollectionPredicate<ItemPredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for BundlePredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "items", |tag| {
            CollectionPredicate::from_nbt_with(tag, ItemPredicate::from_owned_nbt)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(self.0.as_ref(), "items", ItemPredicate::to_nbt_tag_ref)
    }
}

impl HashComponent for BundlePredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "items",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    BundlePredicate,
    "steel:data_component_predicate/bundle_contents"
);
