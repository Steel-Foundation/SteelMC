use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{DowncastType, DowncastTypeKey, Identifier};

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
#[derive(Debug, Clone)]
pub enum SoundEventHolder {
    Registry(SoundEventRef),
    Direct {
        sound_id: Identifier,
        fixed_range: Option<f32>,
    },
}

impl PartialEq for SoundEventHolder {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Registry(left), Self::Registry(right)) => left == right,
            (
                Self::Direct {
                    sound_id: left_id,
                    fixed_range: left_range,
                },
                Self::Direct {
                    sound_id: right_id,
                    fixed_range: right_range,
                },
            ) => left_id == right_id && optional_float_equals(*left_range, *right_range),
            (Self::Registry(_), Self::Direct { .. }) | (Self::Direct { .. }, Self::Registry(_)) => {
                false
            }
        }
    }
}

const fn optional_float_equals(left: Option<f32>, right: Option<f32>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
        }
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

// SAFETY: This Steel-owned key uniquely identifies `SoundEventHolder` within
// the linked process.
unsafe impl DowncastType for SoundEventHolder {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:registry/sound_event_holder");
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

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if let Some(value) = tag.string() {
            let id = Identifier::from_str(&value.to_string()).ok()?;
            return REGISTRY.sound_events.by_key(&id).map(Self::Registry);
        }

        let compound = tag.compound()?;
        let sound_id =
            Identifier::from_str(&compound.get("sound_id")?.string()?.to_string()).ok()?;
        let fixed_range = compound
            .get("range")
            .and_then(steel_utils::nbt::NbtNumeric::codec_f32);
        Some(Self::Direct {
            sound_id,
            fixed_range,
        })
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
        Self::from_owned_nbt(&tag.to_owned())
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag;
    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::Identifier;

    use super::SoundEventHolder;

    fn with_borrowed_tag<R>(tag: NbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    #[test]
    fn direct_sound_range_uses_lenient_numeric_codec() {
        let mut compound = NbtCompound::new();
        compound.insert("sound_id", "minecraft:test");
        compound.insert("range", 5.5_f64);
        let sound = with_borrowed_tag(NbtTag::Compound(compound), SoundEventHolder::from_nbt_tag)
            .expect("direct sound should parse");
        assert!(matches!(
            sound,
            SoundEventHolder::Direct {
                fixed_range: Some(5.5),
                ..
            }
        ));

        let mut malformed = NbtCompound::new();
        malformed.insert("sound_id", "minecraft:test");
        malformed.insert("range", "far");
        let sound = with_borrowed_tag(NbtTag::Compound(malformed), SoundEventHolder::from_nbt_tag)
            .expect("lenient optional range should be ignored");
        assert!(matches!(
            sound,
            SoundEventHolder::Direct {
                fixed_range: None,
                ..
            }
        ));
    }

    #[test]
    fn direct_sound_equality_matches_java_float_rules() {
        let direct = |range| SoundEventHolder::Direct {
            sound_id: Identifier::vanilla_static("test"),
            fixed_range: Some(range),
        };

        assert_eq!(direct(f32::NAN), direct(f32::NAN));
        assert_ne!(direct(0.0), direct(-0.0));
    }
}
