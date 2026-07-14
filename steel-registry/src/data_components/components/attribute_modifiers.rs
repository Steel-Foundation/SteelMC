//! Item attribute modifier component.
#![cfg_attr(
    test,
    expect(
        clippy::unwrap_used,
        reason = "attribute modifier tests build in-memory byte buffers with infallible writes"
    )
)]

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::TextComponent;

use crate::attribute::{AttributeModifierOperation, AttributeRef};
use crate::equipment::{EquipmentSlot, EquipmentSlotGroup};
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Vanilla `minecraft:attribute_modifiers` item component.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemAttributeModifiers {
    pub modifiers: Vec<ItemAttributeModifierEntry>,
}

impl ItemAttributeModifiers {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            modifiers: Vec::new(),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.modifiers.is_empty()
    }

    pub fn for_slot(
        &self,
        slot: EquipmentSlot,
    ) -> impl Iterator<Item = &ItemAttributeModifierEntry> {
        self.modifiers
            .iter()
            .filter(move |entry| entry.slot.test(slot))
    }
}

impl Default for ItemAttributeModifiers {
    fn default() -> Self {
        Self::empty()
    }
}

/// A single item attribute modifier entry.
#[derive(Debug, Clone)]
pub struct ItemAttributeModifierEntry {
    pub attribute: AttributeRef,
    pub id: Identifier,
    pub amount: f64,
    pub operation: AttributeModifierOperation,
    pub slot: EquipmentSlotGroup,
    pub display: ItemAttributeModifierDisplay,
}

impl PartialEq for ItemAttributeModifierEntry {
    fn eq(&self, other: &Self) -> bool {
        self.attribute.key == other.attribute.key
            && self.id == other.id
            && self.amount == other.amount
            && self.operation == other.operation
            && self.slot == other.slot
            && self.display == other.display
    }
}

/// Tooltip display behavior for an item attribute modifier.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemAttributeModifierDisplay {
    Default,
    Hidden,
    OverrideText(Box<TextComponent>),
}

impl ItemAttributeModifierDisplay {
    #[must_use]
    pub const fn id(&self) -> i32 {
        match self {
            Self::Default => 0,
            Self::Hidden => 1,
            Self::OverrideText(_) => 2,
        }
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Hidden => "hidden",
            Self::OverrideText(_) => "override",
        }
    }

    fn from_nbt_compound(compound: simdnbt::borrow::NbtCompound) -> Option<Self> {
        let display_type = compound.get("type")?.string()?.to_str();
        match display_type.as_ref() {
            "default" => Some(Self::Default),
            "hidden" => Some(Self::Hidden),
            "override" => {
                let value = compound.get("value")?;
                Some(Self::OverrideText(Box::new(TextComponent::from_nbt_tag(
                    value,
                )?)))
            }
            _ => None,
        }
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("type", self.name());
        if let Self::OverrideText(text) = self {
            compound.insert("value", text.as_ref().to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl WriteTo for ItemAttributeModifiers {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let count = i32::try_from(self.modifiers.len()).map_err(|_| {
            std::io::Error::other(format!(
                "Attribute modifier list too large: {} entries",
                self.modifiers.len()
            ))
        })?;
        VarInt(count).write(writer)?;
        for entry in &self.modifiers {
            entry.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for ItemAttributeModifiers {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = VarInt::read(data)?.0;
        let count = usize::try_from(count).map_err(|_| {
            std::io::Error::other(format!("Negative attribute modifier count: {count}"))
        })?;
        let mut modifiers = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            modifiers.push(ItemAttributeModifierEntry::read(data)?);
        }
        Ok(Self { modifiers })
    }
}

impl WriteTo for ItemAttributeModifierEntry {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let attribute_id = self.attribute.try_id().ok_or_else(|| {
            std::io::Error::other(format!("Unknown attribute: {}", self.attribute.key))
        })?;
        let attribute_id = i32::try_from(attribute_id).map_err(|_| {
            std::io::Error::other(format!(
                "Attribute id out of protocol range: {attribute_id}"
            ))
        })?;
        VarInt(attribute_id).write(writer)?;
        self.id.write(writer)?;
        self.amount.write(writer)?;
        self.operation.write(writer)?;
        VarInt(self.slot.id()).write(writer)?;
        self.display.write(writer)
    }
}

impl ReadFrom for ItemAttributeModifierEntry {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let attribute_id = VarInt::read(data)?.0;
        let attribute_id = usize::try_from(attribute_id)
            .map_err(|_| std::io::Error::other(format!("Negative attribute id: {attribute_id}")))?;
        let attribute = REGISTRY.attributes.by_id(attribute_id).ok_or_else(|| {
            std::io::Error::other(format!("Unknown attribute id: {attribute_id}"))
        })?;
        let id = Identifier::read(data)?;
        let amount = f64::read(data)?;
        let operation = AttributeModifierOperation::read(data)?;
        let slot_id = VarInt::read(data)?.0;
        let slot = EquipmentSlotGroup::by_id(slot_id);
        let display = ItemAttributeModifierDisplay::read(data)?;

        Ok(Self {
            attribute,
            id,
            amount,
            operation,
            slot,
            display,
        })
    }
}

impl WriteTo for ItemAttributeModifierDisplay {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.id()).write(writer)?;
        if let Self::OverrideText(text) = self {
            text.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for ItemAttributeModifierDisplay {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let display_id = VarInt::read(data)?.0;
        match display_id {
            1 => Ok(Self::Hidden),
            2 => Ok(Self::OverrideText(Box::new(TextComponent::read(data)?))),
            _ => Ok(Self::Default),
        }
    }
}

impl ToNbtTag for ItemAttributeModifiers {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::List(NbtList::Compound(
            self.modifiers
                .into_iter()
                .map(ItemAttributeModifierEntry::into_nbt_compound)
                .collect(),
        ))
    }
}

impl ItemAttributeModifierEntry {
    fn into_nbt_compound(self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("type", self.attribute.key.to_string());
        compound.insert("id", self.id.to_string());
        compound.insert("amount", self.amount);
        compound.insert("operation", self.operation.name());
        if self.slot != EquipmentSlotGroup::Any {
            compound.insert("slot", self.slot.name());
        }
        if !matches!(self.display, ItemAttributeModifierDisplay::Default) {
            compound.insert("display", self.display.to_nbt_tag_ref());
        }
        compound
    }
}

impl FromNbtTag for ItemAttributeModifiers {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let entries = tag.list()?.compounds()?;
        let mut modifiers = Vec::with_capacity(entries.len());

        for compound in entries {
            let attribute_key = compound
                .get("type")
                .and_then(|tag| tag.string())
                .and_then(|value| value.to_str().parse::<Identifier>().ok())?;
            let attribute = REGISTRY.attributes.by_key(&attribute_key)?;
            let id = compound
                .get("id")
                .and_then(|tag| tag.string())
                .and_then(|value| value.to_str().parse::<Identifier>().ok())?;
            let amount = compound.get("amount")?.codec_f64()?;
            let operation = compound
                .get("operation")
                .and_then(|tag| tag.string())
                .and_then(|value| AttributeModifierOperation::by_name(value.to_str().as_ref()))?;
            let slot = match compound.get("slot") {
                Some(tag) => EquipmentSlotGroup::by_name(tag.string()?.to_str().as_ref())?,
                None => EquipmentSlotGroup::Any,
            };
            let display = match compound.get("display") {
                Some(tag) => ItemAttributeModifierDisplay::from_nbt_compound(tag.compound()?)?,
                None => ItemAttributeModifierDisplay::Default,
            };

            modifiers.push(ItemAttributeModifierEntry {
                attribute,
                id,
                amount,
                operation,
                slot,
                display,
            });
        }

        Some(Self { modifiers })
    }
}

impl HashComponent for ItemAttributeModifiers {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for entry in &self.modifiers {
            hasher.put_component_hash(entry);
        }
        hasher.end_list();
    }
}

impl HashComponent for ItemAttributeModifierEntry {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "type", &self.attribute.key.to_string());
        push_hash_entry(&mut entries, "id", &self.id.to_string());
        push_hash_entry(&mut entries, "amount", &self.amount);
        push_hash_entry(&mut entries, "operation", self.operation.name());
        if self.slot != EquipmentSlotGroup::Any {
            push_hash_entry(&mut entries, "slot", self.slot.name());
        }
        if !matches!(self.display, ItemAttributeModifierDisplay::Default) {
            push_hash_entry(&mut entries, "display", &self.display);
        }

        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

impl HashComponent for ItemAttributeModifierDisplay {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "type", self.name());
        if let Self::OverrideText(text) = self {
            push_hash_entry(&mut entries, "value", text.as_ref());
        }

        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
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

    use simdnbt::FromNbtTag;
    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_utils::Identifier;
    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom, WriteTo};

    use crate::attribute::AttributeModifierOperation;
    use crate::equipment::{EquipmentSlot, EquipmentSlotGroup};
    use crate::item_stack::ItemStack;
    use crate::{
        RegistryEntry, test_support::init_test_registry, vanilla_attributes, vanilla_items::ITEMS,
    };

    use super::{ItemAttributeModifierDisplay, ItemAttributeModifierEntry, ItemAttributeModifiers};

    fn with_borrowed_tag<R>(tag: NbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    fn modifier_nbt() -> NbtCompound {
        let mut modifier = NbtCompound::new();
        modifier.insert("type", "minecraft:armor");
        modifier.insert("id", "minecraft:test");
        modifier.insert("amount", 1_i32);
        modifier.insert("operation", "add_value");
        modifier
    }

    #[test]
    fn generated_diamond_sword_has_main_hand_attack_modifiers() {
        init_test_registry();

        let stack = ItemStack::new(&ITEMS.diamond_sword);
        let modifiers = stack
            .get_attribute_modifiers()
            .expect("diamond sword should have attribute modifiers");
        let main_hand_modifiers = modifiers
            .for_slot(EquipmentSlot::MainHand)
            .collect::<Vec<_>>();

        assert_eq!(main_hand_modifiers.len(), 2);
        assert!(main_hand_modifiers.iter().any(|modifier| {
            modifier.attribute.key == vanilla_attributes::ATTACK_DAMAGE.key
                && modifier.id == Identifier::vanilla_static("base_attack_damage")
                && modifier.amount.to_bits() == 6.0f64.to_bits()
                && modifier.operation == AttributeModifierOperation::AddValue
        }));
        assert!(main_hand_modifiers.iter().any(|modifier| {
            modifier.attribute.key == vanilla_attributes::ATTACK_SPEED.key
                && modifier.id == Identifier::vanilla_static("base_attack_speed")
                && modifier.amount.to_bits() == (-2.4000000953674316f64).to_bits()
                && modifier.operation == AttributeModifierOperation::AddValue
        }));
        assert!(modifiers.for_slot(EquipmentSlot::Head).next().is_none());
    }

    #[test]
    fn generated_carved_pumpkin_has_hidden_head_modifier() {
        init_test_registry();

        let stack = ItemStack::new(&ITEMS.carved_pumpkin);
        let modifiers = stack
            .get_attribute_modifiers()
            .expect("carved pumpkin should have attribute modifiers");
        let head_modifiers = modifiers.for_slot(EquipmentSlot::Head).collect::<Vec<_>>();

        assert_eq!(head_modifiers.len(), 1);
        let modifier = head_modifiers[0];
        assert_eq!(
            modifier.attribute.key,
            vanilla_attributes::WAYPOINT_TRANSMIT_RANGE.key
        );
        assert_eq!(
            modifier.id,
            Identifier::vanilla_static("waypoint_transmit_range_hide")
        );
        assert_eq!(
            modifier.operation,
            AttributeModifierOperation::AddMultipliedTotal
        );
        assert_eq!(modifier.display, ItemAttributeModifierDisplay::Hidden);
    }

    #[test]
    fn unknown_attribute_modifier_slot_group_id_falls_back_to_any() {
        init_test_registry();

        let mut bytes = Vec::new();
        let Some(attribute_id) = vanilla_attributes::ARMOR.try_id() else {
            panic!("armor attribute should be registered");
        };
        VarInt(attribute_id as i32).write(&mut bytes).unwrap();
        Identifier::vanilla_static("test")
            .write(&mut bytes)
            .unwrap();
        1.0_f64.write(&mut bytes).unwrap();
        AttributeModifierOperation::AddValue
            .write(&mut bytes)
            .unwrap();
        VarInt(999).write(&mut bytes).unwrap();
        ItemAttributeModifierDisplay::Default
            .write(&mut bytes)
            .unwrap();

        let entry = ItemAttributeModifierEntry::read(&mut Cursor::new(bytes.as_slice()))
            .expect("unknown slot group id should fall back to any");

        assert_eq!(entry.slot, EquipmentSlotGroup::Any);
    }

    #[test]
    fn unknown_attribute_modifier_display_id_falls_back_to_default() {
        let display = ItemAttributeModifierDisplay::read(&mut Cursor::new(&[99][..]))
            .expect("unknown display id should fall back to default");

        assert_eq!(display, ItemAttributeModifierDisplay::Default);
    }

    #[test]
    fn attribute_modifier_nbt_coerces_amount_and_rejects_invalid_optional_fields() {
        init_test_registry();
        let parsed = with_borrowed_tag(
            NbtTag::List(NbtList::Compound(vec![modifier_nbt()])),
            ItemAttributeModifiers::from_nbt_tag,
        )
        .expect("integer amount should decode through Codec.DOUBLE");
        assert_eq!(parsed.modifiers[0].amount, 1.0);

        let mut malformed = modifier_nbt();
        malformed.insert("slot", 1);
        assert!(
            with_borrowed_tag(
                NbtTag::List(NbtList::Compound(vec![malformed])),
                ItemAttributeModifiers::from_nbt_tag,
            )
            .is_none()
        );
    }
}
