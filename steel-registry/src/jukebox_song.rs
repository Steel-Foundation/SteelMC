use rustc_hash::FxHashMap;
use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::TextComponent;

use crate::sound_event::SoundEventHolder;
use crate::{REGISTRY, RegistryExt, RegistryHolderEntry};

#[derive(Debug, Clone)]
pub struct JukeboxSongValue {
    pub sound_event: SoundEventHolder,
    pub description: TextComponent,
    pub length_in_seconds: f32,
    pub comparator_output: i32,
}

impl PartialEq for JukeboxSongValue {
    fn eq(&self, other: &Self) -> bool {
        self.sound_event == other.sound_event
            && self.description == other.description
            && ((self.length_in_seconds.is_nan() && other.length_in_seconds.is_nan())
                || self.length_in_seconds.to_bits() == other.length_in_seconds.to_bits())
            && self.comparator_output == other.comparator_output
    }
}

impl WriteTo for JukeboxSongValue {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.sound_event.write(writer)?;
        self.description.write(writer)?;
        self.length_in_seconds.write(writer)?;
        steel_utils::codec::VarInt(self.comparator_output).write(writer)
    }
}

impl ReadFrom for JukeboxSongValue {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            sound_event: SoundEventHolder::read(data)?,
            description: TextComponent::read(data)?,
            length_in_seconds: f32::read(data)?,
            comparator_output: steel_utils::codec::VarInt::read(data)?.0,
        })
    }
}

impl ToNbtTag for JukeboxSongValue {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("sound_event", self.sound_event.to_nbt_tag());
        compound.insert("description", self.description.to_codec_nbt());
        compound.insert("length_in_seconds", self.length_in_seconds);
        compound.insert("comparator_output", self.comparator_output);
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for JukeboxSongValue {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let length_in_seconds = compound.get("length_in_seconds")?.codec_f32()?;
        let comparator_output = compound.get("comparator_output")?.codec_i32()?;
        if !length_in_seconds.is_finite()
            || length_in_seconds <= 0.0
            || !(0..=15).contains(&comparator_output)
        {
            return None;
        }
        Some(Self {
            sound_event: SoundEventHolder::from_nbt_tag(compound.get("sound_event")?)?,
            description: TextComponent::from_nbt(&compound.get("description")?.to_owned())?,
            length_in_seconds,
            comparator_output,
        })
    }
}

impl HashComponent for JukeboxSongValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.clone().to_nbt_tag().hash_component(hasher);
    }
}

/// Represents a jukebox song definition from a data pack JSON file.
#[derive(Debug)]
pub struct JukeboxSong {
    pub key: Identifier,
    value: JukeboxSongValue,
}

impl JukeboxSong {
    #[must_use]
    pub const fn new(key: Identifier, value: JukeboxSongValue) -> Self {
        Self { key, value }
    }
    #[must_use]
    pub const fn value(&self) -> &JukeboxSongValue {
        &self.value
    }
}

impl ToNbtTag for &JukeboxSong {
    fn to_nbt_tag(self) -> NbtTag {
        self.value.clone().to_nbt_tag()
    }
}

pub type JukeboxSongRef = &'static JukeboxSong;

pub struct JukeboxSongRegistry {
    jukebox_songs_by_id: Vec<JukeboxSongRef>,
    jukebox_songs_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl JukeboxSongRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            jukebox_songs_by_id: Vec::new(),
            jukebox_songs_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    JukeboxSongRegistry,
    JukeboxSongRef,
    jukebox_songs_by_id,
    jukebox_songs_by_key,
    allows_registering
);

crate::impl_registry!(
    JukeboxSongRegistry,
    JukeboxSong,
    jukebox_songs_by_id,
    jukebox_songs_by_key,
    jukebox_songs
);
crate::impl_tagged_registry!(JukeboxSongRegistry, jukebox_songs_by_key, "jukebox song");

impl RegistryHolderEntry for JukeboxSong {
    type Value = JukeboxSongValue;
    const REGISTRY_NAME: &'static str = "jukebox song";
    fn holder_value(&self) -> &Self::Value {
        &self.value
    }
    fn holder_by_id(id: usize) -> Option<&'static Self> {
        REGISTRY.jukebox_songs.by_id(id)
    }
    fn holder_by_key(key: &Identifier) -> Option<&'static Self> {
        REGISTRY.jukebox_songs.by_key(key)
    }
}
