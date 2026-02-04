use crate::RegistryExt;
use rustc_hash::FxHashMap;
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

    pub fn register(&mut self, jukebox_song: JukeboxSongRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register jukebox songs after the registry has been frozen"
        );

        let id = self.jukebox_songs_by_id.len();
        self.jukebox_songs_by_key
            .insert(jukebox_song.key.clone(), id);
        self.jukebox_songs_by_id.push(jukebox_song);
        id
    }

    /// Replaces a song at a given index.
    /// Returns true if the song was replaced and false if the song wasn't replaced
    #[must_use]
    pub fn replace(&mut self, song: JukeboxSongRef, id: usize) -> bool {
        if id >= self.jukebox_songs_by_id.len() {
            return false;
        }
        self.jukebox_songs_by_id[id] = song;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<JukeboxSongRef> {
        self.jukebox_songs_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, jukebox_song: JukeboxSongRef) -> &usize {
        self.jukebox_songs_by_key
            .get(&jukebox_song.key)
            .expect("Jukebox song not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<JukeboxSongRef> {
        self.jukebox_songs_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, JukeboxSongRef)> + '_ {
        self.jukebox_songs_by_id
            .iter()
            .enumerate()
            .map(|(id, &song)| (id, song))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.jukebox_songs_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.jukebox_songs_by_id.is_empty()
    }
}

impl RegistryExt for JukeboxSongRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for JukeboxSongRegistry {
    fn default() -> Self {
        Self::new()
    }
}
