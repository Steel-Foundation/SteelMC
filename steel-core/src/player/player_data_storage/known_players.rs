use tokio::io;
use uuid::Uuid;
use wincode::{SchemaRead, SchemaWrite};

use super::{decode_file, encode_file};
use crate::player::known_players::{GAME_PROFILE_CACHE_LIMIT, KnownPlayer, KnownPlayers};

const KNOWN_PLAYERS_MAGIC: [u8; 4] = *b"STLK";
pub(super) const KNOWN_PLAYERS_STORAGE_VERSION: u16 = 1;
const KNOWN_PLAYERS_DATA_VERSION: i32 = 1;

#[derive(SchemaWrite, SchemaRead)]
pub(super) struct KnownPlayersFile {
    data_version: i32,
    players: Vec<KnownPlayerFile>,
}

#[derive(SchemaWrite, SchemaRead)]
struct KnownPlayerFile {
    uuid: [u8; 16],
    last_known_name: String,
    expires_at_millis: i64,
}

impl KnownPlayersFile {
    pub(super) fn from_known_players(players: &KnownPlayers) -> Self {
        Self {
            data_version: KNOWN_PLAYERS_DATA_VERSION,
            players: players
                .entries()
                .iter()
                .take(GAME_PROFILE_CACHE_LIMIT)
                .map(|player| KnownPlayerFile {
                    uuid: *player.uuid().as_bytes(),
                    last_known_name: player.last_known_name().to_owned(),
                    expires_at_millis: player.expires_at_millis(),
                })
                .collect(),
        }
    }

    pub(super) fn into_known_players(self) -> io::Result<KnownPlayers> {
        if self.data_version != KNOWN_PLAYERS_DATA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported known player index payload version {}",
                    self.data_version
                ),
            ));
        }
        Ok(KnownPlayers::from_entries(self.players.into_iter().map(
            |player| {
                KnownPlayer::with_expiration(
                    Uuid::from_bytes(player.uuid),
                    player.last_known_name,
                    player.expires_at_millis,
                )
            },
        )))
    }
}

pub(super) fn encode_known_players_file(file: &KnownPlayersFile) -> io::Result<Vec<u8>> {
    encode_file(
        KNOWN_PLAYERS_MAGIC,
        KNOWN_PLAYERS_STORAGE_VERSION,
        wincode::serialize(file),
    )
}

pub(super) fn decode_known_players_file(bytes: &[u8]) -> io::Result<KnownPlayersFile> {
    let payload = decode_file(KNOWN_PLAYERS_MAGIC, KNOWN_PLAYERS_STORAGE_VERSION, bytes)?;
    wincode::deserialize(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}
