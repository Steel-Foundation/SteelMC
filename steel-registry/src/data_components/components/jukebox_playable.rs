//! Vanilla `minecraft:jukebox_playable` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::jukebox_song::JukeboxSongRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// A jukebox song attached to an item stack.
///
/// Vanilla's persistent codec only accepts registry references, although its
/// stream codec can represent a direct holder. Steel intentionally rejects
/// that direct stream branch so every representable item stack can persist.
#[derive(Debug, Clone, PartialEq)]
pub struct JukeboxPlayable {
    song: JukeboxSongRef,
}

impl JukeboxPlayable {
    #[must_use]
    pub const fn new(song: JukeboxSongRef) -> Self {
        Self { song }
    }

    #[must_use]
    pub const fn song(&self) -> JukeboxSongRef {
        self.song
    }

    /// Decodes `JukeboxSong.CODEC`, which is a registry-fixed holder codec.
    #[must_use]
    pub fn from_persistent_nbt(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let key = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        REGISTRY.jukebox_songs.by_key(&key).map(Self::new)
    }

    /// Encodes `JukeboxSong.CODEC`, which is a registry-fixed holder codec.
    #[must_use]
    pub fn to_persistent_nbt(&self) -> NbtTag {
        NbtTag::String(self.song.key.to_string().into())
    }
}

impl HashComponent for JukeboxPlayable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.song.key.to_string().hash_component(hasher);
    }
}

impl ReadFrom for JukeboxPlayable {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let encoded_id = VarInt::read(data)?.0;
        if encoded_id == 0 {
            return Err(Error::other(
                "Direct jukebox song holders cannot be stored in item stacks",
            ));
        }

        let id = encoded_id
            .checked_sub(1)
            .and_then(|id| usize::try_from(id).ok())
            .ok_or_else(|| Error::other(format!("Invalid jukebox song holder id: {encoded_id}")))?;
        REGISTRY
            .jukebox_songs
            .by_id(id)
            .map(Self::new)
            .ok_or_else(|| Error::other(format!("Unknown jukebox song holder id: {encoded_id}")))
    }
}

impl WriteTo for JukeboxPlayable {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self
            .song
            .try_id()
            .ok_or_else(|| Error::other(format!("Unknown jukebox song: {}", self.song.key)))?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Jukebox song id out of protocol range: {id}")))?;
        let encoded_id = id
            .checked_add(1)
            .ok_or_else(|| Error::other("Jukebox song id exceeds protocol range"))?;
        VarInt(encoded_id).write(writer)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::owned::NbtTag;
    use steel_utils::codec::VarInt;
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

        let nbt = component.to_persistent_nbt();
        assert_eq!(nbt, NbtTag::String("minecraft:cat".into()));
    }

    #[test]
    fn direct_holder_is_rejected_from_item_stack_representation() {
        init_test_registry();
        let mut network = Vec::new();
        VarInt(0)
            .write(&mut network)
            .expect("direct holder discriminator should encode");

        assert!(JukeboxPlayable::read(&mut Cursor::new(network.as_slice())).is_err());
    }
}
