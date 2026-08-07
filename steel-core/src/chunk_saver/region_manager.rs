//! Region file manager with seek-based chunk access.
//!
//! Uses a sector-based format where only the header (8KB) is kept in memory.
//! Chunk data is read on-demand from disk and converted directly to runtime
//! format, avoiding memory duplication.

use std::{
    fmt,
    io::{self},
    path::PathBuf,
    sync::Weak,
};

use rustc_hash::FxHashMap;
use steel_utils::{ChunkPos, locks::AsyncRwLock};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::oneshot,
};

use crate::chunk::status::ChunkStatus;
use crate::world::World;

use super::{
    ChunkStorage, LoadedChunk, PersistentChunk,
    format::{
        CHUNK_TABLE_SIZE, ChunkEntry, FILE_HEADER_SIZE, FIRST_DATA_SECTOR, FORMAT_VERSION,
        MAX_CHUNK_SIZE, REGION_MAGIC, RegionHeader, RegionPos, SECTOR_SIZE,
    },
};

#[derive(Debug)]
struct CorruptChunkData(String);

impl fmt::Display for CorruptChunkData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Manages region files with seek-based chunk access.
///
/// Only keeps region headers (8KB each) in memory, not chunk data.
/// Chunks are loaded on-demand and converted directly to runtime format.
pub struct RegionManager {
    /// Base directory for region files (e.g., "world/region").
    base_path: PathBuf,
    /// Open region file handles with their headers.
    regions: AsyncRwLock<FxHashMap<RegionPos, RegionHandle>>,
}

/// Prepared chunk data ready to be saved asynchronously.
/// Created by `prepare_chunk_save` during the holder's snapshot-preparation phase.
pub struct PreparedChunkSave {
    /// The chunk position.
    pub pos: ChunkPos,
    /// The highest persisted status captured with the chunk data.
    pub status: ChunkStatus,
    /// The serialized chunk data.
    pub persistent: PersistentChunk<'static>,
    /// Runtime manager entity IDs that were either serialized or explicitly skipped.
    pub handled_runtime_entity_ids: Vec<i32>,
}

/// An open region file with its header.
struct RegionHandle {
    /// File handle for reading/writing.
    file: File,
    /// Chunk location header (8KB).
    header: RegionHeader,
    /// Number of chunks currently loaded from this region.
    loaded_chunk_count: usize,
    /// Whether the header has been modified since last save.
    header_dirty: bool,
    /// Current file size in sectors.
    file_sectors: u32,
}

impl RegionManager {
    /// Creates a new region manager.
    ///
    /// # Arguments
    /// * `base_path` - Directory where region files are stored.
    /// * `registry` - The registry for block state and biome conversions.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
            regions: AsyncRwLock::new(FxHashMap::default()),
        }
    }

    /// Gets the file path for a region.
    fn region_path(&self, pos: RegionPos) -> PathBuf {
        self.base_path.join(pos.filename())
    }

    /// Opens or creates a region file, loading only the header.
    async fn open_region(&self, pos: RegionPos) -> io::Result<RegionHandle> {
        let path = self.region_path(pos);

        if !path.exists() {
            // Create new region file with empty header
            return self.create_region(pos).await;
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .await?;

        // Read and verify magic + version
        let mut header_bytes = [0u8; FILE_HEADER_SIZE];
        file.read_exact(&mut header_bytes).await?;

        let magic = &header_bytes[0..4];
        if magic != REGION_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid region file magic",
            ));
        }

        let version = u16::from_le_bytes([header_bytes[4], header_bytes[5]]);
        if version != FORMAT_VERSION {
            // Version mismatch — backup the old file and create a fresh region.
            drop(file);
            let backup_path = path.with_extension(format!("srg.v{version}.bak"));
            tracing::warn!(
                "Region file {} has version {version} (expected {FORMAT_VERSION}), backing up to {} and recreating",
                path.display(),
                backup_path.display()
            );
            fs::rename(&path, &backup_path).await?;
            return self.create_region(pos).await;
        }

        // Read chunk table
        let mut table_bytes = vec![0u8; CHUNK_TABLE_SIZE];
        file.read_exact(&mut table_bytes).await?;
        let header = RegionHeader::from_bytes(&table_bytes).map_err(|index| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("region chunk table entry {index} has an invalid status byte"),
            )
        })?;

        // Calculate file size in sectors
        let file_size = file.seek(io::SeekFrom::End(0)).await?;
        let file_sectors = file_size.div_ceil(SECTOR_SIZE as u64) as u32;
        Self::validate_region_entries(&header, file_sectors)?;

        Ok(RegionHandle {
            file,
            header,
            loaded_chunk_count: 0,
            header_dirty: false,
            file_sectors,
        })
    }

    /// Creates a new empty region file.
    async fn create_region(&self, pos: RegionPos) -> io::Result<RegionHandle> {
        fs::create_dir_all(&self.base_path).await?;

        let path = self.region_path(pos);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;

        // Write header
        let mut header_bytes = [0u8; FILE_HEADER_SIZE];
        header_bytes[0..4].copy_from_slice(&REGION_MAGIC);
        header_bytes[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        file.write_all(&header_bytes).await?;

        // Write empty chunk table
        let header = RegionHeader::new();
        file.write_all(&header.to_bytes()).await?;
        file.flush().await?;

        Ok(RegionHandle {
            file,
            header,
            loaded_chunk_count: 0,
            header_dirty: false,
            file_sectors: FIRST_DATA_SECTOR,
        })
    }

    /// Writes the header to disk.
    async fn write_header(file: &mut File, header: &RegionHeader) -> io::Result<()> {
        file.seek(io::SeekFrom::Start(FILE_HEADER_SIZE as u64))
            .await?;
        file.write_all(&header.to_bytes()).await?;
        file.flush().await?;
        Ok(())
    }

    /// Reads a chunk's compressed data from disk.
    async fn read_chunk_data(
        file: &mut File,
        sector_offset: u32,
        size: u32,
    ) -> io::Result<Vec<u8>> {
        let byte_offset = u64::from(sector_offset) * SECTOR_SIZE as u64;
        file.seek(io::SeekFrom::Start(byte_offset)).await?;

        let mut compressed = vec![0u8; size as usize];
        file.read_exact(&mut compressed).await?;
        Ok(compressed)
    }

    fn validate_chunk_entry(entry: ChunkEntry, file_sectors: u32) -> io::Result<()> {
        if entry.sector_offset < FIRST_DATA_SECTOR {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "chunk table entry points into the region header at sector {}",
                    entry.sector_offset
                ),
            ));
        }
        if entry.size_bytes == 0 || entry.size_bytes as usize > MAX_CHUNK_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid chunk table entry size {}", entry.size_bytes),
            ));
        }
        let Some(end_sector) = entry.sector_offset.checked_add(entry.sector_count()) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk table entry sector range overflowed",
            ));
        };
        if end_sector > file_sectors {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "chunk table entry ends at sector {end_sector}, past region end {file_sectors}"
                ),
            ));
        }
        Ok(())
    }

    fn validate_region_entries(header: &RegionHeader, file_sectors: u32) -> io::Result<()> {
        let mut occupied = vec![false; file_sectors as usize];
        for sector in occupied.iter_mut().take(FIRST_DATA_SECTOR as usize) {
            *sector = true;
        }
        for (index, &entry) in header.entries.iter().enumerate() {
            if !entry.exists() {
                continue;
            }
            Self::validate_chunk_entry(entry, file_sectors)?;
            let start = entry.sector_offset as usize;
            let end = start + entry.sector_count() as usize;
            if occupied[start..end].iter().any(|is_occupied| *is_occupied) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("chunk table entry {index} overlaps another region allocation"),
                ));
            }
            occupied[start..end].fill(true);
        }
        Ok(())
    }

    async fn clear_corrupt_chunk_if_unchanged(
        &self,
        region_pos: RegionPos,
        index: usize,
        expected_entry: ChunkEntry,
    ) -> io::Result<bool> {
        let mut regions = self.regions.write().await;
        let Some(handle) = regions.get_mut(&region_pos) else {
            return Err(io::Error::other(
                "region was released while clearing corrupt chunk data",
            ));
        };
        if handle.header.entries[index] != expected_entry {
            return Ok(false);
        }

        handle.header.entries[index] = ChunkEntry::empty();
        if let Err(error) = Self::write_header(&mut handle.file, &handle.header).await {
            handle.header.entries[index] = expected_entry;
            return Err(error);
        }
        handle.header_dirty = false;
        Ok(true)
    }

    /// Writes chunk data to disk at the specified sector offset.
    async fn write_chunk_data(
        file: &mut File,
        sector_offset: u32,
        data: &[u8],
        file_sectors: &mut u32,
    ) -> io::Result<()> {
        let byte_offset = u64::from(sector_offset) * SECTOR_SIZE as u64;
        file.seek(io::SeekFrom::Start(byte_offset)).await?;
        file.write_all(data).await?;

        // Pad to sector boundary
        let padding_needed = (SECTOR_SIZE - (data.len() % SECTOR_SIZE)) % SECTOR_SIZE;
        if padding_needed > 0 {
            file.write_all(&vec![0u8; padding_needed]).await?;
        }

        // Update file sectors if we wrote past the end
        let sectors_used = data.len().div_ceil(SECTOR_SIZE) as u32;
        let end_sector = sector_offset + sectors_used;
        if end_sector > *file_sectors {
            *file_sectors = end_sector;
        }

        file.flush().await?;
        Ok(())
    }

    /// Saves prepared chunk data to disk after the snapshot-preparation phase has ended.
    #[expect(
        clippy::missing_panics_doc,
        reason = "panic on `just inserted` is unreachable"
    )]
    pub async fn save_chunk_data(
        &self,
        prepared: PreparedChunkSave,
        thread_pool: &rayon::ThreadPool,
    ) -> io::Result<bool> {
        let pos = prepared.pos;
        let status = prepared.status;
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);

        let (sender, receiver) = oneshot::channel();
        thread_pool.spawn(move || {
            let result = Self::encode_chunk(prepared);
            if sender.send(result).is_err() {
                tracing::trace!(
                    chunk = ?pos,
                    "Discarding encoded chunk after its save task was canceled"
                );
            }
        });
        let compressed = receiver.await.map_err(|_| {
            io::Error::other("chunk encode task ended without returning a result")
        })??;

        let mut regions = self.regions.write().await;

        // Track if we opened the region (so we can close it after)
        let we_opened_region = !regions.contains_key(&region_pos);

        // Get or open the region
        let handle = if let Some(handle) = regions.get_mut(&region_pos) {
            handle
        } else {
            let handle = self.open_region(region_pos).await?;
            regions.insert(region_pos, handle);
            regions.get_mut(&region_pos).expect("just inserted")
        };

        // Find space for the chunk
        let sectors_needed = compressed.len().div_ceil(SECTOR_SIZE) as u32;
        let old_entry = handle.header.entries[index];

        // Try to reuse existing space if it fits
        let sector_offset = if old_entry.exists() && old_entry.sector_count() >= sectors_needed {
            old_entry.sector_offset
        } else {
            handle
                .header
                .find_free_sectors(sectors_needed, handle.file_sectors)
        };

        // Write chunk data
        Self::write_chunk_data(
            &mut handle.file,
            sector_offset,
            &compressed,
            &mut handle.file_sectors,
        )
        .await?;

        // Update header entry
        handle.header.entries[index] =
            super::format::ChunkEntry::new(sector_offset, compressed.len() as u32, status);

        // If we opened this region and no chunks are loaded from it,
        // write the header and close it immediately
        if we_opened_region && handle.loaded_chunk_count == 0 {
            Self::write_header(&mut handle.file, &handle.header).await?;
            regions.remove(&region_pos);
        } else {
            handle.header_dirty = true;
        }

        Ok(true)
    }

    fn encode_chunk(prepared: PreparedChunkSave) -> io::Result<Vec<u8>> {
        let data = wincode::serialize(&prepared.persistent)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let compressed = zstd::encode_all(&data[..], 3)?;

        if compressed.len() > MAX_CHUNK_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Chunk too large: {} bytes (max {})",
                    compressed.len(),
                    MAX_CHUNK_SIZE
                ),
            ));
        }

        Ok(compressed)
    }

    /// Loads a chunk from the appropriate region.
    ///
    /// Automatically opens the region if not already open. The region's reference
    /// count is incremented, so you must call `release_chunk` when done with the chunk.
    ///
    /// Returns `Ok(None)` if the chunk doesn't exist on disk.
    ///
    /// # Arguments
    /// * `pos` - The chunk position
    /// * `min_y` - The minimum Y coordinate of the world
    /// * `height` - The total height of the world
    /// * `level` - Weak reference to the world for Full chunk runtime access
    ///
    /// The region must already be acquired via `acquire_chunk` before calling this.
    pub async fn load_chunk(
        &self,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
        thread_pool: &rayon::ThreadPool,
    ) -> io::Result<Option<LoadedChunk>> {
        if height <= 0 || height % 16 != 0 || min_y % 16 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "chunk world range must be section-aligned, got min_y={min_y}, height={height}"
                ),
            ));
        }
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);

        let (compressed, entry) = {
            let mut regions = self.regions.write().await;

            // Get the region (should already be open via acquire_chunk)
            let Some(handle) = regions.get_mut(&region_pos) else {
                log::warn!("load_chunk called without acquire_chunk for region {region_pos:?}");
                return Ok(None);
            };

            // Check if chunk exists
            let entry = handle.header.entries[index];
            if !entry.exists() {
                return Ok(None);
            }

            // Invalid offsets and sizes indicate damage to the region's location
            // table, not a self-contained chunk payload. Do not discard the slot.
            Self::validate_chunk_entry(entry, handle.file_sectors)?;

            // Read chunk data from disk
            let compressed =
                Self::read_chunk_data(&mut handle.file, entry.sector_offset, entry.size_bytes)
                    .await?;
            (compressed, entry)
        };

        // Keep CPU-heavy decoding off the async runtime. Awaiting the Rayon
        // handoff also lets the region-lock waiter woken above make progress.
        let (sender, receiver) = oneshot::channel();
        thread_pool.spawn(move || {
            let result = Self::decode_chunk(compressed, pos, entry.status, min_y, height, level);
            if sender.send(result).is_err() {
                tracing::trace!(
                    chunk = ?pos,
                    "Discarding decoded chunk after its load task was canceled"
                );
            }
        });

        let decoded = receiver
            .await
            .map_err(|_| io::Error::other("chunk decode task ended without returning a result"))?;
        match decoded {
            Ok(loaded) => Ok(Some(loaded)),
            Err(error) => {
                if !self
                    .clear_corrupt_chunk_if_unchanged(region_pos, index, entry)
                    .await?
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "corrupt chunk payload was superseded before it could be removed: {error}"
                        ),
                    ));
                }
                tracing::error!(
                    chunk = ?pos,
                    "Discarded corrupt chunk payload and will regenerate it: {error}",
                );
                Ok(None)
            }
        }
    }

    fn decode_chunk(
        compressed: Vec<u8>,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> Result<LoadedChunk, CorruptChunkData> {
        let data = zstd::decode_all(&compressed[..])
            .map_err(|error| CorruptChunkData(format!("zstd decode failed: {error}")))?;
        let persistent: PersistentChunk<'_> = wincode::deserialize(&data)
            .map_err(|error| CorruptChunkData(format!("chunk decode failed: {error}")))?;

        ChunkStorage::try_persistent_to_chunk(&persistent, pos, status, min_y, height, level)
            .map_err(|error| CorruptChunkData(format!("chunk materialization failed: {error}")))
    }

    /// Acquires a chunk, incrementing the region's reference count.
    ///
    /// This opens or creates the region file. Call this before loading or
    /// generating a chunk, and call `release_chunk` when done with the chunk.
    ///
    /// Returns `Ok(true)` if the chunk exists on disk, `Ok(false)` if it doesn't.
    #[expect(
        clippy::missing_panics_doc,
        reason = "panic on `just inserted` is unreachable"
    )]
    pub async fn acquire_chunk(&self, pos: ChunkPos) -> io::Result<bool> {
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);

        let mut regions = self.regions.write().await;

        // Get or open/create the region
        let handle = if let Some(handle) = regions.get_mut(&region_pos) {
            handle
        } else {
            // open_region creates the file if it doesn't exist
            let handle = self.open_region(region_pos).await?;
            regions.insert(region_pos, handle);
            regions.get_mut(&region_pos).expect("just inserted")
        };

        // Check if chunk exists
        let exists = handle.header.entries[index].exists();

        // Increment ref count
        handle.loaded_chunk_count += 1;

        Ok(exists)
    }

    /// Releases a loaded chunk, decrementing the region's reference count.
    ///
    /// When all chunks from a region are released, the header is saved (if dirty)
    /// and the file handle is closed.
    ///
    /// This must be called for each chunk returned by `load_chunk`.
    pub async fn release_chunk(&self, pos: ChunkPos) -> io::Result<()> {
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);

        let mut regions = self.regions.write().await;

        let should_close = if let Some(handle) = regions.get_mut(&region_pos) {
            handle.loaded_chunk_count = handle.loaded_chunk_count.saturating_sub(1);
            handle.loaded_chunk_count == 0
        } else {
            return Ok(());
        };

        if should_close
            && let Some(mut handle) = regions.remove(&region_pos)
            && handle.header_dirty
        {
            Self::write_header(&mut handle.file, &handle.header).await?;
        }

        Ok(())
    }

    /// Checks if a chunk exists on disk without loading it.
    pub async fn chunk_exists(&self, pos: ChunkPos) -> io::Result<bool> {
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);

        let regions = self.regions.write().await;

        // Check cached header first
        if let Some(handle) = regions.get(&region_pos) {
            return Ok(handle.header.entries[index].exists());
        }

        drop(regions);

        // Need to read header from disk
        let path = self.region_path(region_pos);
        if !path.exists() {
            return Ok(false);
        }

        let mut file = File::open(&path).await?;

        // Skip magic + version
        file.seek(io::SeekFrom::Start(FILE_HEADER_SIZE as u64))
            .await?;

        // Read just the one entry we need (8 bytes at index * 8)
        file.seek(io::SeekFrom::Current((index * 8) as i64)).await?;
        let mut entry_bytes = [0u8; 8];
        file.read_exact(&mut entry_bytes).await?;

        let Some(entry) = super::format::ChunkEntry::from_bytes(entry_bytes) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk table entry has an invalid status byte",
            ));
        };
        Ok(entry.exists())
    }

    /// Flushes all dirty headers to disk.
    pub async fn flush_all(&self) -> io::Result<()> {
        let mut regions = self.regions.write().await;

        for handle in regions.values_mut() {
            if handle.header_dirty {
                Self::write_header(&mut handle.file, &handle.header).await?;
                handle.header_dirty = false;
            }
        }

        Ok(())
    }

    /// Flushes all dirty headers and closes all region file handles.
    ///
    /// This should be called during graceful shutdown after all chunks have been saved.
    /// It ensures all data is persisted and file handles are properly closed.
    pub async fn close_all(&self) -> io::Result<()> {
        let mut regions = self.regions.write().await;

        for (_, mut handle) in regions.drain() {
            if handle.header_dirty {
                Self::write_header(&mut handle.file, &handle.header).await?;
            }
            // File handle is dropped here, closing the file
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        path::Path,
        process,
        sync::{
            Weak,
            atomic::{AtomicU64, Ordering},
        },
    };

    use super::*;
    use crate::chunk_saver::{PersistentChunk, PersistentLightData};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn test_directory(name: &str) -> PathBuf {
        let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "steel-region-manager-{name}-{}-{sequence}",
            process::id()
        ))
    }

    async fn write_test_region(
        directory: &Path,
        pos: ChunkPos,
        payload: &[u8],
        declared_size: u32,
    ) -> io::Result<()> {
        fs::create_dir_all(directory).await?;
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let path = directory.join(region_pos.filename());
        let mut file = File::create(path).await?;
        let mut file_header = [0u8; FILE_HEADER_SIZE];
        file_header[0..4].copy_from_slice(&REGION_MAGIC);
        file_header[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        file.write_all(&file_header).await?;

        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);
        let mut header = RegionHeader::new();
        header.entries[index] =
            ChunkEntry::new(FIRST_DATA_SECTOR, declared_size, ChunkStatus::Empty);
        file.write_all(&header.to_bytes()).await?;
        file.seek(io::SeekFrom::Start(
            u64::from(FIRST_DATA_SECTOR) * SECTOR_SIZE as u64,
        ))
        .await?;
        file.write_all(payload).await?;
        file.flush().await
    }

    fn test_thread_pool() -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("test thread pool should build")
    }

    async fn assert_slot_exists_on_disk(directory: &Path, pos: ChunkPos) {
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let path = directory.join(region_pos.filename());
        let mut file = File::open(path)
            .await
            .expect("test region should remain readable");
        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);
        file.seek(io::SeekFrom::Start(
            FILE_HEADER_SIZE as u64 + (index * 8) as u64,
        ))
        .await
        .expect("chunk table entry should be seekable");
        let mut bytes = [0; 8];
        file.read_exact(&mut bytes)
            .await
            .expect("chunk table entry should be readable");
        assert!(ChunkEntry::from_bytes(bytes).is_some_and(|entry| entry.exists()));
    }

    #[tokio::test]
    async fn invalid_zstd_payload_is_removed_for_regeneration() {
        let directory = test_directory("zstd");
        let pos = ChunkPos::new(0, 0);
        let payload = b"this is not a zstd frame";
        write_test_region(&directory, pos, payload, payload.len() as u32)
            .await
            .expect("test region should be written");

        let manager = RegionManager::new(&directory);
        assert!(
            manager
                .acquire_chunk(pos)
                .await
                .expect("region should open")
        );
        let loaded = manager
            .load_chunk(pos, 0, 16, Weak::new(), &test_thread_pool())
            .await
            .expect("corrupt payload should be handled");
        assert!(loaded.is_none());
        assert!(
            !manager
                .chunk_exists(pos)
                .await
                .expect("header should be readable")
        );
        manager
            .release_chunk(pos)
            .await
            .expect("region should release");

        let reopened = RegionManager::new(&directory);
        assert!(
            !reopened
                .chunk_exists(pos)
                .await
                .expect("header should be flushed")
        );
        fs::remove_dir_all(directory)
            .await
            .expect("test directory should be removable");
    }

    #[tokio::test]
    async fn semantically_invalid_complete_payload_is_removed_for_regeneration() {
        let directory = test_directory("semantic");
        let pos = ChunkPos::new(0, 0);
        let persistent = PersistentChunk {
            last_modified: 0,
            block_states: Vec::new(),
            biomes: Vec::new(),
            sections: Vec::new(),
            block_entities: Vec::new(),
            entities: Vec::new(),
            block_ticks: Vec::new(),
            fluid_ticks: Vec::new(),
            heightmaps: Vec::new(),
            light: PersistentLightData::default(),
            carving_mask: None,
            postprocessing: Vec::new(),
            structure_starts: Vec::new(),
            structure_references: Vec::new(),
            pois: Vec::new(),
        };
        let encoded = wincode::serialize(&persistent).expect("test chunk should encode");
        let payload = zstd::encode_all(encoded.as_slice(), 1).expect("test chunk should compress");
        write_test_region(&directory, pos, &payload, payload.len() as u32)
            .await
            .expect("test region should be written");

        let manager = RegionManager::new(&directory);
        assert!(
            manager
                .acquire_chunk(pos)
                .await
                .expect("region should open")
        );
        let loaded = manager
            .load_chunk(pos, 0, 16, Weak::new(), &test_thread_pool())
            .await
            .expect("semantic corruption should be handled");
        assert!(loaded.is_none());
        assert!(
            !manager
                .chunk_exists(pos)
                .await
                .expect("slot should be cleared")
        );
        manager
            .release_chunk(pos)
            .await
            .expect("region should release");
        fs::remove_dir_all(directory)
            .await
            .expect("test directory should be removable");
    }

    #[tokio::test]
    async fn incomplete_payload_read_is_an_error_and_keeps_slot() {
        let directory = test_directory("short-read");
        let pos = ChunkPos::new(0, 0);
        write_test_region(&directory, pos, &[1, 2, 3], 128)
            .await
            .expect("test region should be written");

        let manager = RegionManager::new(&directory);
        assert!(
            manager
                .acquire_chunk(pos)
                .await
                .expect("region should open")
        );
        let Err(error) = manager
            .load_chunk(pos, 0, 16, Weak::new(), &test_thread_pool())
            .await
        else {
            panic!("short filesystem read must not be treated as payload corruption");
        };
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert!(manager.chunk_exists(pos).await.expect("slot should remain"));
        manager
            .release_chunk(pos)
            .await
            .expect("region should release");
        assert_slot_exists_on_disk(&directory, pos).await;
        fs::remove_dir_all(directory)
            .await
            .expect("test directory should be removable");
    }

    #[tokio::test]
    async fn invalid_world_geometry_is_not_classified_as_chunk_corruption() {
        let directory = test_directory("invalid-world-range");
        let pos = ChunkPos::new(0, 0);
        let payload = b"payload must not be decoded";
        write_test_region(&directory, pos, payload, payload.len() as u32)
            .await
            .expect("test region should be written");

        let manager = RegionManager::new(&directory);
        assert!(
            manager
                .acquire_chunk(pos)
                .await
                .expect("region should open")
        );
        let Err(error) = manager
            .load_chunk(pos, 1, 16, Weak::new(), &test_thread_pool())
            .await
        else {
            panic!("invalid world geometry must fail before decoding the chunk");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(manager.chunk_exists(pos).await.expect("slot should remain"));
        manager
            .release_chunk(pos)
            .await
            .expect("region should release");
        assert_slot_exists_on_disk(&directory, pos).await;
        fs::remove_dir_all(directory)
            .await
            .expect("test directory should be removable");
    }

    #[tokio::test]
    async fn invalid_chunk_status_byte_is_a_structural_error_and_is_preserved() {
        let directory = test_directory("invalid-status");
        let pos = ChunkPos::new(0, 0);
        let payload = b"payload must not be decoded";
        write_test_region(&directory, pos, payload, payload.len() as u32)
            .await
            .expect("test region should be written");

        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let path = directory.join(region_pos.filename());
        let mut file = OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .expect("test region should reopen for corruption");
        file.seek(io::SeekFrom::Start(FILE_HEADER_SIZE as u64 + 7))
            .await
            .expect("status byte should be seekable");
        file.write_all(&[u8::MAX])
            .await
            .expect("status byte should be writable");
        file.flush().await.expect("status byte should be flushed");
        drop(file);

        let manager = RegionManager::new(&directory);
        let Err(error) = manager.acquire_chunk(pos).await else {
            panic!("invalid status byte must reject the region header");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let mut file = File::open(path)
            .await
            .expect("rejected region should remain readable");
        file.seek(io::SeekFrom::Start(FILE_HEADER_SIZE as u64 + 7))
            .await
            .expect("status byte should remain seekable");
        let mut status = [0];
        file.read_exact(&mut status)
            .await
            .expect("status byte should remain readable");
        assert_eq!(status[0], u8::MAX);

        fs::remove_dir_all(directory)
            .await
            .expect("test directory should be removable");
    }
}
