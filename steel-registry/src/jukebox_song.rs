use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use text_components::TextComponent;

/// Represents a jukebox song definition from a data pack JSON file.
#[derive(Debug)]
pub struct JukeboxSong {
    pub key: Identifier,
    pub sound_event: Identifier,
    pub description: TextComponent,
    pub length_in_seconds: f32,
    pub comparator_output: i32,
}

impl ToNbtTag for &JukeboxSong {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        let sound_event = self.sound_event.to_string();
        compound.insert("sound_event", sound_event.as_str());
        compound.insert("description", (&self.description).to_nbt_tag());
        compound.insert("length_in_seconds", self.length_in_seconds);
        compound.insert("comparator_output", self.comparator_output);
        NbtTag::Compound(compound)
    }
}

pub type JukeboxSongRef = &'static JukeboxSong;

pub struct JukeboxSongRegistry {
    jukebox_songs_by_id: Vec<JukeboxSongRef>,
    jukebox_songs_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl JukeboxSongRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            jukebox_songs_by_id: Vec::new(),
            jukebox_songs_by_key: FxHashMap::default(),
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
