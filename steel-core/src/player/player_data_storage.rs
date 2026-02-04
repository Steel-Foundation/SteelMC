//! Player data storage for saving and loading player state.
//!
//! Saves player data to `players/<uuid>.dat` as gzip-compressed NBT.

use std::{io::Cursor, path::PathBuf, sync::Arc};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use simdnbt::borrow::read_compound as read_borrowed_compound;
use std::io::{Read, Write};
use tokio::{fs, io};
use uuid::Uuid;

use super::player_data::PersistentPlayerData;
use crate::player::Player;

/// Manages player data persistence.
///
/// Stores player data in `players/<uuid>.dat` files using gzip-compressed NBT.
/// This is a server-level storage (not per-world) since player inventory
/// persists across dimensions.
pub struct PlayerDataStorage {
    /// Path to the players directory.
    players_dir: PathBuf,
}

impl PlayerDataStorage {
    /// Creates a new player data storage.
    ///
    /// Creates the `players/` directory if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub async fn new() -> io::Result<Self> {
        let players_dir = PathBuf::from("players");

        // Create directory if it doesn't exist
        if !players_dir.exists() {
            fs::create_dir_all(&players_dir).await?;
        }

        Ok(Self { players_dir })
    }

    /// Returns the path to a player's data file.
    fn get_player_file(&self, uuid: Uuid) -> PathBuf {
        self.players_dir.join(format!("{uuid}.dat"))
    }

    /// Returns the path to a player's temporary data file.
    fn get_temp_file(&self, uuid: Uuid) -> PathBuf {
        self.players_dir.join(format!("{uuid}.dat.tmp"))
    }

    /// Returns the path to a player's backup data file.
    fn get_backup_file(&self, uuid: Uuid) -> PathBuf {
        self.players_dir.join(format!("{uuid}.dat_old"))
    }

    /// Saves a player's data to disk.
    ///
    /// Uses atomic write pattern:
    /// 1. Write to `<uuid>.dat.tmp`
    /// 2. Rename existing `<uuid>.dat` to `<uuid>.dat_old` (backup)
    /// 3. Rename `.tmp` to `.dat`
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub async fn save(&self, player: &Player) -> io::Result<()> {
        let uuid = player.gameprofile.id;
        let data = PersistentPlayerData::from_player(player);

        let temp_path = self.get_temp_file(uuid);
        let final_path = self.get_player_file(uuid);
        let backup_path = self.get_backup_file(uuid);

        // Serialize to NBT and compress (CPU-bound, but fast enough to do inline)
        let nbt = data.to_nbt();
        let mut nbt_bytes = Vec::new();
        nbt.write(&mut nbt_bytes);

        let mut compressed = Vec::new();
        {
            let mut encoder = GzEncoder::new(&mut compressed, Compression::default());
            encoder.write_all(&nbt_bytes)?;
            encoder.finish()?;
        }

        // Write compressed data to temp file
        fs::write(&temp_path, &compressed).await?;

        // Atomic replace: backup old file, rename temp to final
        if final_path.exists() {
            // Remove old backup if it exists
            if backup_path.exists() {
                let _ = fs::remove_file(&backup_path).await;
            }
            // Rename current to backup
            fs::rename(&final_path, &backup_path).await?;
        }

        // Rename temp to final
        fs::rename(&temp_path, &final_path).await?;

        log::debug!("Saved player data for {uuid}");
        Ok(())
    }

    /// Loads a player's data from disk.
    ///
    /// Returns `None` if the player has no saved data (new player).
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub async fn load(&self, uuid: Uuid) -> io::Result<Option<PersistentPlayerData>> {
        let path = self.get_player_file(uuid);

        if !path.exists() {
            return Ok(None);
        }

        // Read compressed data
        let compressed = fs::read(&path).await?;

        // Decompress (CPU-bound, but fast enough to do inline)
        let mut decoder = GzDecoder::new(compressed.as_slice());
        let mut bytes = Vec::new();
        decoder.read_to_end(&mut bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to decompress player data: {e}"),
            )
        })?;

        // Parse NBT
        let nbt = read_borrowed_compound(&mut Cursor::new(&bytes)).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse player NBT: {e}"),
            )
        })?;

        // Deserialize player data
        let data = PersistentPlayerData::from_nbt(&nbt).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid player data format")
        })?;

        log::debug!("Loaded player data for {uuid}");
        Ok(Some(data))
    }

    /// Saves multiple players' data.
    ///
    /// Returns the number of players successfully saved.
    pub async fn save_all(&self, players: &[Arc<Player>]) -> io::Result<usize> {
        let mut saved = 0;
        for player in players {
            match self.save(player).await {
                Ok(()) => saved += 1,
                Err(e) => {
                    log::error!("Failed to save player {}: {e}", player.gameprofile.id);
                }
            }
        }
        Ok(saved)
    }
}
