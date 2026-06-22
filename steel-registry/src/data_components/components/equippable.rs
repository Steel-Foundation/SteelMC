//! Equippable component for armor and equipment items.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use crate::{
    REGISTRY, RegistryEntry, RegistryExt, TaggedRegistryExt, entity_type::EntityTypeRef,
    equipment::EquipmentSlot, sound_event::SoundEventHolder, sound_events,
};
use steel_utils::{
    Identifier,
    codec::VarInt,
    hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries},
    serial::{ReadFrom, WriteTo},
};

/// Entity types allowed to equip an item.
#[derive(Debug, Clone, PartialEq)]
pub enum EquippableAllowedEntities {
    /// A tag of entity types, such as `minecraft:can_equip_saddle`.
    Tag(Identifier),
    /// Direct entity type references.
    EntityTypes(Vec<EntityTypeRef>),
}

impl EquippableAllowedEntities {
    /// Returns whether this holder set contains the entity type.
    #[must_use]
    pub fn contains(&self, entity_type: EntityTypeRef) -> bool {
        match self {
            Self::Tag(tag) => REGISTRY.entity_types.is_in_tag(entity_type, tag),
            Self::EntityTypes(entity_types) => entity_types.contains(&entity_type),
        }
    }
}

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
        write_allowed_entities(writer, &self.allowed_entities)?;
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
            allowed_entities: read_allowed_entities(data)?,
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

impl HashComponent for EquippableAllowedEntities {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        match self {
            Self::Tag(tag) => hasher.put_string(&format!("#{tag}")),
            Self::EntityTypes(entity_types) => {
                hasher.start_list();
                for entity_type in entity_types {
                    hasher.put_string(&entity_type.key.to_string());
                }
                hasher.end_list();
            }
        }
    }
}

fn write_allowed_entities(
    writer: &mut impl Write,
    allowed_entities: &Option<EquippableAllowedEntities>,
) -> Result<()> {
    let Some(allowed_entities) = allowed_entities else {
        return false.write(writer);
    };

    true.write(writer)?;
    match allowed_entities {
        EquippableAllowedEntities::Tag(tag) => {
            VarInt(0).write(writer)?;
            tag.write(writer)
        }
        EquippableAllowedEntities::EntityTypes(entity_types) => {
            let len = i32::try_from(entity_types.len()).map_err(|_| {
                Error::other(format!(
                    "Allowed entity holder set too large: {}",
                    entity_types.len()
                ))
            })?;
            VarInt(len + 1).write(writer)?;
            for entity_type in entity_types {
                let id = entity_type.try_id().ok_or_else(|| {
                    Error::other(format!("Unknown entity type: {}", entity_type.key))
                })?;
                let id = i32::try_from(id)
                    .map_err(|_| Error::other(format!("Entity type id out of range: {id}")))?;
                VarInt(id).write(writer)?;
            }
            Ok(())
        }
    }
}

fn read_allowed_entities(data: &mut Cursor<&[u8]>) -> Result<Option<EquippableAllowedEntities>> {
    if !bool::read(data)? {
        return Ok(None);
    }

    let encoded_count = VarInt::read(data)?.0;
    if encoded_count == 0 {
        return Ok(Some(EquippableAllowedEntities::Tag(Identifier::read(
            data,
        )?)));
    }
    if encoded_count < 0 {
        return Err(Error::other(format!(
            "Negative allowed entity holder set count: {encoded_count}"
        )));
    }

    let count = encoded_count - 1;
    if count > 4096 {
        return Err(Error::other(format!(
            "Allowed entity holder set count out of range: {count}"
        )));
    }

    let mut entity_types = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let id = VarInt::read(data)?.0;
        if id < 0 {
            return Err(Error::other(format!("Negative entity type id: {id}")));
        }
        let entity_type = REGISTRY
            .entity_types
            .by_id(id as usize)
            .ok_or_else(|| Error::other(format!("Unknown entity type id: {id}")))?;
        entity_types.push(entity_type);
    }
    Ok(Some(EquippableAllowedEntities::EntityTypes(entity_types)))
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
        compound.insert("equip_sound", self.equip_sound.to_nbt_tag());
        if let Some(asset_id) = self.asset_id {
            compound.insert("asset_id", asset_id.to_string());
        }
        if let Some(camera_overlay) = self.camera_overlay {
            compound.insert("camera_overlay", camera_overlay.to_string());
        }
        compound.insert("dispensable", i8::from(self.dispensable));
        compound.insert("swappable", i8::from(self.swappable));
        compound.insert("damage_on_hurt", i8::from(self.damage_on_hurt));
        compound.insert("equip_on_interact", i8::from(self.equip_on_interact));
        compound.insert("can_be_sheared", i8::from(self.can_be_sheared));
        compound.insert("shearing_sound", self.shearing_sound.to_nbt_tag());
        if let Some(allowed_entities) = self.allowed_entities {
            match allowed_entities {
                EquippableAllowedEntities::Tag(tag) => {
                    compound.insert("allowed_entities", format!("#{tag}"));
                }
                EquippableAllowedEntities::EntityTypes(entity_types) => {
                    let values: Vec<NbtTag> = entity_types
                        .into_iter()
                        .map(|entity_type| NbtTag::String(entity_type.key.to_string().into()))
                        .collect();
                    compound.insert(
                        "allowed_entities",
                        simdnbt::owned::NbtList::String(
                            values
                                .into_iter()
                                .filter_map(|value| match value {
                                    NbtTag::String(value) => Some(value),
                                    _ => None,
                                })
                                .collect(),
                        ),
                    );
                }
            }
        }
        NbtTag::Compound(compound)
    }
}

impl simdnbt::FromNbtTag for Equippable {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let slot_str = compound.get("slot")?.string()?.to_str();
        let slot = EquipmentSlot::by_name(&slot_str)?;
        let equip_sound = compound
            .get("equip_sound")
            .and_then(SoundEventHolder::from_nbt_tag)
            .unwrap_or_else(|| SoundEventHolder::registry(&sound_events::ITEM_ARMOR_EQUIP_GENERIC));
        let asset_id = compound.get("asset_id").and_then(parse_identifier_nbt);
        let camera_overlay = compound
            .get("camera_overlay")
            .and_then(parse_identifier_nbt);
        let allowed_entities = compound
            .get("allowed_entities")
            .and_then(parse_allowed_entities_nbt);
        let dispensable = compound
            .get("dispensable")
            .and_then(|tag| tag.byte())
            .map(|value| value != 0)
            .unwrap_or(true);
        let swappable = compound
            .get("swappable")
            .and_then(|tag| tag.byte())
            .map(|value| value != 0)
            .unwrap_or(true);
        let damage_on_hurt = compound
            .get("damage_on_hurt")
            .and_then(|tag| tag.byte())
            .map(|value| value != 0)
            .unwrap_or(true);
        let equip_on_interact = compound
            .get("equip_on_interact")
            .and_then(|tag| tag.byte())
            .map(|value| value != 0)
            .unwrap_or(false);
        let can_be_sheared = compound
            .get("can_be_sheared")
            .and_then(|tag| tag.byte())
            .map(|value| value != 0)
            .unwrap_or(false);
        let shearing_sound = compound
            .get("shearing_sound")
            .and_then(SoundEventHolder::from_nbt_tag)
            .unwrap_or_else(|| SoundEventHolder::registry(&sound_events::ITEM_SHEARS_SNIP));

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

fn parse_allowed_entities_nbt(tag: simdnbt::borrow::NbtTag) -> Option<EquippableAllowedEntities> {
    if let Some(value) = tag.string() {
        return parse_allowed_entities_string(&value.to_str());
    }

    let list = tag.list()?;
    let strings = list.strings()?;
    let mut entity_types = Vec::new();
    for value in strings {
        let id = Identifier::from_str(&value.to_str()).ok()?;
        entity_types.push(REGISTRY.entity_types.by_key(&id)?);
    }

    Some(EquippableAllowedEntities::EntityTypes(entity_types))
}

fn parse_allowed_entities_string(value: &str) -> Option<EquippableAllowedEntities> {
    if let Some(tag) = value.strip_prefix('#') {
        return Identifier::from_str(tag)
            .ok()
            .map(EquippableAllowedEntities::Tag);
    }

    let id = Identifier::from_str(value).ok()?;
    let entity_type = REGISTRY.entity_types.by_key(&id)?;
    Some(EquippableAllowedEntities::EntityTypes(vec![entity_type]))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Equippable, EquippableAllowedEntities};
    use crate::data_components::ComponentData;
    use crate::item_stack::ItemStack;
    use crate::sound_event::SoundEventHolder;
    use crate::sound_events;
    use crate::test_support::init_test_registry;
    use crate::vanilla_entities::{LLAMA, PIG, PLAYER, WOLF};
    use crate::vanilla_entity_type_tags::EntityTypeTag;
    use crate::vanilla_items::ITEMS;
    use steel_utils::Identifier;
    use steel_utils::serial::{ReadFrom, WriteTo};

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

        let pumpkin = ItemStack::new(&ITEMS.carved_pumpkin);
        let Some(pumpkin_equippable) = pumpkin.get_equippable() else {
            panic!("carved pumpkin should have equippable data");
        };
        assert!(!pumpkin_equippable.swappable);
        assert!(pumpkin_equippable.dispensable);
        assert_eq!(
            pumpkin_equippable.camera_overlay.as_ref(),
            Some(&Identifier::vanilla_static("misc/pumpkinblur"))
        );

        let helmet = ItemStack::new(&ITEMS.diamond_helmet);
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

        let saddle = ItemStack::new(&ITEMS.saddle);
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

        let carpet = ItemStack::new(&ITEMS.white_carpet);
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

        let wolf_armor = ItemStack::new(&ITEMS.wolf_armor);
        let Some(wolf_armor_equippable) = wolf_armor.get_equippable() else {
            panic!("wolf armor should have equippable data");
        };
        assert!(wolf_armor_equippable.can_be_equipped_by(&WOLF));
        assert!(!wolf_armor_equippable.can_be_equipped_by(&PLAYER));
    }

    #[test]
    fn equippable_network_round_trips_tag_and_direct_holder_sets() {
        init_test_registry();

        let saddle = ItemStack::new(&ITEMS.saddle);
        let Some(saddle_equippable) = saddle.get_equippable() else {
            panic!("saddle should have equippable data");
        };
        assert_eq!(&round_trip_equippable(saddle_equippable), saddle_equippable);

        let carpet = ItemStack::new(&ITEMS.white_carpet);
        let Some(carpet_equippable) = carpet.get_equippable() else {
            panic!("carpet should have equippable data");
        };
        assert_eq!(&round_trip_equippable(carpet_equippable), carpet_equippable);
    }

    #[test]
    fn equippable_hash_includes_vanilla_codec_fields() {
        init_test_registry();

        let saddle = ItemStack::new(&ITEMS.saddle);
        let Some(saddle_equippable) = saddle.get_equippable() else {
            panic!("saddle should have equippable data");
        };
        let helmet = ItemStack::new(&ITEMS.diamond_helmet);
        let Some(helmet_equippable) = helmet.get_equippable() else {
            panic!("diamond helmet should have equippable data");
        };

        let saddle_hash = ComponentData::Equippable(saddle_equippable.clone()).compute_hash();
        let helmet_hash = ComponentData::Equippable(helmet_equippable.clone()).compute_hash();
        assert_ne!(saddle_hash, helmet_hash);
    }
}
