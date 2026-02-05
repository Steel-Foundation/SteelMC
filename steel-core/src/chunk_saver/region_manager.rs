//! Region file manager with seek-based chunk access.
//!
//! Uses a sector-based format where only the header (8KB) is kept in memory.
//! Chunk data is read on-demand from disk and converted directly to runtime
//! format, avoiding memory duplication.

use std::{
    io::{self},
    path::PathBuf,
    sync::Weak,
};

use rustc_hash::FxHashMap;
use steel_utils::{ChunkPos, locks::AsyncRwLock};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::world::World;

use super::{
    ChunkStorage, PersistentChunk,
    format::{
        CHUNK_TABLE_SIZE, FILE_HEADER_SIZE, FIRST_DATA_SECTOR, FORMAT_VERSION, MAX_CHUNK_SIZE,
        REGION_MAGIC, RegionHeader, RegionPos, SECTOR_SIZE,
    },
};

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
/// Created by `prepare_chunk_save` while holding the chunk lock.
pub struct PreparedChunkSave {
    /// The chunk position.
    pub pos: ChunkPos,
    /// The serialized chunk data.
    pub persistent: PersistentChunk,
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
        if version > FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Region file version {version} is newer than supported version {FORMAT_VERSION}"
                ),
            ));
        }

        // Read chunk table
        let mut table_bytes = vec![0u8; CHUNK_TABLE_SIZE];
        file.read_exact(&mut table_bytes).await?;
        let header = RegionHeader::from_bytes(&table_bytes);

        // Calculate file size in sectors
        let file_size = file.seek(io::SeekFrom::End(0)).await?;
        let file_sectors = file_size.div_ceil(SECTOR_SIZE as u64) as u32;

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

    /// Saves prepared chunk data to disk. This is the async part that doesn't
    /// need to hold the chunk lock.
    #[allow(clippy::missing_panics_doc)]
    pub async fn save_chunk_data(
        &self,
        prepared: PreparedChunkSave,
        status: ChunkStatus,
    ) -> io::Result<bool> {
        let pos = prepared.pos;
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);

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

        // Serialize the prepared data
        let persistent = prepared.persistent;
        let data = wincode::serialize(&persistent)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Compress with zstd
        let compressed = zstd::encode_all(&data[..], 3)?;

        if compressed.len() > MAX_CHUNK_SIZE {
            // Clean up if we opened the region
            if we_opened_region {
                regions.remove(&region_pos);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Chunk too large: {} bytes (max {})",
                    compressed.len(),
                    MAX_CHUNK_SIZE
                ),
            ));
        }

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
    /// * `level` - Weak reference to the world for `LevelChunk`
    ///
    /// The region must already be acquired via `acquire_chunk` before calling this.
    #[allow(clippy::missing_panics_doc)]
    pub async fn load_chunk(
        &self,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> io::Result<Option<(ChunkAccess, ChunkStatus)>> {
        let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(local_x, local_z);

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

        // Read chunk data from disk
        let compressed =
            Self::read_chunk_data(&mut handle.file, entry.sector_offset, entry.size_bytes).await?;

        // Decompress
        let data = zstd::decode_all(&compressed[..])?;

        // Deserialize
        let persistent: PersistentChunk = wincode::deserialize(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Convert to runtime format (persistent is dropped after this - no duplication!)
        let status = entry.status;
        let chunk =
            ChunkStorage::persistent_to_chunk(&persistent, pos, status, min_y, height, level);

        Ok(Some((chunk, status)))
    }

    /// Acquires a chunk, incrementing the region's reference count.
    ///
    /// This opens or creates the region file. Call this before loading or
    /// generating a chunk, and call `release_chunk` when done with the chunk.
    ///
    /// Returns `Ok(true)` if the chunk exists on disk, `Ok(false)` if it doesn't.
    #[allow(clippy::missing_panics_doc)]
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

        let entry = super::format::ChunkEntry::from_bytes(entry_bytes);
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
