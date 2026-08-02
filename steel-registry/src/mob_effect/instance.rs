//! Vanilla mob-effect instance codec model.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::mob_effect::MobEffectRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

const MAX_EFFECT_DEPTH: usize = 512;

/// One status-effect instance, including Vanilla's hidden fallback chain.
#[derive(Debug, Clone)]
pub struct MobEffectInstance {
    effect: MobEffectRef,
    duration: i32,
    amplifier: i32,
    ambient: bool,
    show_particles: bool,
    show_icon: bool,
    hidden_effect: Option<Box<MobEffectInstanceDetails>>,
}

/// Recursive fields shared by a visible effect and its hidden fallbacks.
#[derive(Debug, Clone)]
pub struct MobEffectInstanceDetails {
    amplifier: i32,
    duration: i32,
    ambient: bool,
    show_particles: bool,
    show_icon: bool,
    hidden_effect: Option<Box<Self>>,
}

impl MobEffectInstance {
    #[must_use]
    pub fn new(
        effect: MobEffectRef,
        duration: i32,
        amplifier: i32,
        ambient: bool,
        show_particles: bool,
        show_icon: bool,
        hidden_effect: Option<MobEffectInstanceDetails>,
    ) -> Self {
        Self {
            effect,
            duration,
            amplifier: amplifier.clamp(0, 255),
            ambient,
            show_particles,
            show_icon,
            hidden_effect: hidden_effect.map(Box::new),
        }
    }

    #[must_use]
    pub fn simple(effect: MobEffectRef, duration: i32, amplifier: i32) -> Self {
        Self::new(effect, duration, amplifier, false, true, true, None)
    }

    #[must_use]
    pub const fn effect(&self) -> MobEffectRef {
        self.effect
    }

    #[must_use]
    pub const fn duration(&self) -> i32 {
        self.duration
    }

    #[must_use]
    pub const fn amplifier(&self) -> i32 {
        self.amplifier
    }

    #[must_use]
    pub const fn ambient(&self) -> bool {
        self.ambient
    }

    #[must_use]
    pub const fn show_particles(&self) -> bool {
        self.show_particles
    }

    #[must_use]
    pub const fn show_icon(&self) -> bool {
        self.show_icon
    }

    #[must_use]
    pub fn hidden_effect(&self) -> Option<&MobEffectInstanceDetails> {
        self.hidden_effect.as_deref()
    }

    fn details(&self) -> MobEffectInstanceDetails {
        MobEffectInstanceDetails {
            amplifier: self.amplifier,
            duration: self.duration,
            ambient: self.ambient,
            show_particles: self.show_particles,
            show_icon: self.show_icon,
            hidden_effect: self.hidden_effect.clone(),
        }
    }

    fn from_details(effect: MobEffectRef, details: MobEffectInstanceDetails) -> Self {
        Self::new(
            effect,
            details.duration,
            details.amplifier,
            details.ambient,
            details.show_particles,
            details.show_icon,
            details.hidden_effect.map(|hidden| *hidden),
        )
    }

    pub(crate) fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = self.details().to_nbt_compound();
        compound.insert("id", self.effect.key.to_string());
        NbtTag::Compound(compound)
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let id = Identifier::from_str(&compound.get("id")?.string()?.to_string()).ok()?;
        let effect = REGISTRY.mob_effects.by_key(&id)?;
        let details = MobEffectInstanceDetails::from_owned_compound(compound, 0)?;
        Some(Self::from_details(effect, details))
    }
}

impl PartialEq for MobEffectInstance {
    fn eq(&self, other: &Self) -> bool {
        self.effect.key == other.effect.key
            && self.duration == other.duration
            && self.amplifier == other.amplifier
            && self.ambient == other.ambient
            && self.show_particles == other.show_particles
            && self.show_icon == other.show_icon
    }
}

impl MobEffectInstanceDetails {
    #[must_use]
    pub fn new(
        amplifier: i32,
        duration: i32,
        ambient: bool,
        show_particles: bool,
        show_icon: bool,
        hidden_effect: Option<Self>,
    ) -> Self {
        Self {
            amplifier: amplifier.clamp(0, 255),
            duration,
            ambient,
            show_particles,
            show_icon,
            hidden_effect: hidden_effect.map(Box::new),
        }
    }

    fn to_nbt_compound(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        if self.amplifier != 0 {
            compound.insert("amplifier", self.amplifier as u8 as i8);
        }
        if self.duration != 0 {
            compound.insert("duration", self.duration);
        }
        if self.ambient {
            compound.insert("ambient", true);
        }
        if !self.show_particles {
            compound.insert("show_particles", false);
        }
        compound.insert("show_icon", self.show_icon);
        if let Some(hidden) = &self.hidden_effect {
            compound.insert("hidden_effect", NbtTag::Compound(hidden.to_nbt_compound()));
        }
        compound
    }

    fn from_owned_compound(compound: &NbtCompound, depth: usize) -> Option<Self> {
        if depth >= MAX_EFFECT_DEPTH {
            return None;
        }
        let amplifier = match compound.get("amplifier") {
            Some(tag) => i32::from(tag.codec_i32()? as i8 as u8),
            None => 0,
        };
        let duration = optional_i32(compound.get("duration"), 0)?;
        let ambient = optional_bool(compound.get("ambient"), false)?;
        let show_particles = optional_bool(compound.get("show_particles"), true)?;
        let show_icon = match compound.get("show_icon") {
            Some(tag) => tag.codec_bool()?,
            None => show_particles,
        };
        let hidden_effect = match compound.get("hidden_effect") {
            Some(tag) => Some(Self::from_owned_compound(tag.compound()?, depth + 1)?),
            None => None,
        };
        Some(Self::new(
            amplifier,
            duration,
            ambient,
            show_particles,
            show_icon,
            hidden_effect,
        ))
    }
}

impl PartialEq for MobEffectInstanceDetails {
    fn eq(&self, other: &Self) -> bool {
        self.amplifier == other.amplifier
            && self.duration == other.duration
            && self.ambient == other.ambient
            && self.show_particles == other.show_particles
            && self.show_icon == other.show_icon
            && self.hidden_effect == other.hidden_effect
    }
}

impl WriteTo for MobEffectInstance {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self
            .effect
            .try_id()
            .ok_or_else(|| Error::other(format!("Unknown mob effect: {}", self.effect.key)))?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Mob effect id out of range: {id}")))?;
        VarInt(id).write(writer)?;
        write_details(&self.details(), writer, 0)
    }
}

impl ReadFrom for MobEffectInstance {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative mob effect id: {id}")))?;
        let effect = REGISTRY
            .mob_effects
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown mob effect id: {id}")))?;
        Ok(Self::from_details(effect, read_details(data, 0)?))
    }
}

impl ToNbtTag for MobEffectInstance {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for MobEffectInstance {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for MobEffectInstance {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = details_hash_entries(&self.details());
        push_hash_entry(&mut entries, "id", &self.effect.key);
        hash_entries(hasher, &mut entries);
    }
}

impl HashComponent for MobEffectInstanceDetails {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = details_hash_entries(self);
        hash_entries(hasher, &mut entries);
    }
}

fn write_details(
    details: &MobEffectInstanceDetails,
    writer: &mut impl Write,
    depth: usize,
) -> Result<()> {
    if depth >= MAX_EFFECT_DEPTH {
        return Err(Error::other("Mob effect hidden chain is too deep"));
    }
    VarInt(details.amplifier).write(writer)?;
    VarInt(details.duration).write(writer)?;
    details.ambient.write(writer)?;
    details.show_particles.write(writer)?;
    details.show_icon.write(writer)?;
    details.hidden_effect.is_some().write(writer)?;
    if let Some(hidden) = &details.hidden_effect {
        write_details(hidden, writer, depth + 1)?;
    }
    Ok(())
}

fn read_details(data: &mut Cursor<&[u8]>, depth: usize) -> Result<MobEffectInstanceDetails> {
    if depth >= MAX_EFFECT_DEPTH {
        return Err(Error::other("Mob effect hidden chain is too deep"));
    }
    let amplifier = VarInt::read(data)?.0;
    let duration = VarInt::read(data)?.0;
    let ambient = bool::read(data)?;
    let show_particles = bool::read(data)?;
    let show_icon = bool::read(data)?;
    let hidden_effect = if bool::read(data)? {
        Some(read_details(data, depth + 1)?)
    } else {
        None
    };
    Ok(MobEffectInstanceDetails::new(
        amplifier,
        duration,
        ambient,
        show_particles,
        show_icon,
        hidden_effect,
    ))
}

fn details_hash_entries(details: &MobEffectInstanceDetails) -> Vec<HashEntry> {
    let mut entries = Vec::with_capacity(6);
    if details.amplifier != 0 {
        push_hash_entry(&mut entries, "amplifier", &(details.amplifier as u8 as i8));
    }
    if details.duration != 0 {
        push_hash_entry(&mut entries, "duration", &details.duration);
    }
    if details.ambient {
        push_hash_entry(&mut entries, "ambient", &true);
    }
    if !details.show_particles {
        push_hash_entry(&mut entries, "show_particles", &false);
    }
    push_hash_entry(&mut entries, "show_icon", &details.show_icon);
    if let Some(hidden) = &details.hidden_effect {
        push_hash_entry(&mut entries, "hidden_effect", hidden.as_ref());
    }
    entries
}

fn optional_i32(tag: Option<&NbtTag>, default: i32) -> Option<i32> {
    match tag {
        Some(tag) => tag.codec_i32(),
        None => Some(default),
    }
}

fn optional_bool(tag: Option<&NbtTag>, default: bool) -> Option<bool> {
    match tag {
        Some(tag) => tag.codec_bool(),
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

    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{MobEffectInstance, MobEffectInstanceDetails};
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    #[test]
    fn effect_instances_round_trip_recursive_details_and_clamp_amplifier() {
        init_test_registry();
        let speed = REGISTRY
            .mob_effects
            .by_key(&steel_utils::Identifier::vanilla_static("speed"))
            .expect("speed should be registered");
        let value = MobEffectInstance::new(
            speed,
            200,
            300,
            false,
            true,
            false,
            Some(MobEffectInstanceDetails::new(
                1, 400, true, false, false, None,
            )),
        );
        assert_eq!(value.amplifier(), 255);
        let nbt = value.clone().to_nbt_tag();
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice()))
            .expect("effect NBT should parse");
        assert_eq!(
            MobEffectInstance::from_nbt_tag(borrowed.as_tag()),
            Some(value.clone())
        );

        let mut network = Vec::new();
        value.write(&mut network).expect("effect should encode");
        assert_eq!(
            MobEffectInstance::read(&mut Cursor::new(network.as_slice()))
                .expect("effect should decode"),
            value
        );
    }

    #[test]
    fn effect_instance_equality_ignores_hidden_effects_like_vanilla() {
        init_test_registry();
        let speed = REGISTRY
            .mob_effects
            .by_key(&steel_utils::Identifier::vanilla_static("speed"))
            .expect("speed should be registered");
        let without_hidden = MobEffectInstance::simple(speed, 200, 1);
        let with_hidden = MobEffectInstance::new(
            speed,
            200,
            1,
            false,
            true,
            true,
            Some(MobEffectInstanceDetails::new(
                2, 400, false, true, true, None,
            )),
        );

        assert_eq!(with_hidden, without_hidden);
    }
}
