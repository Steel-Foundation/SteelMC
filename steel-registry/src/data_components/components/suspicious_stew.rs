//! Vanilla `minecraft:suspicious_stew_effects` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::mob_effect::MobEffectRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// One effect granted by a suspicious stew.
#[derive(Debug, Clone)]
pub struct SuspiciousStewEffect {
    effect: MobEffectRef,
    duration: i32,
}

impl SuspiciousStewEffect {
    pub const DEFAULT_DURATION: i32 = 160;

    #[must_use]
    pub const fn new(effect: MobEffectRef, duration: i32) -> Self {
        Self { effect, duration }
    }

    #[must_use]
    pub const fn effect(&self) -> MobEffectRef {
        self.effect
    }

    #[must_use]
    pub const fn duration(&self) -> i32 {
        self.duration
    }

    fn to_nbt_compound(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("id", self.effect.key.to_string());
        if self.duration != Self::DEFAULT_DURATION {
            compound.insert("duration", self.duration);
        }
        compound
    }

    fn from_nbt_compound(compound: &NbtCompound) -> Option<Self> {
        let id = Identifier::from_str(&compound.get("id")?.string()?.to_string()).ok()?;
        let effect = REGISTRY.mob_effects.by_key(&id)?;
        let duration = compound
            .get("duration")
            .and_then(steel_utils::nbt::NbtNumeric::codec_i32)
            .unwrap_or(Self::DEFAULT_DURATION);
        Some(Self::new(effect, duration))
    }
}

impl PartialEq for SuspiciousStewEffect {
    fn eq(&self, other: &Self) -> bool {
        self.effect.key == other.effect.key && self.duration == other.duration
    }
}

impl WriteTo for SuspiciousStewEffect {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self
            .effect
            .try_id()
            .ok_or_else(|| Error::other(format!("Unknown mob effect: {}", self.effect.key)))?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Mob effect id out of range: {id}")))?;
        VarInt(id).write(writer)?;
        VarInt(self.duration).write(writer)
    }
}

impl ReadFrom for SuspiciousStewEffect {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative mob effect id: {id}")))?;
        let effect = REGISTRY
            .mob_effects
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown mob effect id: {id}")))?;
        Ok(Self::new(effect, VarInt::read(data)?.0))
    }
}

impl HashComponent for SuspiciousStewEffect {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "id", &self.effect.key);
        if self.duration != Self::DEFAULT_DURATION {
            push_hash_entry(&mut entries, "duration", &self.duration);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Ordered effects granted by a suspicious stew.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SuspiciousStewEffects {
    effects: Vec<SuspiciousStewEffect>,
}

impl SuspiciousStewEffects {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    #[must_use]
    pub const fn new(effects: Vec<SuspiciousStewEffect>) -> Self {
        Self { effects }
    }

    #[must_use]
    pub fn effects(&self) -> &[SuspiciousStewEffect] {
        &self.effects
    }
}

impl WriteTo for SuspiciousStewEffects {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_count(self.effects.len(), writer)?;
        for effect in &self.effects {
            effect.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for SuspiciousStewEffects {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_count(data)?;
        let mut effects = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            effects.push(SuspiciousStewEffect::read(data)?);
        }
        Ok(Self::new(effects))
    }
}

impl ToNbtTag for SuspiciousStewEffects {
    fn to_nbt_tag(self) -> NbtTag {
        if self.effects.is_empty() {
            NbtTag::List(NbtList::Empty)
        } else {
            NbtTag::List(NbtList::Compound(
                self.effects
                    .iter()
                    .map(SuspiciousStewEffect::to_nbt_compound)
                    .collect(),
            ))
        }
    }
}

impl FromNbtTag for SuspiciousStewEffects {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let effects = tag
            .list()?
            .to_owned()
            .as_nbt_tags()
            .iter()
            .map(|tag| SuspiciousStewEffect::from_nbt_compound(tag.compound()?))
            .collect::<Option<Vec<_>>>()?;
        Some(Self::new(effects))
    }
}

impl HashComponent for SuspiciousStewEffects {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for effect in &self.effects {
            hasher.put_component_hash(effect);
        }
        hasher.end_list();
    }
}

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(count).map_err(|_| Error::other("Effect list is too large"))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count).map_err(|_| Error::other(format!("Negative effect count: {count}")))
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

    use super::{SuspiciousStewEffect, SuspiciousStewEffects};
    use crate::data_components::vanilla_components::SUSPICIOUS_STEW_EFFECTS;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    #[test]
    fn stew_effects_round_trip_and_malformed_duration_uses_lenient_default() {
        init_test_registry();
        let night_vision = REGISTRY
            .mob_effects
            .by_key(&steel_utils::Identifier::vanilla_static("night_vision"))
            .expect("night vision should be registered");
        let value = SuspiciousStewEffects::new(vec![SuspiciousStewEffect::new(
            night_vision,
            SuspiciousStewEffect::DEFAULT_DURATION,
        )]);
        let nbt = value.clone().to_nbt_tag();
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice()))
            .expect("stew NBT should parse");
        assert_eq!(
            SuspiciousStewEffects::from_nbt_tag(borrowed.as_tag()),
            Some(value.clone())
        );
        let mut network = Vec::new();
        value.write(&mut network).expect("effects should encode");
        assert_eq!(
            SuspiciousStewEffects::read(&mut Cursor::new(network.as_slice()))
                .expect("effects should decode"),
            value
        );
    }

    #[test]
    fn extracted_suspicious_stew_has_an_empty_effect_list() {
        init_test_registry();
        let stew = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("suspicious_stew"))
            .expect("suspicious stew should be registered");
        assert_eq!(
            stew.components.get(SUSPICIOUS_STEW_EFFECTS),
            Some(SuspiciousStewEffects::empty())
        );
    }
}
