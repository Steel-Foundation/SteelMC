use super::*;

/// Predicate for one attribute-modifier entry.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeModifierEntryPredicate {
    attribute: Option<RegistryHolderSet<Attribute>>,
    id: Option<Identifier>,
    amount: DoubleBounds,
    operation: Option<AttributeModifierOperation>,
    slot: Option<EquipmentSlotGroup>,
}

impl AttributeModifierEntryPredicate {
    #[must_use]
    pub const fn new(
        attribute: Option<RegistryHolderSet<Attribute>>,
        id: Option<Identifier>,
        amount: DoubleBounds,
        operation: Option<AttributeModifierOperation>,
        slot: Option<EquipmentSlotGroup>,
    ) -> Self {
        Self {
            attribute,
            id,
            amount,
            operation,
            slot,
        }
    }

    #[must_use]
    pub const fn attribute(&self) -> Option<&RegistryHolderSet<Attribute>> {
        self.attribute.as_ref()
    }

    #[must_use]
    pub const fn id(&self) -> Option<&Identifier> {
        self.id.as_ref()
    }

    #[must_use]
    pub const fn amount(&self) -> DoubleBounds {
        self.amount
    }

    #[must_use]
    pub const fn operation(&self) -> Option<AttributeModifierOperation> {
        self.operation
    }

    #[must_use]
    pub const fn slot(&self) -> Option<EquipmentSlotGroup> {
        self.slot
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            attribute: decode_optional(compound, "attribute", RegistryHolderSet::from_owned_nbt)?,
            id: decode_optional(compound, "id", |tag| owned_string(tag)?.parse().ok())?,
            amount: compound
                .get("amount")
                .map_or(Some(DoubleBounds::ANY), DoubleBounds::from_owned_nbt)?,
            operation: decode_optional(compound, "operation", |tag| {
                AttributeModifierOperation::by_name(&owned_string(tag)?)
            })?,
            slot: decode_optional(compound, "slot", |tag| {
                EquipmentSlotGroup::by_name(&owned_string(tag)?)
            })?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(attribute) = &self.attribute {
            compound.insert("attribute", attribute.clone().to_nbt_tag());
        }
        if let Some(id) = &self.id {
            compound.insert("id", id.to_string());
        }
        if !self.amount.is_any() {
            compound.insert("amount", self.amount.as_nbt_tag());
        }
        if let Some(operation) = self.operation {
            compound.insert("operation", operation.name());
        }
        if let Some(slot) = self.slot {
            compound.insert("slot", slot.name());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for AttributeModifierEntryPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(attribute) = &self.attribute {
            push_hash_entry(&mut entries, "attribute", attribute);
        }
        if let Some(id) = &self.id {
            push_hash_entry(&mut entries, "id", id);
        }
        if !self.amount.is_any() {
            push_hash_entry(&mut entries, "amount", &self.amount);
        }
        if let Some(operation) = self.operation {
            push_hash_entry(&mut entries, "operation", operation.name());
        }
        if let Some(slot) = self.slot {
            push_hash_entry(&mut entries, "slot", slot.name());
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Predicate over item attribute modifiers.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeModifiersPredicate(
    Option<CollectionPredicate<AttributeModifierEntryPredicate>>,
);

impl AttributeModifiersPredicate {
    #[must_use]
    pub const fn new(
        modifiers: Option<CollectionPredicate<AttributeModifierEntryPredicate>>,
    ) -> Self {
        Self(modifiers)
    }

    #[must_use]
    pub const fn modifiers(&self) -> Option<&CollectionPredicate<AttributeModifierEntryPredicate>> {
        self.0.as_ref()
    }
}

impl DataComponentPredicateCodec for AttributeModifiersPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        decode_optional(compound, "modifiers", |tag| {
            CollectionPredicate::from_nbt_with(tag, AttributeModifierEntryPredicate::from_nbt_value)
        })
        .map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        collection_field_nbt(
            self.0.as_ref(),
            "modifiers",
            AttributeModifierEntryPredicate::to_nbt_value,
        )
    }
}

impl HashComponent for AttributeModifiersPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hash_optional_collection_field(
            self.0.as_ref(),
            "modifiers",
            hasher,
            HashComponent::compute_hash,
        );
    }
}

impl_predicate_downcast_type!(
    AttributeModifiersPredicate,
    "steel:data_component_predicate/attribute_modifiers"
);
