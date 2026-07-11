use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Built-in sound event registry entry used by sound packets and data-driven audio refs.
#[derive(Debug)]
pub struct SoundEvent {
    pub key: Identifier,
    pub sound_id: Identifier,
    pub fixed_range: Option<f32>,
}

impl SoundEvent {
    /// Vanilla `SoundEvent.getRange`.
    #[must_use]
    pub fn range(&self, volume: f32) -> f32 {
        self.fixed_range
            .unwrap_or(if volume > 1.0 { 16.0 * volume } else { 16.0 })
    }

    /// Returns the `VarInt` payload used by vanilla holder-based sound packets.
    #[must_use]
    pub fn packet_holder_id(&self) -> i32 {
        let id = crate::RegistryEntry::id(self);
        assert!(
            id < i32::MAX as usize,
            "sound event registry id exceeds protocol VarInt range"
        );
        id as i32 + 1
    }
}

pub type SoundEventRef = &'static SoundEvent;

/// Vanilla `Holder<SoundEvent>`.
#[derive(Debug, Clone, PartialEq)]
pub enum SoundEventHolder {
    Registry(SoundEventRef),
    Direct {
        sound_id: Identifier,
        fixed_range: Option<f32>,
    },
}

impl SoundEventHolder {
    #[must_use]
    pub const fn registry(sound: SoundEventRef) -> Self {
        Self::Registry(sound)
    }

    #[must_use]
    pub const fn registry_ref(&self) -> Option<SoundEventRef> {
        match self {
            Self::Registry(sound) => Some(*sound),
            Self::Direct { .. } => None,
        }
    }
}

impl WriteTo for SoundEventHolder {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        match self {
            Self::Registry(sound) => {
                let id = sound
                    .try_id()
                    .ok_or_else(|| Error::other(format!("Unknown sound event: {}", sound.key)))?;
                let id = i32::try_from(id).map_err(|_| {
                    Error::other(format!("Sound event id out of protocol range: {id}"))
                })?;
                VarInt(id + 1).write(writer)
            }
            Self::Direct {
                sound_id,
                fixed_range,
            } => {
                VarInt(0).write(writer)?;
                sound_id.write(writer)?;
                fixed_range.write(writer)
            }
        }
    }
}

impl ReadFrom for SoundEventHolder {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let holder_id = VarInt::read(data)?.0;
        if holder_id == 0 {
            return Ok(Self::Direct {
                sound_id: Identifier::read(data)?,
                fixed_range: Option::<f32>::read(data)?,
            });
        }
        if holder_id < 0 {
            return Err(Error::other(format!(
                "Negative sound event holder id: {holder_id}"
            )));
        }

        REGISTRY
            .sound_events
            .by_id((holder_id - 1) as usize)
            .map(Self::Registry)
            .ok_or_else(|| Error::other(format!("Unknown sound event holder id: {holder_id}")))
    }
}

impl ToNbtTag for SoundEventHolder {
    fn to_nbt_tag(self) -> NbtTag {
        match self {
            Self::Registry(sound) => sound.key.to_string().to_nbt_tag(),
            Self::Direct {
                sound_id,
                fixed_range,
            } => {
                let mut compound = NbtCompound::new();
                compound.insert("sound_id", sound_id.to_string());
                if let Some(range) = fixed_range {
                    compound.insert("range", range);
                }
                NbtTag::Compound(compound)
            }
        }
    }
}

impl FromNbtTag for SoundEventHolder {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(value) = tag.string() {
            let id = Identifier::from_str(&value.to_str()).ok()?;
            return REGISTRY.sound_events.by_key(&id).map(Self::Registry);
        }

        let compound = tag.compound()?;
        let sound_id = compound
            .get("sound_id")?
            .string()
            .and_then(|value| Identifier::from_str(&value.to_str()).ok())?;
        let fixed_range = compound.get("range").and_then(|tag| tag.float());
        Some(Self::Direct {
            sound_id,
            fixed_range,
        })
    }
}

impl HashComponent for SoundEventHolder {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        match self {
            Self::Registry(sound) => hasher.put_string(&sound.key.to_string()),
            Self::Direct {
                sound_id,
                fixed_range,
            } => {
                let mut entries = Vec::new();
                push_hash_entry(&mut entries, "sound_id", &sound_id.to_string());
                if let Some(range) = fixed_range {
                    push_hash_entry(&mut entries, "range", range);
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
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

pub struct SoundEventRegistry {
    sound_events_by_id: Vec<SoundEventRef>,
    sound_events_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl SoundEventRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sound_events_by_id: Vec::new(),
            sound_events_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    SoundEventRegistry,
    SoundEventRef,
    sound_events_by_id,
    sound_events_by_key,
    allows_registering
);

crate::impl_registry!(
    SoundEventRegistry,
    SoundEvent,
    sound_events_by_id,
    sound_events_by_key,
    sound_events
);
