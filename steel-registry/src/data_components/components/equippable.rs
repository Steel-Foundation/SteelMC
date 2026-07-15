//! Equippable component for armor and equipment items.

use std::io::{Cursor, Result, Write};
use std::str::FromStr;

use crate::{
    RegistryHolderSet,
    entity_type::{EntityType, EntityTypeRef},
    equipment::EquipmentSlot,
    sound_event::SoundEventHolder,
    sound_events,
};
use steel_utils::{
    Identifier,
    codec::VarInt,
    hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries},
    nbt::NbtNumeric as _,
    serial::{ReadFrom, WriteTo},
};

/// Entity types allowed to equip an item.
pub type EquippableAllowedEntities = RegistryHolderSet<EntityType>;

/// The equippable component data.
#[derive(Debug, Clone, PartialEq)]
pub struct Equippable {
    pub slot: EquipmentSlot,
    pub equip_sound: SoundEventHolder,
    pub asset_id: Option<Identifier>,
    pub camera_overlay: Option<Identifier>,
    pub allowed_entities: Option<EquippableAllowedEntities>,
    pub dispensable: bool,
    pub swappable: bool,
    pub damage_on_hurt: bool,
    pub equip_on_interact: bool,
    pub can_be_sheared: bool,
    pub shearing_sound: SoundEventHolder,
}

impl Equippable {
    /// Returns whether this item can be equipped by the entity type.
    #[must_use]
    pub fn can_be_equipped_by(&self, entity_type: EntityTypeRef) -> bool {
        self.allowed_entities
            .as_ref()
            .is_none_or(|allowed| allowed.contains(entity_type))
    }
}

impl WriteTo for Equippable {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.slot.id()).write(writer)?;
        self.equip_sound.write(writer)?;
        self.asset_id.write(writer)?;
        self.camera_overlay.write(writer)?;
        self.allowed_entities.write(writer)?;
        self.dispensable.write(writer)?;
        self.swappable.write(writer)?;
        self.damage_on_hurt.write(writer)?;
        self.equip_on_interact.write(writer)?;
        self.can_be_sheared.write(writer)?;
        self.shearing_sound.write(writer)?;
        Ok(())
    }
}

impl ReadFrom for Equippable {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let slot_id = VarInt::read(data)?.0;
        Ok(Self {
            slot: EquipmentSlot::by_id(slot_id),
            equip_sound: SoundEventHolder::read(data)?,
            asset_id: Option::<Identifier>::read(data)?,
            camera_overlay: Option::<Identifier>::read(data)?,
            allowed_entities: Option::<EquippableAllowedEntities>::read(data)?,
            dispensable: bool::read(data)?,
            swappable: bool::read(data)?,
            damage_on_hurt: bool::read(data)?,
            equip_on_interact: bool::read(data)?,
            can_be_sheared: bool::read(data)?,
            shearing_sound: SoundEventHolder::read(data)?,
        })
    }
}

impl HashComponent for Equippable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "slot", self.slot.name());
        if self.equip_sound != SoundEventHolder::registry(&sound_events::ITEM_ARMOR_EQUIP_GENERIC) {
            push_hash_entry(&mut entries, "equip_sound", &self.equip_sound);
        }
        if let Some(asset_id) = &self.asset_id {
            push_hash_entry(&mut entries, "asset_id", &asset_id.to_string());
        }
        if let Some(camera_overlay) = &self.camera_overlay {
            push_hash_entry(&mut entries, "camera_overlay", &camera_overlay.to_string());
        }
        if let Some(allowed_entities) = &self.allowed_entities {
            push_hash_entry(&mut entries, "allowed_entities", allowed_entities);
        }
        if !self.dispensable {
            push_hash_entry(&mut entries, "dispensable", &self.dispensable);
        }
        if !self.swappable {
            push_hash_entry(&mut entries, "swappable", &self.swappable);
        }
        if !self.damage_on_hurt {
            push_hash_entry(&mut entries, "damage_on_hurt", &self.damage_on_hurt);
        }
        if self.equip_on_interact {
            push_hash_entry(&mut entries, "equip_on_interact", &self.equip_on_interact);
        }
        if self.can_be_sheared {
            push_hash_entry(&mut entries, "can_be_sheared", &self.can_be_sheared);
        }
        if self.shearing_sound != SoundEventHolder::registry(&sound_events::ITEM_SHEARS_SNIP) {
            push_hash_entry(&mut entries, "shearing_sound", &self.shearing_sound);
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

impl simdnbt::ToNbtTag for Equippable {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};

        let mut compound = NbtCompound::new();
        compound.insert("slot", self.slot.name());
        if self.equip_sound != SoundEventHolder::registry(&sound_events::ITEM_ARMOR_EQUIP_GENERIC) {
            compound.insert("equip_sound", self.equip_sound.to_nbt_tag());
        }
        if let Some(asset_id) = self.asset_id {
            compound.insert("asset_id", asset_id.to_string());
        }
        if let Some(camera_overlay) = self.camera_overlay {
            compound.insert("camera_overlay", camera_overlay.to_string());
        }
        if !self.dispensable {
            compound.insert("dispensable", i8::from(self.dispensable));
        }
        if !self.swappable {
            compound.insert("swappable", i8::from(self.swappable));
        }
        if !self.damage_on_hurt {
            compound.insert("damage_on_hurt", i8::from(self.damage_on_hurt));
        }
        if self.equip_on_interact {
            compound.insert("equip_on_interact", i8::from(self.equip_on_interact));
        }
        if self.can_be_sheared {
            compound.insert("can_be_sheared", i8::from(self.can_be_sheared));
        }
        if self.shearing_sound != SoundEventHolder::registry(&sound_events::ITEM_SHEARS_SNIP) {
            compound.insert("shearing_sound", self.shearing_sound.to_nbt_tag());
        }
        if let Some(allowed_entities) = self.allowed_entities {
            compound.insert("allowed_entities", allowed_entities.to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl simdnbt::FromNbtTag for Equippable {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let slot_str = compound.get("slot")?.string()?.to_str();
        let slot = EquipmentSlot::by_name(&slot_str)?;
        let equip_sound = match compound.get("equip_sound") {
            Some(tag) => SoundEventHolder::from_nbt_tag(tag)?,
            None => SoundEventHolder::registry(&sound_events::ITEM_ARMOR_EQUIP_GENERIC),
        };
        let asset_id = match compound.get("asset_id") {
            Some(tag) => Some(parse_identifier_nbt(tag)?),
            None => None,
        };
        let camera_overlay = match compound.get("camera_overlay") {
            Some(tag) => Some(parse_identifier_nbt(tag)?),
            None => None,
        };
        let allowed_entities = match compound.get("allowed_entities") {
            Some(tag) => Some(EquippableAllowedEntities::from_nbt_tag(tag)?),
            None => None,
        };
        let dispensable = optional_bool(compound.get("dispensable"), true)?;
        let swappable = optional_bool(compound.get("swappable"), true)?;
        let damage_on_hurt = optional_bool(compound.get("damage_on_hurt"), true)?;
        let equip_on_interact = optional_bool(compound.get("equip_on_interact"), false)?;
        let can_be_sheared = optional_bool(compound.get("can_be_sheared"), false)?;
        let shearing_sound = match compound.get("shearing_sound") {
            Some(tag) => SoundEventHolder::from_nbt_tag(tag)?,
            None => SoundEventHolder::registry(&sound_events::ITEM_SHEARS_SNIP),
        };

        Some(Self {
            slot,
            equip_sound,
            asset_id,
            camera_overlay,
            allowed_entities,
            dispensable,
            swappable,
            damage_on_hurt,
            equip_on_interact,
            can_be_sheared,
            shearing_sound,
        })
    }
}

fn parse_identifier_nbt(tag: simdnbt::borrow::NbtTag) -> Option<Identifier> {
    Identifier::from_str(&tag.string()?.to_str()).ok()
}

fn optional_bool(tag: Option<simdnbt::borrow::NbtTag<'_, '_>>, default: bool) -> Option<bool> {
    match tag {
        Some(tag) => tag.codec_bool(),
        None => Some(default),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Equippable, EquippableAllowedEntities};
    use crate::data_components::{ComponentData, vanilla_components::EQUIPPABLE};
    use crate::item_stack::ItemStack;
    use crate::sound_event::SoundEventHolder;
    use crate::sound_events;
    use crate::test_support::init_test_registry;
    use crate::vanilla_entities::{LLAMA, PIG, PLAYER, WOLF};
    use crate::vanilla_entity_type_tags::EntityTypeTag;
    use crate::vanilla_items;
    use crate::{REGISTRY, RegistryExt};
    use simdnbt::FromNbtTag;
    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::Identifier;
    use steel_utils::serial::{ReadFrom, WriteTo};

    fn with_borrowed_tag<R>(tag: NbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    fn round_trip_equippable(equippable: &Equippable) -> Equippable {
        let mut bytes = Vec::new();
        equippable
            .write(&mut bytes)
            .expect("equippable should serialize");
        Equippable::read(&mut Cursor::new(bytes.as_slice())).expect("equippable should deserialize")
    }

    #[test]
    fn extracted_equippable_fields_gate_swapping_and_entity_types() {
        init_test_registry();

        let pumpkin = ItemStack::new(&vanilla_items::CARVED_PUMPKIN);
        let Some(pumpkin_equippable) = pumpkin.get_equippable() else {
            panic!("carved pumpkin should have equippable data");
        };
        assert!(!pumpkin_equippable.swappable);
        assert!(pumpkin_equippable.dispensable);
        assert_eq!(
            pumpkin_equippable.camera_overlay.as_ref(),
            Some(&Identifier::vanilla_static("misc/pumpkinblur"))
        );

        let helmet = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
        let Some(helmet_equippable) = helmet.get_equippable() else {
            panic!("diamond helmet should have equippable data");
        };
        assert!(helmet_equippable.dispensable);
        assert!(helmet_equippable.swappable);
        assert!(helmet_equippable.damage_on_hurt);
        assert!(!helmet_equippable.can_be_sheared);
        assert_eq!(
            helmet_equippable.equip_sound,
            SoundEventHolder::registry(&sound_events::ITEM_ARMOR_EQUIP_DIAMOND)
        );
        assert_eq!(
            helmet_equippable.asset_id.as_ref(),
            Some(&Identifier::vanilla_static("diamond"))
        );
        assert!(helmet_equippable.can_be_equipped_by(&PLAYER));

        let saddle = ItemStack::new(&vanilla_items::SADDLE);
        let Some(saddle_equippable) = saddle.get_equippable() else {
            panic!("saddle should have equippable data");
        };
        assert!(saddle_equippable.dispensable);
        assert!(saddle_equippable.equip_on_interact);
        assert!(saddle_equippable.can_be_sheared);
        assert_eq!(
            saddle_equippable.shearing_sound,
            SoundEventHolder::registry(&sound_events::ITEM_SADDLE_UNEQUIP)
        );
        assert_eq!(
            saddle_equippable.asset_id.as_ref(),
            Some(&Identifier::vanilla_static("saddle"))
        );
        assert_eq!(
            saddle_equippable.allowed_entities,
            Some(EquippableAllowedEntities::Tag(
                EntityTypeTag::CAN_EQUIP_SADDLE
            ))
        );

        let carpet = ItemStack::new(&vanilla_items::WHITE_CARPET);
        let Some(carpet_equippable) = carpet.get_equippable() else {
            panic!("carpet should have equippable data");
        };
        assert!(carpet_equippable.can_be_sheared);
        assert_eq!(
            carpet_equippable.shearing_sound,
            SoundEventHolder::registry(&sound_events::ITEM_LLAMA_CARPET_UNEQUIP)
        );
        assert!(carpet_equippable.can_be_equipped_by(&LLAMA));
        assert!(!carpet_equippable.can_be_equipped_by(&PIG));
        assert!(!carpet_equippable.can_be_equipped_by(&PLAYER));

        let wolf_armor = ItemStack::new(&vanilla_items::WOLF_ARMOR);
        let Some(wolf_armor_equippable) = wolf_armor.get_equippable() else {
            panic!("wolf armor should have equippable data");
        };
        assert!(wolf_armor_equippable.can_be_equipped_by(&WOLF));
        assert!(!wolf_armor_equippable.can_be_equipped_by(&PLAYER));
    }

    #[test]
    fn equippable_network_round_trips_tag_and_direct_holder_sets() {
        init_test_registry();

        let saddle = ItemStack::new(&vanilla_items::SADDLE);
        let Some(saddle_equippable) = saddle.get_equippable() else {
            panic!("saddle should have equippable data");
        };
        assert_eq!(&round_trip_equippable(saddle_equippable), saddle_equippable);

        let carpet = ItemStack::new(&vanilla_items::WHITE_CARPET);
        let Some(carpet_equippable) = carpet.get_equippable() else {
            panic!("carpet should have equippable data");
        };
        assert_eq!(&round_trip_equippable(carpet_equippable), carpet_equippable);
    }

    #[test]
    fn equippable_hash_includes_vanilla_codec_fields() {
        init_test_registry();

        let saddle = ItemStack::new(&vanilla_items::SADDLE);
        let Some(saddle_equippable) = saddle.get_equippable() else {
            panic!("saddle should have equippable data");
        };
        let helmet = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
        let Some(helmet_equippable) = helmet.get_equippable() else {
            panic!("diamond helmet should have equippable data");
        };

        let component_type = REGISTRY
            .data_components
            .by_key(&EQUIPPABLE.key)
            .expect("equippable component should be registered");
        let saddle_hash = component_type
            .compute_hash(&ComponentData::new(saddle_equippable.clone()))
            .expect("equippable should have a persistent hash codec");
        let helmet_hash = component_type
            .compute_hash(&ComponentData::new(helmet_equippable.clone()))
            .expect("equippable should have a persistent hash codec");
        assert_ne!(saddle_hash, helmet_hash);
    }

    #[test]
    fn equippable_nbt_defaults_only_missing_fields() {
        init_test_registry();
        let mut compound = NbtCompound::new();
        compound.insert("slot", "head");
        compound.insert("dispensable", 0_i32);
        let equippable = with_borrowed_tag(NbtTag::Compound(compound), Equippable::from_nbt_tag)
            .expect("numeric boolean should parse");
        assert!(!equippable.dispensable);
        assert!(equippable.swappable);

        let mut malformed = NbtCompound::new();
        malformed.insert("slot", "head");
        malformed.insert("camera_overlay", 1);
        assert!(with_borrowed_tag(NbtTag::Compound(malformed), Equippable::from_nbt_tag).is_none());

        let mut unknown_tag = NbtCompound::new();
        unknown_tag.insert("slot", "head");
        unknown_tag.insert("allowed_entities", "#minecraft:not_a_tag");
        assert!(
            with_borrowed_tag(NbtTag::Compound(unknown_tag), Equippable::from_nbt_tag).is_none()
        );
    }
}
