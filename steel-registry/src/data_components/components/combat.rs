//! Combat-related item components.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::damage_type::DamageTypeRef;
use crate::sound_event::SoundEventHolder;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

#[derive(Debug, Clone, PartialEq)]
pub struct DamageTypeComponent {
    pub damage_type: DamageTypeRef,
}

impl DamageTypeComponent {
    #[must_use]
    pub const fn new(damage_type: DamageTypeRef) -> Self {
        Self { damage_type }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    pub item_damage_per_attack: i32,
    pub disable_blocking_for_seconds: f32,
}

impl Default for Weapon {
    fn default() -> Self {
        Self {
            item_damage_per_attack: 1,
            disable_blocking_for_seconds: 0.0,
        }
    }
}

impl WriteTo for DamageTypeComponent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self.damage_type.try_id().ok_or_else(|| {
            Error::other(format!("Unknown damage type: {}", self.damage_type.key))
        })?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Damage type id out of protocol range: {id}")))?;
        VarInt(id).write(writer)
    }
}

impl ReadFrom for DamageTypeComponent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative damage type id: {id}")))?;
        let damage_type = REGISTRY
            .damage_types
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown damage type id: {id}")))?;
        Ok(Self { damage_type })
    }
}

impl ToNbtTag for DamageTypeComponent {
    fn to_nbt_tag(self) -> NbtTag {
        self.damage_type.key.to_string().to_nbt_tag()
    }
}

impl FromNbtTag for DamageTypeComponent {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let id = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        REGISTRY
            .damage_types
            .by_key(&id)
            .map(|damage_type| Self { damage_type })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttackRange {
    pub min_reach: f32,
    pub max_reach: f32,
    pub min_creative_reach: f32,
    pub max_creative_reach: f32,
    pub hitbox_margin: f32,
    pub mob_factor: f32,
}

impl Default for AttackRange {
    fn default() -> Self {
        Self {
            min_reach: 0.0,
            max_reach: 3.0,
            min_creative_reach: 0.0,
            max_creative_reach: 5.0,
            hitbox_margin: 0.3,
            mob_factor: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PiercingWeapon {
    pub deals_knockback: bool,
    pub dismounts: bool,
    pub sound: Option<SoundEventHolder>,
    pub hit_sound: Option<SoundEventHolder>,
}

impl Default for PiercingWeapon {
    fn default() -> Self {
        Self {
            deals_knockback: true,
            dismounts: false,
            sound: None,
            hit_sound: None,
        }
    }
}

impl WriteTo for Weapon {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.item_damage_per_attack).write(writer)?;
        self.disable_blocking_for_seconds.write(writer)
    }
}

impl ReadFrom for Weapon {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            item_damage_per_attack: VarInt::read(data)?.0,
            disable_blocking_for_seconds: f32::read(data)?,
        })
    }
}

impl WriteTo for AttackRange {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.min_reach.write(writer)?;
        self.max_reach.write(writer)?;
        self.min_creative_reach.write(writer)?;
        self.max_creative_reach.write(writer)?;
        self.hitbox_margin.write(writer)?;
        self.mob_factor.write(writer)
    }
}

impl ReadFrom for AttackRange {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            min_reach: f32::read(data)?,
            max_reach: f32::read(data)?,
            min_creative_reach: f32::read(data)?,
            max_creative_reach: f32::read(data)?,
            hitbox_margin: f32::read(data)?,
            mob_factor: f32::read(data)?,
        })
    }
}

impl WriteTo for PiercingWeapon {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.deals_knockback.write(writer)?;
        self.dismounts.write(writer)?;
        self.sound.write(writer)?;
        self.hit_sound.write(writer)
    }
}

impl ReadFrom for PiercingWeapon {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            deals_knockback: bool::read(data)?,
            dismounts: bool::read(data)?,
            sound: Option::<SoundEventHolder>::read(data)?,
            hit_sound: Option::<SoundEventHolder>::read(data)?,
        })
    }
}

impl ToNbtTag for Weapon {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if self.item_damage_per_attack != 1 {
            compound.insert("item_damage_per_attack", self.item_damage_per_attack);
        }
        if self.disable_blocking_for_seconds.to_bits() != 0.0_f32.to_bits() {
            compound.insert(
                "disable_blocking_for_seconds",
                self.disable_blocking_for_seconds,
            );
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for Weapon {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let item_damage_per_attack = match compound.get("item_damage_per_attack") {
            Some(tag) => tag.codec_i32()?,
            None => 1,
        };
        if item_damage_per_attack < 0 {
            return None;
        }
        let disable_blocking_for_seconds = optional_ranged_f32(
            compound.get("disable_blocking_for_seconds"),
            0.0,
            0.0,
            f32::MAX,
        )?;
        Some(Self {
            item_damage_per_attack,
            disable_blocking_for_seconds,
        })
    }
}

impl ToNbtTag for AttackRange {
    fn to_nbt_tag(self) -> NbtTag {
        let default = Self::default();
        let mut compound = NbtCompound::new();
        if self.min_reach.to_bits() != default.min_reach.to_bits() {
            compound.insert("min_reach", self.min_reach);
        }
        if self.max_reach.to_bits() != default.max_reach.to_bits() {
            compound.insert("max_reach", self.max_reach);
        }
        if self.min_creative_reach.to_bits() != default.min_creative_reach.to_bits() {
            compound.insert("min_creative_reach", self.min_creative_reach);
        }
        if self.max_creative_reach.to_bits() != default.max_creative_reach.to_bits() {
            compound.insert("max_creative_reach", self.max_creative_reach);
        }
        if self.hitbox_margin.to_bits() != default.hitbox_margin.to_bits() {
            compound.insert("hitbox_margin", self.hitbox_margin);
        }
        if self.mob_factor.to_bits() != default.mob_factor.to_bits() {
            compound.insert("mob_factor", self.mob_factor);
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for AttackRange {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let default = Self::default();
        Some(Self {
            min_reach: optional_ranged_f32(
                compound.get("min_reach"),
                default.min_reach,
                0.0,
                64.0,
            )?,
            max_reach: optional_ranged_f32(
                compound.get("max_reach"),
                default.max_reach,
                0.0,
                64.0,
            )?,
            min_creative_reach: optional_ranged_f32(
                compound.get("min_creative_reach"),
                default.min_creative_reach,
                0.0,
                64.0,
            )?,
            max_creative_reach: optional_ranged_f32(
                compound.get("max_creative_reach"),
                default.max_creative_reach,
                0.0,
                64.0,
            )?,
            hitbox_margin: optional_ranged_f32(
                compound.get("hitbox_margin"),
                default.hitbox_margin,
                0.0,
                1.0,
            )?,
            mob_factor: optional_ranged_f32(
                compound.get("mob_factor"),
                default.mob_factor,
                0.0,
                2.0,
            )?,
        })
    }
}

impl ToNbtTag for PiercingWeapon {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !self.deals_knockback {
            compound.insert("deals_knockback", self.deals_knockback);
        }
        if self.dismounts {
            compound.insert("dismounts", self.dismounts);
        }
        if let Some(sound) = self.sound {
            compound.insert("sound", sound.to_nbt_tag());
        }
        if let Some(sound) = self.hit_sound {
            compound.insert("hit_sound", sound.to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for PiercingWeapon {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let deals_knockback = match compound.get("deals_knockback") {
            Some(tag) => tag.codec_bool()?,
            None => true,
        };
        let dismounts = match compound.get("dismounts") {
            Some(tag) => tag.codec_bool()?,
            None => false,
        };
        let sound = match compound.get("sound") {
            Some(tag) => Some(SoundEventHolder::from_nbt_tag(tag)?),
            None => None,
        };
        let hit_sound = match compound.get("hit_sound") {
            Some(tag) => Some(SoundEventHolder::from_nbt_tag(tag)?),
            None => None,
        };
        Some(Self {
            deals_knockback,
            dismounts,
            sound,
            hit_sound,
        })
    }
}

fn optional_ranged_f32(
    tag: Option<simdnbt::borrow::NbtTag<'_, '_>>,
    default: f32,
    min: f32,
    max: f32,
) -> Option<f32> {
    let value = match tag {
        Some(tag) => tag.codec_f32()?,
        None => default,
    };
    (value.is_finite() && !value.is_sign_negative() && value >= min && value <= max)
        .then_some(value)
}

impl HashComponent for Weapon {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if self.item_damage_per_attack != 1 {
            push_hash_entry(
                &mut entries,
                "item_damage_per_attack",
                &self.item_damage_per_attack,
            );
        }
        if self.disable_blocking_for_seconds.to_bits() != 0.0_f32.to_bits() {
            push_hash_entry(
                &mut entries,
                "disable_blocking_for_seconds",
                &self.disable_blocking_for_seconds,
            );
        }
        hash_entries(hasher, &mut entries);
    }
}

impl HashComponent for DamageTypeComponent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(&self.damage_type.key.to_string());
    }
}

impl HashComponent for AttackRange {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let default = Self::default();
        let mut entries = Vec::new();
        if self.min_reach.to_bits() != default.min_reach.to_bits() {
            push_hash_entry(&mut entries, "min_reach", &self.min_reach);
        }
        if self.max_reach.to_bits() != default.max_reach.to_bits() {
            push_hash_entry(&mut entries, "max_reach", &self.max_reach);
        }
        if self.min_creative_reach.to_bits() != default.min_creative_reach.to_bits() {
            push_hash_entry(&mut entries, "min_creative_reach", &self.min_creative_reach);
        }
        if self.max_creative_reach.to_bits() != default.max_creative_reach.to_bits() {
            push_hash_entry(&mut entries, "max_creative_reach", &self.max_creative_reach);
        }
        if self.hitbox_margin.to_bits() != default.hitbox_margin.to_bits() {
            push_hash_entry(&mut entries, "hitbox_margin", &self.hitbox_margin);
        }
        if self.mob_factor.to_bits() != default.mob_factor.to_bits() {
            push_hash_entry(&mut entries, "mob_factor", &self.mob_factor);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl HashComponent for PiercingWeapon {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if !self.deals_knockback {
            push_hash_entry(&mut entries, "deals_knockback", &self.deals_knockback);
        }
        if self.dismounts {
            push_hash_entry(&mut entries, "dismounts", &self.dismounts);
        }
        if let Some(sound) = &self.sound {
            push_hash_entry(&mut entries, "sound", sound);
        }
        if let Some(sound) = &self.hit_sound {
            push_hash_entry(&mut entries, "hit_sound", sound);
        }
        hash_entries(hasher, &mut entries);
    }
}

fn hash_entries(hasher: &mut ComponentHasher, entries: &mut [HashEntry]) {
    sort_map_entries(entries);
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
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
    use simdnbt::owned::{NbtCompound, NbtTag};

    use super::{AttackRange, Weapon};

    fn with_borrowed_tag<R>(tag: NbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    #[test]
    fn combat_components_coerce_numbers_but_reject_malformed_present_fields() {
        let mut weapon = NbtCompound::new();
        weapon.insert("item_damage_per_attack", 2_i8);
        weapon.insert("disable_blocking_for_seconds", 5.5_f64);
        let weapon = with_borrowed_tag(NbtTag::Compound(weapon), Weapon::from_nbt_tag)
            .expect("valid weapon should parse");
        assert_eq!(weapon.item_damage_per_attack, 2);
        assert_eq!(weapon.disable_blocking_for_seconds, 5.5);

        let mut malformed = NbtCompound::new();
        malformed.insert("item_damage_per_attack", "two");
        assert!(with_borrowed_tag(NbtTag::Compound(malformed), Weapon::from_nbt_tag).is_none());

        let mut out_of_range = NbtCompound::new();
        out_of_range.insert("hitbox_margin", 1.5_f64);
        assert!(
            with_borrowed_tag(NbtTag::Compound(out_of_range), AttackRange::from_nbt_tag).is_none()
        );
    }
}
