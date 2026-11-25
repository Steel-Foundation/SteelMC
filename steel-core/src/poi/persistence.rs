//! POI columns live in their own region files so searches don't load chunks.
//!
//! The region layout matches chunk storage: fixed sectors, indexed by a header.

use std::{io, path::PathBuf};

use rustc_hash::FxHashMap;
use steel_utils::{ChunkPos, Identifier, locks::AsyncRwLock};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};
use wincode::{SchemaRead, SchemaWrite};

use crate::chunk_saver::{
    CHUNKS_PER_REGION, FILE_HEADER_SIZE, FIRST_DATA_SECTOR, MAX_CHUNK_SIZE, REGION_SIZE, RegionPos,
    SECTOR_SIZE,
};

/// Magic bytes for POI region files: "STLP" (Steel POI).
const POI_REGION_MAGIC: [u8; 4] = *b"STLP";

/// POI region format version, independent from the chunk format.
const POI_FORMAT_VERSION: u16 = 1;

// Each header entry is a sector offset followed by a size, both u32.
const POI_ENTRY_SIZE: usize = 8;

const POI_TABLE_SIZE: usize = CHUNKS_PER_REGION * POI_ENTRY_SIZE;

const ZERO_PADDING: [u8; SECTOR_SIZE] = [0; SECTOR_SIZE];

/// Every POI saved for one chunk column.
#[derive(Debug, Clone, SchemaWrite, SchemaRead)]
pub struct PersistentPoiColumn {
    /// Saved POIs in the column.
    pub records: Vec<PersistentPoiRecord>,
}

/// A POI as written to disk.
#[derive(Debug, Clone, SchemaWrite, SchemaRead)]
pub struct PersistentPoiRecord {
    /// X within the chunk, 0-15.
    pub x: u8,
    /// World-space Y.
    pub y: i16,
    /// Z within the chunk, 0-15.
    pub z: u8,
    /// Registry key of the POI type.
    pub poi_type: Identifier,
    /// Tickets left when saved.
    pub free_tickets: u32,
}

/// Where POI columns are stored.
pub enum PoiStorage {
    /// Region files on disk.
    Disk(PoiRegionManager),
    /// RAM-only storage.
    RamOnly(RamOnlyPoiStorage),
}

impl PoiStorage {
    /// Opens a store backed by region files.
    #[must_use]
    pub fn disk(base_path: impl Into<PathBuf>) -> Self {
        Self::Disk(PoiRegionManager::new(base_path))
    }

    /// Creates a store that keeps columns only in memory.
    #[must_use]
    pub fn ram_only() -> Self {
        Self::RamOnly(RamOnlyPoiStorage::new())
    }

    /// Loads a POI column, if it exists.
    pub async fn load_column(&self, pos: ChunkPos) -> io::Result<Option<PersistentPoiColumn>> {
        match self {
            Self::Disk(manager) => manager.load_column(pos).await,
            Self::RamOnly(storage) => storage.load_column(pos).await,
        }
    }

    /// Writes a POI column.
    pub async fn save_column(&self, pos: ChunkPos, column: &PersistentPoiColumn) -> io::Result<()> {
        match self {
            Self::Disk(manager) => manager.save_column(pos, column).await,
            Self::RamOnly(storage) => storage.save_column(pos, column).await,
        }
    }

    /// Flushes and closes all backend handles.
    pub async fn close_all(&self) -> io::Result<()> {
        match self {
            Self::Disk(manager) => manager.close_all().await,
            Self::RamOnly(_) => Ok(()),
        }
    }
}

/// RAM-only POI storage.
pub struct RamOnlyPoiStorage {
    columns: AsyncRwLock<FxHashMap<ChunkPos, PersistentPoiColumn>>,
}

impl RamOnlyPoiStorage {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            columns: AsyncRwLock::new(FxHashMap::default()),
        }
    }

    async fn load_column(&self, pos: ChunkPos) -> io::Result<Option<PersistentPoiColumn>> {
        Ok(self.columns.read().await.get(&pos).cloned())
    }

    async fn save_column(&self, pos: ChunkPos, column: &PersistentPoiColumn) -> io::Result<()> {
        self.columns.write().await.insert(pos, column.clone());
        Ok(())
    }
}

impl Default for RamOnlyPoiStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// POI storage in region files.
pub struct PoiRegionManager {
    base_path: PathBuf,
    regions: AsyncRwLock<FxHashMap<RegionPos, PoiRegionHandle>>,
}

struct PoiRegionHandle {
    file: File,
    header: PoiRegionHeader,
    file_sectors: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PoiRegionEntry {
    sector_offset: u32,
    size_bytes: u32,
}

impl PoiRegionEntry {
    const fn new(sector_offset: u32, size_bytes: u32) -> Self {
        Self {
            sector_offset,
            size_bytes,
        }
    }

    const fn empty() -> Self {
        Self {
            sector_offset: 0,
            size_bytes: 0,
        }
    }

    const fn exists(self) -> bool {
        self.sector_offset != 0
    }

    const fn sector_count(self) -> u32 {
        if self.size_bytes == 0 {
            0
        } else {
            (self.size_bytes as usize).div_ceil(SECTOR_SIZE) as u32
        }
    }

    const fn to_bytes(self) -> [u8; POI_ENTRY_SIZE] {
        let offset = self.sector_offset.to_le_bytes();
        let size = self.size_bytes.to_le_bytes();
        [
            offset[0], offset[1], offset[2], offset[3], size[0], size[1], size[2], size[3],
        ]
    }

    const fn from_bytes(bytes: [u8; POI_ENTRY_SIZE]) -> Self {
        Self {
            sector_offset: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            size_bytes: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        }
    }
}

struct PoiRegionHeader {
    entries: Box<[PoiRegionEntry; CHUNKS_PER_REGION]>,
}

impl PoiRegionHeader {
    fn new() -> Self {
        Self {
            entries: Box::new([PoiRegionEntry::empty(); CHUNKS_PER_REGION]),
        }
    }

    const fn chunk_index(local_x: usize, local_z: usize) -> usize {
        local_z * REGION_SIZE + local_x
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(POI_TABLE_SIZE);
        for entry in &self.entries {
            bytes.extend_from_slice(&entry.to_bytes());
        }
        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        let mut entries = Box::new([PoiRegionEntry::empty(); CHUNKS_PER_REGION]);
        for (i, chunk) in bytes.as_chunks::<POI_ENTRY_SIZE>().0.iter().enumerate() {
            entries[i] = PoiRegionEntry::from_bytes(*chunk);
        }
        Self { entries }
    }

    fn find_free_sectors(&self, sectors_needed: u32, file_sectors: u32) -> u32 {
        if sectors_needed == 0 {
            return FIRST_DATA_SECTOR;
        }

        let mut used_ranges: Vec<(u32, u32)> = self
            .entries
            .iter()
            .filter(|entry| entry.exists())
            .map(|entry| {
                (
                    entry.sector_offset,
                    entry.sector_offset + entry.sector_count(),
                )
            })
            .collect();
        used_ranges.sort_by_key(|range| range.0);

        let mut current_sector = FIRST_DATA_SECTOR;
        for (start, end) in used_ranges {
            if start >= current_sector + sectors_needed {
                return current_sector;
            }
            current_sector = current_sector.max(end);
        }

        current_sector.max(file_sectors)
    }
}

const fn region_slot(pos: ChunkPos) -> (RegionPos, usize) {
    let region_pos = RegionPos::from_chunk(pos.0.x, pos.0.y);
    let (local_x, local_z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
    (region_pos, PoiRegionHeader::chunk_index(local_x, local_z))
}

impl PoiRegionManager {
    /// Creates a POI region manager rooted at `base_path`.
    #[must_use]
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
            regions: AsyncRwLock::new(FxHashMap::default()),
        }
    }

    fn region_path(&self, pos: RegionPos) -> PathBuf {
        self.base_path.join(pos.filename())
    }

    async fn open_region(
        &self,
        pos: RegionPos,
        create: bool,
    ) -> io::Result<Option<PoiRegionHandle>> {
        let path = self.region_path(pos);
        if !path.exists() {
            if !create {
                return Ok(None);
            }
            return self.create_region(pos).await.map(Some);
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .await?;

        let mut header_bytes = [0u8; FILE_HEADER_SIZE];
        file.read_exact(&mut header_bytes).await?;
        if header_bytes[0..4] != POI_REGION_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid POI region file magic",
            ));
        }

        let version = u16::from_le_bytes([header_bytes[4], header_bytes[5]]);
        if version != POI_FORMAT_VERSION {
            if !create {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("POI region version {version} does not match {POI_FORMAT_VERSION}"),
                ));
            }
            drop(file);
            let backup_path = path.with_extension(format!("srg.v{version}.bak"));
            tracing::warn!(
                "POI region file {} has version {version} (expected {POI_FORMAT_VERSION}), backing up to {} and recreating",
                path.display(),
                backup_path.display()
            );
            fs::rename(&path, &backup_path).await?;
            return self.create_region(pos).await.map(Some);
        }

        let mut table_bytes = vec![0u8; POI_TABLE_SIZE];
        file.read_exact(&mut table_bytes).await?;
        let header = PoiRegionHeader::from_bytes(&table_bytes);

        let file_size = file.seek(io::SeekFrom::End(0)).await?;
        let file_sectors = file_size.div_ceil(SECTOR_SIZE as u64) as u32;
        Self::validate_entries(&header, file_sectors)?;

        Ok(Some(PoiRegionHandle {
            file,
            header,
            file_sectors,
        }))
    }

    async fn create_region(&self, pos: RegionPos) -> io::Result<PoiRegionHandle> {
        fs::create_dir_all(&self.base_path).await?;

        let path = self.region_path(pos);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;

        let mut header_bytes = [0u8; FILE_HEADER_SIZE];
        header_bytes[0..4].copy_from_slice(&POI_REGION_MAGIC);
        header_bytes[4..6].copy_from_slice(&POI_FORMAT_VERSION.to_le_bytes());
        file.write_all(&header_bytes).await?;

        let header = PoiRegionHeader::new();
        file.write_all(&header.to_bytes()).await?;
        file.flush().await?;

        Ok(PoiRegionHandle {
            file,
            header,
            file_sectors: FIRST_DATA_SECTOR,
        })
    }

    fn validate_entries(header: &PoiRegionHeader, file_sectors: u32) -> io::Result<()> {
        let mut occupied = vec![false; file_sectors as usize];
        for sector in occupied.iter_mut().take(FIRST_DATA_SECTOR as usize) {
            *sector = true;
        }

        for (index, &entry) in header.entries.iter().enumerate() {
            if !entry.exists() {
                continue;
            }
            if entry.sector_offset < FIRST_DATA_SECTOR
                || entry.size_bytes == 0
                || entry.size_bytes as usize > MAX_CHUNK_SIZE
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("POI region entry {index} has an invalid allocation"),
                ));
            }
            let Some(end_sector) = entry.sector_offset.checked_add(entry.sector_count()) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "POI region entry sector range overflowed",
                ));
            };
            if end_sector > file_sectors {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "POI region entry {index} ends at sector {end_sector}, past region end {file_sectors}"
                    ),
                ));
            }
            let start = entry.sector_offset as usize;
            let end = start + entry.sector_count() as usize;
            if occupied[start..end].iter().any(|is_occupied| *is_occupied) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("POI region entry {index} overlaps another allocation"),
                ));
            }
            occupied[start..end].fill(true);
        }

        Ok(())
    }

    async fn write_header(file: &mut File, header: &PoiRegionHeader) -> io::Result<()> {
        file.seek(io::SeekFrom::Start(FILE_HEADER_SIZE as u64))
            .await?;
        file.write_all(&header.to_bytes()).await?;
        file.flush().await?;
        Ok(())
    }

    async fn write_column_data(
        file: &mut File,
        sector_offset: u32,
        data: &[u8],
        file_sectors: &mut u32,
    ) -> io::Result<()> {
        let byte_offset = u64::from(sector_offset) * SECTOR_SIZE as u64;
        file.seek(io::SeekFrom::Start(byte_offset)).await?;
        file.write_all(data).await?;

        let padding = (SECTOR_SIZE - (data.len() % SECTOR_SIZE)) % SECTOR_SIZE;
        if padding > 0 {
            file.write_all(&ZERO_PADDING[..padding]).await?;
        }

        let sectors_used = data.len().div_ceil(SECTOR_SIZE) as u32;
        let end_sector = sector_offset + sectors_used;
        if end_sector > *file_sectors {
            *file_sectors = end_sector;
        }

        file.flush().await?;
        Ok(())
    }

    #[expect(clippy::map_entry, reason = "region opening is async")]
    async fn load_column(&self, pos: ChunkPos) -> io::Result<Option<PersistentPoiColumn>> {
        let (region_pos, index) = region_slot(pos);

        let mut regions = self.regions.write().await;
        if !regions.contains_key(&region_pos) {
            let Some(handle) = self.open_region(region_pos, false).await? else {
                return Ok(None);
            };
            regions.insert(region_pos, handle);
        }
        let Some(handle) = regions.get_mut(&region_pos) else {
            return Err(io::Error::other(
                "POI region handle disappeared after opening",
            ));
        };

        let entry = handle.header.entries[index];
        if !entry.exists() {
            return Ok(None);
        }

        let byte_offset = u64::from(entry.sector_offset) * SECTOR_SIZE as u64;
        handle.file.seek(io::SeekFrom::Start(byte_offset)).await?;
        let mut compressed = vec![0u8; entry.size_bytes as usize];
        handle.file.read_exact(&mut compressed).await?;

        let data = zstd::decode_all(&compressed[..])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let column: PersistentPoiColumn = wincode::deserialize_exact(&data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        Ok(Some(column))
    }

    #[expect(clippy::map_entry, reason = "region opening is async")]
    async fn save_column(&self, pos: ChunkPos, column: &PersistentPoiColumn) -> io::Result<()> {
        let data = wincode::serialize(column)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let compressed = zstd::encode_all(&data[..], 3)?;
        if compressed.len() > MAX_CHUNK_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "POI column too large: {} bytes (max {MAX_CHUNK_SIZE})",
                    compressed.len()
                ),
            ));
        }

        let (region_pos, index) = region_slot(pos);

        let mut regions = self.regions.write().await;
        if !regions.contains_key(&region_pos) {
            let Some(handle) = self.open_region(region_pos, true).await? else {
                return Err(io::Error::other("creating a POI region returned no handle"));
            };
            regions.insert(region_pos, handle);
        }
        let Some(handle) = regions.get_mut(&region_pos) else {
            return Err(io::Error::other(
                "POI region handle disappeared after opening",
            ));
        };

        let sectors_needed = compressed.len().div_ceil(SECTOR_SIZE) as u32;
        let old_entry = handle.header.entries[index];
        let sector_offset = if old_entry.exists() && old_entry.sector_count() >= sectors_needed {
            old_entry.sector_offset
        } else {
            // Free the old allocation so the new data can reuse it.
            handle.header.entries[index] = PoiRegionEntry::empty();
            handle
                .header
                .find_free_sectors(sectors_needed, handle.file_sectors)
        };

        Self::write_column_data(
            &mut handle.file,
            sector_offset,
            &compressed,
            &mut handle.file_sectors,
        )
        .await?;

        handle.header.entries[index] = PoiRegionEntry::new(sector_offset, compressed.len() as u32);
        Self::write_header(&mut handle.file, &handle.header).await?;
        Ok(())
    }

    async fn close_all(&self) -> io::Result<()> {
        self.regions.write().await.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        path::PathBuf,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        PersistentPoiColumn, PersistentPoiRecord, PoiRegionEntry, PoiRegionHeader, PoiRegionManager,
    };
    use crate::chunk_saver::{FIRST_DATA_SECTOR, SECTOR_SIZE};
    use steel_utils::{ChunkPos, Identifier};
    use tokio::fs;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn test_directory(name: &str) -> PathBuf {
        let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "steel-poi-storage-{name}-{}-{sequence}",
            process::id()
        ))
    }

    fn column(x: u8, z: u8) -> PersistentPoiColumn {
        PersistentPoiColumn {
            records: vec![PersistentPoiRecord {
                x,
                y: 64,
                z,
                poi_type: Identifier::vanilla_static("home"),
                free_tickets: 1,
            }],
        }
    }

    #[tokio::test]
    async fn columns_round_trip_across_region_reopen() {
        let directory = test_directory("roundtrip");
        let pos = ChunkPos::new(-3, 7);
        let manager = PoiRegionManager::new(&directory);
        manager
            .save_column(pos, &column(2, 9))
            .await
            .expect("POI column should save");
        manager.close_all().await.expect("POI store should close");

        let reopened = PoiRegionManager::new(&directory);
        let loaded = reopened
            .load_column(pos)
            .await
            .expect("POI column should load")
            .expect("POI column should exist");
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].x, 2);
        assert_eq!(loaded.records[0].z, 9);
        assert_eq!(loaded.records[0].y, 64);
        assert_eq!(loaded.records[0].free_tickets, 1);
        reopened.close_all().await.expect("POI store should close");
        fs::remove_dir_all(directory)
            .await
            .expect("test directory should be removable");
    }

    #[test]
    fn freed_sector_ranges_are_reused() {
        let mut header = PoiRegionHeader::new();
        header.entries[0] = PoiRegionEntry::new(FIRST_DATA_SECTOR, SECTOR_SIZE as u32);
        header.entries[1] = PoiRegionEntry::new(FIRST_DATA_SECTOR + 1, SECTOR_SIZE as u32);
        assert_eq!(
            header.find_free_sectors(1, FIRST_DATA_SECTOR + 2),
            FIRST_DATA_SECTOR + 2
        );

        header.entries[0] = PoiRegionEntry::empty();
        assert_eq!(
            header.find_free_sectors(1, FIRST_DATA_SECTOR + 2),
            FIRST_DATA_SECTOR
        );
    }
}
