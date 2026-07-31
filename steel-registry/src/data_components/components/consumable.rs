//! Vanilla consumable and death-protection item components.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::consume_effect::ConsumeEffectData;
use crate::sound_event::SoundEventHolder;

/// Arm animation displayed while using an item.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ItemUseAnimation {
    None,
    #[default]
    Eat,
    Drink,
    Block,
    Bow,
    Trident,
    Crossbow,
    Spyglass,
    TootHorn,
    Brush,
    Bundle,
    Spear,
}

impl ItemUseAnimation {
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Eat => 1,
            Self::Drink => 2,
            Self::Block => 3,
            Self::Bow => 4,
            Self::Trident => 5,
            Self::Crossbow => 6,
            Self::Spyglass => 7,
            Self::TootHorn => 8,
            Self::Brush => 9,
            Self::Bundle => 10,
            Self::Spear => 11,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Eat => "eat",
            Self::Drink => "drink",
            Self::Block => "block",
            Self::Bow => "bow",
            Self::Trident => "trident",
            Self::Crossbow => "crossbow",
            Self::Spyglass => "spyglass",
            Self::TootHorn => "toot_horn",
            Self::Brush => "brush",
            Self::Bundle => "bundle",
            Self::Spear => "spear",
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Eat,
            2 => Self::Drink,
            3 => Self::Block,
            4 => Self::Bow,
            5 => Self::Trident,
            6 => Self::Crossbow,
            7 => Self::Spyglass,
            8 => Self::TootHorn,
            9 => Self::Brush,
            10 => Self::Bundle,
            11 => Self::Spear,
            _ => Self::None,
        }
    }

    const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "eat" => Some(Self::Eat),
            "drink" => Some(Self::Drink),
            "block" => Some(Self::Block),
            "bow" => Some(Self::Bow),
            "trident" => Some(Self::Trident),
            "crossbow" => Some(Self::Crossbow),
            "spyglass" => Some(Self::Spyglass),
            "toot_horn" => Some(Self::TootHorn),
            "brush" => Some(Self::Brush),
            "bundle" => Some(Self::Bundle),
            "spear" => Some(Self::Spear),
            _ => None,
        }
    }
}

impl WriteTo for ItemUseAnimation {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.id()).write(writer)
    }
}

impl ReadFrom for ItemUseAnimation {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::by_id(VarInt::read(data)?.0))
    }
}

/// Use timing, presentation, and post-consumption effects.
#[derive(Debug, Clone)]
pub struct Consumable {
    consume_seconds: f32,
    animation: ItemUseAnimation,
    sound: SoundEventHolder,
    has_consume_particles: bool,
    on_consume_effects: Vec<ConsumeEffectData>,
}

impl Consumable {
    pub const DEFAULT_CONSUME_SECONDS: f32 = 1.6;

    pub fn new(
        consume_seconds: f32,
        animation: ItemUseAnimation,
        sound: SoundEventHolder,
        has_consume_particles: bool,
        on_consume_effects: Vec<ConsumeEffectData>,
    ) -> Result<Self> {
        if !is_non_negative_float(consume_seconds) {
            return Err(Error::other("Consume duration must be non-negative"));
        }
        Ok(Self {
            consume_seconds,
            animation,
            sound,
            has_consume_particles,
            on_consume_effects,
        })
    }

    pub(crate) fn from_extracted(
        consume_seconds: f32,
        animation: ItemUseAnimation,
        sound: SoundEventHolder,
        has_consume_particles: bool,
        on_consume_effects: Vec<ConsumeEffectData>,
    ) -> Self {
        assert!(
            is_non_negative_float(consume_seconds),
            "extracted consume duration must be non-negative"
        );
        Self {
            consume_seconds,
            animation,
            sound,
            has_consume_particles,
            on_consume_effects,
        }
    }

    #[must_use]
    pub const fn consume_seconds(&self) -> f32 {
        self.consume_seconds
    }

    #[must_use]
    pub const fn animation(&self) -> ItemUseAnimation {
        self.animation
    }

    #[must_use]
    pub const fn sound(&self) -> &SoundEventHolder {
        &self.sound
    }

    #[must_use]
    pub const fn has_consume_particles(&self) -> bool {
        self.has_consume_particles
    }

    #[must_use]
    pub fn on_consume_effects(&self) -> &[ConsumeEffectData] {
        &self.on_consume_effects
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !float_equals(self.consume_seconds, Self::DEFAULT_CONSUME_SECONDS) {
            compound.insert("consume_seconds", self.consume_seconds);
        }
        if self.animation != ItemUseAnimation::Eat {
            compound.insert("animation", self.animation.serialized_name());
        }
        if !is_default_eat_sound(&self.sound) {
            compound.insert("sound", self.sound.clone().to_nbt_tag());
        }
        if !self.has_consume_particles {
            compound.insert("has_consume_particles", false);
        }
        if !self.on_consume_effects.is_empty() {
            compound.insert(
                "on_consume_effects",
                NbtList::Compound(
                    self.on_consume_effects
                        .iter()
                        .map(|effect| match effect.to_nbt_tag_ref() {
                            NbtTag::Compound(compound) => compound,
                            _ => unreachable!("consume-effect codec always produces a compound"),
                        })
                        .collect(),
                ),
            );
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let animation = match compound.get("animation") {
            Some(tag) => ItemUseAnimation::from_serialized_name(&tag.string()?.to_string())?,
            None => ItemUseAnimation::Eat,
        };
        let sound = match compound.get("sound") {
            Some(tag) => SoundEventHolder::from_owned_nbt(tag)?,
            None => default_eat_sound(),
        };
        let has_consume_particles = match compound.get("has_consume_particles") {
            Some(tag) => tag.codec_bool()?,
            None => true,
        };
        let on_consume_effects = match compound.get("on_consume_effects") {
            Some(tag) => tag
                .list()?
                .as_nbt_tags()
                .iter()
                .map(ConsumeEffectData::from_owned_nbt)
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        Self::new(
            optional_f32(
                compound.get("consume_seconds"),
                Self::DEFAULT_CONSUME_SECONDS,
            )?,
            animation,
            sound,
            has_consume_particles,
            on_consume_effects,
        )
        .ok()
    }
}

impl PartialEq for Consumable {
    fn eq(&self, other: &Self) -> bool {
        float_equals(self.consume_seconds, other.consume_seconds)
            && self.animation == other.animation
            && self.sound == other.sound
            && self.has_consume_particles == other.has_consume_particles
            && self.on_consume_effects == other.on_consume_effects
    }
}

impl WriteTo for Consumable {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.consume_seconds.write(writer)?;
        self.animation.write(writer)?;
        self.sound.write(writer)?;
        self.has_consume_particles.write(writer)?;
        write_effect_list(&self.on_consume_effects, writer)
    }
}

impl ReadFrom for Consumable {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(
            f32::read(data)?,
            ItemUseAnimation::read(data)?,
            SoundEventHolder::read(data)?,
            bool::read(data)?,
            read_effect_list(data)?,
        )
    }
}

impl ToNbtTag for Consumable {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for Consumable {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for Consumable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(5);
        if !float_equals(self.consume_seconds, Self::DEFAULT_CONSUME_SECONDS) {
            push_hash_entry(&mut entries, "consume_seconds", &self.consume_seconds);
        }
        if self.animation != ItemUseAnimation::Eat {
            push_hash_entry(&mut entries, "animation", self.animation.serialized_name());
        }
        if !is_default_eat_sound(&self.sound) {
            push_hash_entry(&mut entries, "sound", &self.sound);
        }
        if !self.has_consume_particles {
            push_hash_entry(&mut entries, "has_consume_particles", &false);
        }
        if !self.on_consume_effects.is_empty() {
            push_hash_entry(
                &mut entries,
                "on_consume_effects",
                &ConsumeEffectList(&self.on_consume_effects),
            );
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Effects applied when an item prevents death.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DeathProtection {
    death_effects: Vec<ConsumeEffectData>,
}

impl DeathProtection {
    #[must_use]
    pub const fn new(death_effects: Vec<ConsumeEffectData>) -> Self {
        Self { death_effects }
    }

    #[must_use]
    pub fn death_effects(&self) -> &[ConsumeEffectData] {
        &self.death_effects
    }
}

impl WriteTo for DeathProtection {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_effect_list(&self.death_effects, writer)
    }
}

impl ReadFrom for DeathProtection {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(read_effect_list(data)?))
    }
}

impl ToNbtTag for DeathProtection {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !self.death_effects.is_empty() {
            compound.insert(
                "death_effects",
                NbtList::Compound(
                    self.death_effects
                        .iter()
                        .map(|effect| match effect.to_nbt_tag_ref() {
                            NbtTag::Compound(compound) => compound,
                            _ => unreachable!("consume-effect codec always produces a compound"),
                        })
                        .collect(),
                ),
            );
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for DeathProtection {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let tag = tag.to_owned();
        let compound = tag.compound()?;
        let death_effects = match compound.get("death_effects") {
            Some(tag) => tag
                .list()?
                .as_nbt_tags()
                .iter()
                .map(ConsumeEffectData::from_owned_nbt)
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        Some(Self::new(death_effects))
    }
}

impl HashComponent for DeathProtection {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(1);
        if !self.death_effects.is_empty() {
            push_hash_entry(
                &mut entries,
                "death_effects",
                &ConsumeEffectList(&self.death_effects),
            );
        }
        hash_entries(hasher, &mut entries);
    }
}

struct ConsumeEffectList<'a>(&'a [ConsumeEffectData]);

impl HashComponent for ConsumeEffectList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for effect in self.0 {
            hasher.put_component_hash(effect);
        }
        hasher.end_list();
    }
}

fn default_eat_sound() -> SoundEventHolder {
    SoundEventHolder::registry(&crate::sound_events::ENTITY_GENERIC_EAT)
}

fn is_default_eat_sound(sound: &SoundEventHolder) -> bool {
    sound == &default_eat_sound()
}

fn write_effect_list(effects: &[ConsumeEffectData], writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(effects.len())
        .map_err(|_| Error::other("Consume effect list is too large"))?;
    VarInt(count).write(writer)?;
    for effect in effects {
        effect.write(writer)?;
    }
    Ok(())
}

fn read_effect_list(data: &mut Cursor<&[u8]>) -> Result<Vec<ConsumeEffectData>> {
    let count = VarInt::read(data)?.0;
    let count = usize::try_from(count)
        .map_err(|_| Error::other(format!("Negative consume effect count: {count}")))?;
    let mut effects = Vec::with_capacity(count.min(65_536));
    for _ in 0..count {
        effects.push(ConsumeEffectData::read(data)?);
    }
    Ok(effects)
}

const fn float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

const fn is_non_negative_float(value: f32) -> bool {
    value.is_finite() && !value.is_sign_negative()
}

fn optional_f32(tag: Option<&NbtTag>, default: f32) -> Option<f32> {
    match tag {
        Some(tag) => tag.codec_f32(),
        None => Some(default),
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

    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{Consumable, DeathProtection, ItemUseAnimation};
    use crate::consume_effect::{
        ApplyStatusEffectsConsumeEffect, ClearAllStatusEffectsConsumeEffect,
        RemoveStatusEffectsConsumeEffect, TeleportRandomlyConsumeEffect,
    };
    use crate::data_components::vanilla_components::{CONSUMABLE, DEATH_PROTECTION};
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse<T: simdnbt::FromNbtTag>(tag: simdnbt::owned::NbtTag) -> Option<T> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        T::from_nbt_tag(borrowed.as_tag())
    }

    fn assert_round_trip<T>(value: &T)
    where
        T: Clone
            + PartialEq
            + std::fmt::Debug
            + simdnbt::ToNbtTag
            + simdnbt::FromNbtTag
            + steel_utils::serial::WriteTo
            + steel_utils::serial::ReadFrom,
    {
        assert_eq!(parse(value.clone().to_nbt_tag()), Some(value.clone()));
        let mut network = Vec::new();
        value.write(&mut network).expect("component should encode");
        assert_eq!(
            T::read(&mut Cursor::new(network.as_slice())).expect("component should decode"),
            value.clone()
        );
    }

    #[test]
    fn extracted_consumables_keep_typed_effects_and_round_trip() {
        init_test_registry();
        let golden_apple = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("golden_apple"))
            .expect("golden apple should be registered")
            .components
            .get(CONSUMABLE)
            .expect("golden apple should be consumable");
        let apply = golden_apple.on_consume_effects()[0]
            .downcast_ref::<ApplyStatusEffectsConsumeEffect>()
            .expect("golden apple should apply effects");
        assert_eq!(apply.effects().len(), 2);
        assert_round_trip(&golden_apple);

        let milk = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("milk_bucket"))
            .expect("milk bucket should be registered")
            .components
            .get(CONSUMABLE)
            .expect("milk should be consumable");
        assert_eq!(milk.animation(), ItemUseAnimation::Drink);
        assert!(!milk.has_consume_particles());
        assert!(
            milk.on_consume_effects()[0]
                .downcast_ref::<ClearAllStatusEffectsConsumeEffect>()
                .is_some()
        );
        assert_round_trip(&milk);

        let honey = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("honey_bottle"))
            .expect("honey bottle should be registered")
            .components
            .get(CONSUMABLE)
            .expect("honey should be consumable");
        assert!(
            honey.on_consume_effects()[0]
                .downcast_ref::<RemoveStatusEffectsConsumeEffect>()
                .is_some()
        );
        assert_round_trip(&honey);
    }

    #[test]
    fn extracted_totem_death_protection_round_trips() {
        init_test_registry();
        let totem = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("totem_of_undying"))
            .expect("totem should be registered")
            .components
            .get(DEATH_PROTECTION)
            .expect("totem should provide death protection");
        assert_eq!(totem.death_effects().len(), 2);
        assert!(
            totem.death_effects()[0]
                .downcast_ref::<ClearAllStatusEffectsConsumeEffect>()
                .is_some()
        );
        assert_round_trip::<DeathProtection>(&totem);
    }

    #[test]
    fn network_animation_ids_use_vanilla_zero_fallback() {
        let mut bytes = Vec::new();
        VarInt(i32::MAX)
            .write(&mut bytes)
            .expect("test id should encode");
        assert_eq!(
            ItemUseAnimation::read(&mut Cursor::new(bytes.as_slice()))
                .expect("unknown id should decode"),
            ItemUseAnimation::None
        );
        assert!(
            Consumable::new(
                -1.0,
                ItemUseAnimation::Eat,
                crate::sound_event::SoundEventHolder::Direct {
                    sound_id: steel_utils::Identifier::vanilla_static("test"),
                    fixed_range: None,
                },
                true,
                Vec::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn persistent_float_ranges_match_vanilla_float_compare_bounds() {
        let sound = || crate::sound_event::SoundEventHolder::Direct {
            sound_id: steel_utils::Identifier::vanilla_static("test"),
            fixed_range: None,
        };
        for invalid in [-0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(
                Consumable::new(invalid, ItemUseAnimation::Eat, sound(), true, Vec::new()).is_err()
            );
        }
        assert!(Consumable::new(0.0, ItemUseAnimation::Eat, sound(), true, Vec::new()).is_ok());

        for invalid in [0.0, -0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(TeleportRandomlyConsumeEffect::new(invalid).is_err());
        }
        assert!(TeleportRandomlyConsumeEffect::new(f32::MAX).is_ok());

        for invalid in [-0.0, 1.1, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(ApplyStatusEffectsConsumeEffect::new(Vec::new(), invalid).is_err());
        }
        assert!(ApplyStatusEffectsConsumeEffect::new(Vec::new(), 0.0).is_ok());
        assert!(ApplyStatusEffectsConsumeEffect::new(Vec::new(), 1.0).is_ok());
    }
}
