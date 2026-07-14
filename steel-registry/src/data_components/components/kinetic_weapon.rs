//! Vanilla `minecraft:kinetic_weapon` item component.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::sound_event::SoundEventHolder;

/// Time and speed thresholds for one kinetic-weapon action.
#[derive(Debug, Clone)]
pub struct KineticWeaponCondition {
    max_duration_ticks: i32,
    min_speed: f32,
    min_relative_speed: f32,
}

impl PartialEq for KineticWeaponCondition {
    fn eq(&self, other: &Self) -> bool {
        self.max_duration_ticks == other.max_duration_ticks
            && java_float_equals(self.min_speed, other.min_speed)
            && java_float_equals(self.min_relative_speed, other.min_relative_speed)
    }
}

impl KineticWeaponCondition {
    pub fn new(max_duration_ticks: i32, min_speed: f32, min_relative_speed: f32) -> Result<Self> {
        if max_duration_ticks < 0 {
            return Err(Error::other(
                "Kinetic weapon condition duration must be non-negative",
            ));
        }
        Ok(Self {
            max_duration_ticks,
            min_speed,
            min_relative_speed,
        })
    }

    pub(crate) const fn from_extracted(
        max_duration_ticks: i32,
        min_speed: f32,
        min_relative_speed: f32,
    ) -> Self {
        assert!(
            max_duration_ticks >= 0,
            "extracted kinetic weapon duration must be non-negative"
        );
        Self {
            max_duration_ticks,
            min_speed,
            min_relative_speed,
        }
    }

    #[must_use]
    pub const fn max_duration_ticks(&self) -> i32 {
        self.max_duration_ticks
    }

    #[must_use]
    pub const fn min_speed(&self) -> f32 {
        self.min_speed
    }

    #[must_use]
    pub const fn min_relative_speed(&self) -> f32 {
        self.min_relative_speed
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("max_duration_ticks", self.max_duration_ticks);
        if self.min_speed.to_bits() != 0.0_f32.to_bits() {
            compound.insert("min_speed", self.min_speed);
        }
        if self.min_relative_speed.to_bits() != 0.0_f32.to_bits() {
            compound.insert("min_relative_speed", self.min_relative_speed);
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let max_duration_ticks = compound.get("max_duration_ticks")?.codec_i32()?;
        let min_speed = optional_owned_f32(compound.get("min_speed"), 0.0)?;
        let min_relative_speed = optional_owned_f32(compound.get("min_relative_speed"), 0.0)?;
        Self::new(max_duration_ticks, min_speed, min_relative_speed).ok()
    }
}

impl WriteTo for KineticWeaponCondition {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.max_duration_ticks).write(writer)?;
        self.min_speed.write(writer)?;
        self.min_relative_speed.write(writer)
    }
}

impl ReadFrom for KineticWeaponCondition {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(VarInt::read(data)?.0, f32::read(data)?, f32::read(data)?)
    }
}

impl HashComponent for KineticWeaponCondition {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(3);
        push_hash_entry(&mut entries, "max_duration_ticks", &self.max_duration_ticks);
        if self.min_speed.to_bits() != 0.0_f32.to_bits() {
            push_hash_entry(&mut entries, "min_speed", &self.min_speed);
        }
        if self.min_relative_speed.to_bits() != 0.0_f32.to_bits() {
            push_hash_entry(&mut entries, "min_relative_speed", &self.min_relative_speed);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Data controlling a spear-like continuous kinetic attack.
#[derive(Debug, Clone)]
pub struct KineticWeapon {
    contact_cooldown_ticks: i32,
    delay_ticks: i32,
    dismount_conditions: Option<KineticWeaponCondition>,
    knockback_conditions: Option<KineticWeaponCondition>,
    damage_conditions: Option<KineticWeaponCondition>,
    forward_movement: f32,
    damage_multiplier: f32,
    sound: Option<SoundEventHolder>,
    hit_sound: Option<SoundEventHolder>,
}

impl PartialEq for KineticWeapon {
    fn eq(&self, other: &Self) -> bool {
        self.contact_cooldown_ticks == other.contact_cooldown_ticks
            && self.delay_ticks == other.delay_ticks
            && self.dismount_conditions == other.dismount_conditions
            && self.knockback_conditions == other.knockback_conditions
            && self.damage_conditions == other.damage_conditions
            && java_float_equals(self.forward_movement, other.forward_movement)
            && java_float_equals(self.damage_multiplier, other.damage_multiplier)
            && self.sound == other.sound
            && self.hit_sound == other.hit_sound
    }
}

impl KineticWeapon {
    #[expect(clippy::too_many_arguments, reason = "mirrors Vanilla's record fields")]
    pub fn new(
        contact_cooldown_ticks: i32,
        delay_ticks: i32,
        dismount_conditions: Option<KineticWeaponCondition>,
        knockback_conditions: Option<KineticWeaponCondition>,
        damage_conditions: Option<KineticWeaponCondition>,
        forward_movement: f32,
        damage_multiplier: f32,
        sound: Option<SoundEventHolder>,
        hit_sound: Option<SoundEventHolder>,
    ) -> Result<Self> {
        if contact_cooldown_ticks < 0 || delay_ticks < 0 {
            return Err(Error::other(
                "Kinetic weapon cooldown and delay must be non-negative",
            ));
        }
        Ok(Self {
            contact_cooldown_ticks,
            delay_ticks,
            dismount_conditions,
            knockback_conditions,
            damage_conditions,
            forward_movement,
            damage_multiplier,
            sound,
            hit_sound,
        })
    }

    #[expect(clippy::too_many_arguments, reason = "mirrors Vanilla's record fields")]
    pub(crate) fn from_extracted(
        contact_cooldown_ticks: i32,
        delay_ticks: i32,
        dismount_conditions: Option<KineticWeaponCondition>,
        knockback_conditions: Option<KineticWeaponCondition>,
        damage_conditions: Option<KineticWeaponCondition>,
        forward_movement: f32,
        damage_multiplier: f32,
        sound: Option<SoundEventHolder>,
        hit_sound: Option<SoundEventHolder>,
    ) -> Self {
        assert!(
            contact_cooldown_ticks >= 0 && delay_ticks >= 0,
            "extracted kinetic weapon durations must be non-negative"
        );
        Self {
            contact_cooldown_ticks,
            delay_ticks,
            dismount_conditions,
            knockback_conditions,
            damage_conditions,
            forward_movement,
            damage_multiplier,
            sound,
            hit_sound,
        }
    }

    #[must_use]
    pub const fn contact_cooldown_ticks(&self) -> i32 {
        self.contact_cooldown_ticks
    }

    #[must_use]
    pub const fn delay_ticks(&self) -> i32 {
        self.delay_ticks
    }

    #[must_use]
    pub const fn dismount_conditions(&self) -> Option<&KineticWeaponCondition> {
        self.dismount_conditions.as_ref()
    }

    #[must_use]
    pub const fn knockback_conditions(&self) -> Option<&KineticWeaponCondition> {
        self.knockback_conditions.as_ref()
    }

    #[must_use]
    pub const fn damage_conditions(&self) -> Option<&KineticWeaponCondition> {
        self.damage_conditions.as_ref()
    }

    #[must_use]
    pub const fn forward_movement(&self) -> f32 {
        self.forward_movement
    }

    #[must_use]
    pub const fn damage_multiplier(&self) -> f32 {
        self.damage_multiplier
    }

    #[must_use]
    pub const fn sound(&self) -> Option<&SoundEventHolder> {
        self.sound.as_ref()
    }

    #[must_use]
    pub const fn hit_sound(&self) -> Option<&SoundEventHolder> {
        self.hit_sound.as_ref()
    }
}

impl WriteTo for KineticWeapon {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.contact_cooldown_ticks).write(writer)?;
        VarInt(self.delay_ticks).write(writer)?;
        self.dismount_conditions.write(writer)?;
        self.knockback_conditions.write(writer)?;
        self.damage_conditions.write(writer)?;
        self.forward_movement.write(writer)?;
        self.damage_multiplier.write(writer)?;
        self.sound.write(writer)?;
        self.hit_sound.write(writer)
    }
}

impl ReadFrom for KineticWeapon {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(
            VarInt::read(data)?.0,
            VarInt::read(data)?.0,
            Option::<KineticWeaponCondition>::read(data)?,
            Option::<KineticWeaponCondition>::read(data)?,
            Option::<KineticWeaponCondition>::read(data)?,
            f32::read(data)?,
            f32::read(data)?,
            Option::<SoundEventHolder>::read(data)?,
            Option::<SoundEventHolder>::read(data)?,
        )
    }
}

impl ToNbtTag for KineticWeapon {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if self.contact_cooldown_ticks != 10 {
            compound.insert("contact_cooldown_ticks", self.contact_cooldown_ticks);
        }
        if self.delay_ticks != 0 {
            compound.insert("delay_ticks", self.delay_ticks);
        }
        if let Some(condition) = self.dismount_conditions {
            compound.insert("dismount_conditions", condition.to_nbt_tag_ref());
        }
        if let Some(condition) = self.knockback_conditions {
            compound.insert("knockback_conditions", condition.to_nbt_tag_ref());
        }
        if let Some(condition) = self.damage_conditions {
            compound.insert("damage_conditions", condition.to_nbt_tag_ref());
        }
        if self.forward_movement.to_bits() != 0.0_f32.to_bits() {
            compound.insert("forward_movement", self.forward_movement);
        }
        if self.damage_multiplier.to_bits() != 1.0_f32.to_bits() {
            compound.insert("damage_multiplier", self.damage_multiplier);
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

impl FromNbtTag for KineticWeapon {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let contact_cooldown_ticks = optional_i32(compound.get("contact_cooldown_ticks"), 10)?;
        let delay_ticks = optional_i32(compound.get("delay_ticks"), 0)?;
        let dismount_conditions = optional_condition(compound.get("dismount_conditions"))?;
        let knockback_conditions = optional_condition(compound.get("knockback_conditions"))?;
        let damage_conditions = optional_condition(compound.get("damage_conditions"))?;
        let forward_movement = optional_f32(compound.get("forward_movement"), 0.0)?;
        let damage_multiplier = optional_f32(compound.get("damage_multiplier"), 1.0)?;
        let sound = optional_sound(compound.get("sound"))?;
        let hit_sound = optional_sound(compound.get("hit_sound"))?;
        Self::new(
            contact_cooldown_ticks,
            delay_ticks,
            dismount_conditions,
            knockback_conditions,
            damage_conditions,
            forward_movement,
            damage_multiplier,
            sound,
            hit_sound,
        )
        .ok()
    }
}

impl HashComponent for KineticWeapon {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(9);
        if self.contact_cooldown_ticks != 10 {
            push_hash_entry(
                &mut entries,
                "contact_cooldown_ticks",
                &self.contact_cooldown_ticks,
            );
        }
        if self.delay_ticks != 0 {
            push_hash_entry(&mut entries, "delay_ticks", &self.delay_ticks);
        }
        if let Some(condition) = &self.dismount_conditions {
            push_hash_entry(&mut entries, "dismount_conditions", condition);
        }
        if let Some(condition) = &self.knockback_conditions {
            push_hash_entry(&mut entries, "knockback_conditions", condition);
        }
        if let Some(condition) = &self.damage_conditions {
            push_hash_entry(&mut entries, "damage_conditions", condition);
        }
        if self.forward_movement.to_bits() != 0.0_f32.to_bits() {
            push_hash_entry(&mut entries, "forward_movement", &self.forward_movement);
        }
        if self.damage_multiplier.to_bits() != 1.0_f32.to_bits() {
            push_hash_entry(&mut entries, "damage_multiplier", &self.damage_multiplier);
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

fn optional_i32(tag: Option<simdnbt::borrow::NbtTag<'_, '_>>, default: i32) -> Option<i32> {
    match tag {
        Some(tag) => tag.codec_i32(),
        None => Some(default),
    }
}

fn optional_f32<T: steel_utils::nbt::NbtNumeric>(tag: Option<T>, default: f32) -> Option<f32> {
    match tag {
        Some(tag) => tag.codec_f32(),
        None => Some(default),
    }
}

fn optional_owned_f32(tag: Option<&NbtTag>, default: f32) -> Option<f32> {
    match tag {
        Some(tag) => tag.codec_f32(),
        None => Some(default),
    }
}

const fn java_float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

#[expect(
    clippy::option_option,
    reason = "the outer option reports codec failure while the inner option represents an absent field"
)]
fn optional_condition(
    tag: Option<simdnbt::borrow::NbtTag<'_, '_>>,
) -> Option<Option<KineticWeaponCondition>> {
    match tag {
        Some(tag) => Some(Some(KineticWeaponCondition::from_owned_nbt(
            &tag.to_owned(),
        )?)),
        None => Some(None),
    }
}

#[expect(
    clippy::option_option,
    reason = "the outer option reports codec failure while the inner option represents an absent field"
)]
fn optional_sound(
    tag: Option<simdnbt::borrow::NbtTag<'_, '_>>,
) -> Option<Option<SoundEventHolder>> {
    match tag {
        Some(tag) => Some(Some(SoundEventHolder::from_nbt_tag(tag)?)),
        None => Some(None),
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{KineticWeapon, KineticWeaponCondition};
    use crate::data_components::vanilla_components::KINETIC_WEAPON;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<KineticWeapon> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        KineticWeapon::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn kinetic_weapon_round_trips_both_codecs_and_hashes_record_shape() {
        let value = KineticWeapon::new(
            10,
            8,
            Some(KineticWeaponCondition::new(50, 9.0, 0.0).expect("valid condition")),
            None,
            Some(KineticWeaponCondition::new(175, 0.0, 4.6).expect("valid condition")),
            0.38,
            1.2,
            None,
            None,
        )
        .expect("valid kinetic weapon");
        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        assert_eq!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value.write(&mut network).expect("weapon should encode");
        assert_eq!(
            KineticWeapon::read(&mut Cursor::new(network.as_slice()))
                .expect("weapon should decode"),
            value
        );
    }

    #[test]
    fn negative_persistent_durations_are_rejected() {
        assert!(KineticWeaponCondition::new(-1, 0.0, 0.0).is_err());
        assert!(KineticWeapon::new(-1, 0, None, None, None, 0.0, 1.0, None, None).is_err());
    }

    #[test]
    fn equality_uses_java_record_float_semantics() {
        let first_nan = f32::from_bits(0x7fc0_0001);
        let second_nan = f32::from_bits(0x7fc0_0002);
        assert_eq!(
            KineticWeaponCondition::new(1, first_nan, 0.0).expect("valid condition"),
            KineticWeaponCondition::new(1, second_nan, 0.0).expect("valid condition")
        );
        assert_ne!(
            KineticWeaponCondition::new(1, 0.0, 0.0).expect("valid condition"),
            KineticWeaponCondition::new(1, -0.0, 0.0).expect("valid condition")
        );

        assert_eq!(
            KineticWeapon::new(10, 0, None, None, None, first_nan, 1.0, None, None)
                .expect("valid weapon"),
            KineticWeapon::new(10, 0, None, None, None, second_nan, 1.0, None, None)
                .expect("valid weapon")
        );
        assert_ne!(
            KineticWeapon::new(10, 0, None, None, None, 0.0, 1.0, None, None)
                .expect("valid weapon"),
            KineticWeapon::new(10, 0, None, None, None, -0.0, 1.0, None, None)
                .expect("valid weapon")
        );
    }

    #[test]
    fn extracted_netherite_spear_keeps_kinetic_thresholds_and_sounds() {
        init_test_registry();
        let spear = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("netherite_spear"))
            .expect("netherite spear should be registered");
        let kinetic = spear
            .components
            .get(KINETIC_WEAPON)
            .expect("netherite spear should have a kinetic weapon component");
        assert_eq!(kinetic.delay_ticks(), 8);
        assert_eq!(kinetic.forward_movement(), 0.38);
        assert_eq!(kinetic.damage_multiplier(), 1.2);
        assert_eq!(
            kinetic
                .damage_conditions()
                .expect("damage condition")
                .min_relative_speed(),
            4.6
        );
        assert_eq!(
            kinetic
                .sound()
                .and_then(crate::sound_event::SoundEventHolder::registry_ref)
                .map(|sound| &sound.key),
            Some(&steel_utils::Identifier::vanilla_static("item.spear.use"))
        );
        assert_eq!(
            kinetic
                .hit_sound()
                .and_then(crate::sound_event::SoundEventHolder::registry_ref)
                .map(|sound| &sound.key),
            Some(&steel_utils::Identifier::vanilla_static("item.spear.hit"))
        );
    }
}
