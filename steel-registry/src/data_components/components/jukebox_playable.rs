//! Vanilla `minecraft:jukebox_playable` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::jukebox_song::{JukeboxSong, JukeboxSongRef, JukeboxSongValue};
use crate::{REGISTRY, RegistryExt, RegistryHolder};

/// A jukebox song attached to an item stack.
///
#[derive(Debug, Clone, PartialEq)]
pub struct JukeboxPlayable {
    song: RegistryHolder<JukeboxSong>,
}

impl JukeboxPlayable {
    #[must_use]
    pub const fn new(song: JukeboxSongRef) -> Self {
        Self {
            song: RegistryHolder::reference(song),
        }
    }

    #[must_use]
    pub const fn direct(song: JukeboxSongValue) -> Self {
        Self {
            song: RegistryHolder::direct(song),
        }
    }

    #[must_use]
    pub const fn song(&self) -> &RegistryHolder<JukeboxSong> {
        &self.song
    }

    /// Decodes `JukeboxSong.CODEC`, which is a registry-fixed holder codec.
    #[must_use]
    pub fn from_persistent_nbt(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let key = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        REGISTRY.jukebox_songs.by_key(&key).map(Self::new)
    }

    /// Encodes `JukeboxSong.CODEC`, which is a registry-fixed holder codec.
    pub fn to_persistent_nbt(&self) -> Result<NbtTag> {
        let Some(song) = self.song.as_reference() else {
            return Err(Error::other("Direct jukebox song holder is not persistent"));
        };
        Ok(NbtTag::String(song.key.to_string().into()))
    }
}

impl HashComponent for JukeboxPlayable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.song.hash_component(hasher);
    }
}

impl ReadFrom for JukeboxPlayable {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        RegistryHolder::read(data).map(|song| Self { song })
    }
}

impl WriteTo for JukeboxPlayable {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.song.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::owned::NbtTag;
    use steel_utils::serial::{ReadFrom, WriteTo};

    use super::JukeboxPlayable;
    use crate::test_support::init_test_registry;
    use crate::vanilla_jukebox_songs;

    #[test]
    fn registry_reference_round_trips_both_codecs() {
        init_test_registry();
        let component = JukeboxPlayable::new(&vanilla_jukebox_songs::CAT);

        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("registry jukebox holder should encode");
        assert_eq!(
            JukeboxPlayable::read(&mut Cursor::new(network.as_slice()))
                .expect("registry jukebox holder should decode"),
            component
        );

        let nbt = component
            .to_persistent_nbt()
            .expect("reference is persistent");
        assert_eq!(nbt, NbtTag::String("minecraft:cat".into()));
    }

    #[test]
    fn direct_holder_round_trips_stream_and_is_not_persistent() {
        use crate::jukebox_song::JukeboxSongValue;
        use crate::sound_event::SoundEventHolder;
        use steel_utils::Identifier;
        use text_components::TextComponent;

        init_test_registry();
        let component = JukeboxPlayable::direct(JukeboxSongValue {
            sound_event: SoundEventHolder::Direct {
                sound_id: Identifier::vanilla_static("custom_song"),
                fixed_range: Some(12.0),
            },
            description: TextComponent::plain("Custom song"),
            length_in_seconds: -1.0,
            comparator_output: 99,
        });
        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("direct holder should encode");
        assert_eq!(
            JukeboxPlayable::read(&mut Cursor::new(network.as_slice()))
                .expect("direct holder should decode"),
            component
        );
        assert!(component.to_persistent_nbt().is_err());
    }
}
