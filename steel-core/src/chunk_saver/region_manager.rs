//! Region file manager with seek-based chunk access.
//!
//! Uses a sector-based format where only the header (8KB) is kept in memory.
//! Chunk data is read on-demand from disk and converted directly to runtime
//! format, avoiding memory duplication.

use std::{
    io,
    path::PathBuf,
    sync::Weak,
    time::{SystemTime, UNIX_EPOCH},
};

use rustc_hash::FxHashMap;
use steel_registry::{REGISTRY, Registry};
use steel_utils::{BlockStateId, ChunkPos, Identifier};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    level_chunk::LevelChunk,
    paletted_container::PalettedContainer,
    proto_chunk::ProtoChunk,
    section::{ChunkSection, SectionHolder, Sections},
};
use crate::world::World;
use steel_utils::locks::AsyncRwLock;

use super::{
    bit_pack::{bits_for_palette_len, pack_indices, unpack_indices},
    format::{
        BIOMES_PER_SECTION, BLOCKS_PER_SECTION, CHUNK_TABLE_SIZE, FILE_HEADER_SIZE,
        FIRST_DATA_SECTOR, FORMAT_VERSION, MAX_CHUNK_SIZE, PersistentBiomeData,
        PersistentBlockState, PersistentChunk, PersistentSection, REGION_MAGIC, RegionHeader,
        RegionPos, SECTOR_SIZE,
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
    persistent: PersistentChunk,
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

/// Builder for creating a persistent chunk with its own palettes.
struct ChunkBuilder<'a> {
    block_states: Vec<PersistentBlockState>,
    biomes: Vec<Identifier>,
    registry: &'a Registry,
}

impl<'a> ChunkBuilder<'a> {
    fn new(registry: &'a Registry) -> Self {
        Self {
            block_states: Vec::new(),
            biomes: Vec::new(),
            registry,
        }
    }

    /// Ensures a block state exists in the chunk's palette, returning its index.
    fn ensure_block_state(&mut self, block_id: BlockStateId) -> u16 {
        // Get block and properties from registry
        let block = self
            .registry
            .blocks
            .by_state_id(block_id)
            .expect("Invalid block state ID");
        let properties = self.registry.blocks.get_properties(block_id);

        let persistent = PersistentBlockState {
            name: block.key.clone(),
            properties: properties
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };

        // Check if already exists
        if let Some(idx) = self.block_states.iter().position(|s| s == &persistent) {
            return idx as u16;
        }

        // Add new entry
        let idx = self.block_states.len();
        self.block_states.push(persistent);
        idx as u16
    }

    /// Ensures a biome exists in the chunk's palette, returning its index.
    fn ensure_biome(&mut self, biome_id: u8) -> u16 {
        // Get biome identifier from registry
        let biome = self
            .registry
            .biomes
            .by_id(biome_id as usize)
            .expect("Invalid biome ID");
        let identifier = biome.key.clone();

        if let Some(idx) = self.biomes.iter().position(|b| b == &identifier) {
            return idx as u16;
        }

        let idx = self.biomes.len();
        self.biomes.push(identifier);
        idx as u16
    }
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

    /// Saves a chunk to the appropriate region.
    ///
    /// The chunk is serialized, compressed, and written to disk immediately.
    /// If the region was already open (has loaded chunks), the header update is
    /// deferred. If this call opened the region, it will be closed after saving.
    ///
    /// If the chunk is not dirty, this is a no-op and returns `Ok(false)`.
    /// Returns `Ok(true)` if the chunk was saved.
    /// Prepares chunk data for saving. Call this while holding the chunk lock,
    /// then pass the result to `save_chunk_data` after releasing the lock.
    #[must_use]
    pub fn prepare_chunk_save(chunk: &ChunkAccess) -> Option<PreparedChunkSave> {
        if !chunk.is_dirty() {
            return None;
        }

        let pos = chunk.pos();
        let persistent = Self::sections_to_persistent(chunk.sections());

        Some(PreparedChunkSave { pos, persistent })
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

        // Track if we just opened this region
        let was_already_open = regions.contains_key(&region_pos);

        // Get or open the region
        let handle = if let Some(handle) = regions.get_mut(&region_pos) {
            handle
        } else {
            // Try to open region file
            match self.open_region(region_pos).await {
                Ok(handle) => {
                    regions.insert(region_pos, handle);
                    regions.get_mut(&region_pos).expect("just inserted")
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            }
        };

        // Check if chunk exists
        let entry = handle.header.entries[index];
        if !entry.exists() {
            // Clean up if we just opened this region for nothing
            if !was_already_open && handle.loaded_chunk_count == 0 && !handle.header_dirty {
                regions.remove(&region_pos);
            }
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
        let chunk = Self::persistent_to_chunk(&persistent, pos, status, min_y, height, level);

        // Increment ref count
        handle.loaded_chunk_count += 1;

        Ok(Some((chunk, status)))
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

    /// Converts sections to persistent format.
    fn sections_to_persistent(sections: &Sections) -> PersistentChunk {
        let mut builder = ChunkBuilder::new(&REGISTRY);

        let persistent_sections = sections
            .sections
            .iter()
            .map(|section| Self::section_to_persistent(section, &mut builder))
            .collect();

        PersistentChunk {
            last_modified: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as u32)
                .unwrap_or(0),
            block_states: builder.block_states,
            biomes: builder.biomes,
            sections: persistent_sections,
            block_entities: Vec::new(), // TODO: Implement block entity serialization
        }
    }

    /// Converts a runtime section to persistent format.
    fn section_to_persistent(
        section: &SectionHolder,
        builder: &mut ChunkBuilder,
    ) -> PersistentSection {
        let section = section.read();
        let biomes = Self::biomes_to_persistent(&section.biomes, builder);

        match &section.states {
            PalettedContainer::Homogeneous(block_id) => {
                let block_idx = builder.ensure_block_state(*block_id);
                PersistentSection::Homogeneous {
                    block_state: block_idx,
                    biomes,
                }
            }
            PalettedContainer::Heterogeneous(data) => {
                // Build section-local palette (indices into chunk's block_states)
                let palette: Vec<u16> = data
                    .palette
                    .iter()
                    .map(|(block_id, _)| builder.ensure_block_state(*block_id))
                    .collect();

                // Pack block indices (indices into section-local palette)
                let bits = bits_for_palette_len(palette.len())
                    .expect("Heterogeneous section should have palette length >= 2");
                let indices: Vec<u32> = data
                    .cube
                    .iter()
                    .flatten()
                    .flatten()
                    .map(|block_id| {
                        data.palette
                            .iter()
                            .position(|(v, _)| v == block_id)
                            .unwrap_or(0) as u32
                    })
                    .collect();

                let block_data = pack_indices(&indices, bits);

                PersistentSection::Heterogeneous {
                    palette,
                    bits_per_entry: bits,
                    block_data,
                    biomes,
                }
            }
        }
    }

    /// Converts runtime biome data to persistent format.
    fn biomes_to_persistent(
        biomes: &PalettedContainer<u8, 4>,
        builder: &mut ChunkBuilder,
    ) -> PersistentBiomeData {
        match biomes {
            PalettedContainer::Homogeneous(biome_id) => {
                let biome_idx = builder.ensure_biome(*biome_id);
                PersistentBiomeData::Homogeneous { biome: biome_idx }
            }
            PalettedContainer::Heterogeneous(data) => {
                // Build section-local palette (indices into chunk's biomes)
                let palette: Vec<u16> = data
                    .palette
                    .iter()
                    .map(|(biome_id, _)| builder.ensure_biome(*biome_id))
                    .collect();

                let bits = bits_for_palette_len(palette.len())
                    .expect("Heterogeneous biome data should have palette length >= 2");
                let indices: Vec<u32> = data
                    .cube
                    .iter()
                    .flatten()
                    .flatten()
                    .map(|biome_id| {
                        data.palette
                            .iter()
                            .position(|(v, _)| v == biome_id)
                            .unwrap_or(0) as u32
                    })
                    .collect();

                let biome_data = pack_indices(&indices, bits);

                PersistentBiomeData::Heterogeneous {
                    palette,
                    bits_per_entry: bits,
                    biome_data,
                }
            }
        }
    }

    /// Converts a persistent chunk to runtime format.
    /// The returned chunk is not dirty (freshly loaded from disk).
    ///
    /// # Arguments
    /// * `persistent` - The persistent chunk data
    /// * `pos` - The chunk position
    /// * `status` - The chunk status
    /// * `min_y` - The minimum Y coordinate of the world
    /// * `height` - The total height of the world
    /// * `level` - Weak reference to the world for `LevelChunk`
    fn persistent_to_chunk(
        persistent: &PersistentChunk,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> ChunkAccess {
        let sections: Vec<ChunkSection> = persistent
            .sections
            .iter()
            .map(|section| Self::persistent_to_section(section, persistent))
            .collect();

        match status {
            ChunkStatus::Full => ChunkAccess::Full(LevelChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                min_y,
                height,
                level,
            )),
            _ => ChunkAccess::Proto(ProtoChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                status,
                min_y,
                height,
            )),
        }
    }

    /// Converts a persistent section to runtime format.
    fn persistent_to_section(
        persistent: &PersistentSection,
        chunk: &PersistentChunk,
    ) -> ChunkSection {
        match persistent {
            PersistentSection::Homogeneous {
                block_state,
                biomes,
            } => {
                let block_id = Self::resolve_block_state(chunk, *block_state);
                let biome_data = Self::persistent_to_biomes(biomes, chunk);

                ChunkSection::new_with_biomes(PalettedContainer::Homogeneous(block_id), biome_data)
            }
            PersistentSection::Heterogeneous {
                palette,
                bits_per_entry,
                block_data,
                biomes,
            } => {
                // Unpack indices (into section-local palette)
                let indices = unpack_indices(block_data, *bits_per_entry, BLOCKS_PER_SECTION);

                // Build runtime palette by resolving section-local -> chunk -> runtime
                let runtime_palette: Vec<BlockStateId> = palette
                    .iter()
                    .map(|&idx| Self::resolve_block_state(chunk, idx))
                    .collect();

                // Build cube
                let mut cube = Box::new([[[BlockStateId(0); 16]; 16]; 16]);
                for (i, &idx) in indices.iter().enumerate() {
                    let y = i / 256;
                    let z = (i / 16) % 16;
                    let x = i % 16;
                    cube[y][z][x] = runtime_palette
                        .get(idx as usize)
                        .copied()
                        .unwrap_or(BlockStateId(0));
                }

                let states = PalettedContainer::from_cube(cube);
                let biome_data = Self::persistent_to_biomes(biomes, chunk);

                ChunkSection::new_with_biomes(states, biome_data)
            }
        }
    }

    /// Converts persistent biome data to runtime format.
    fn persistent_to_biomes(
        persistent: &PersistentBiomeData,
        chunk: &PersistentChunk,
    ) -> PalettedContainer<u8, 4> {
        match persistent {
            PersistentBiomeData::Homogeneous { biome } => {
                let biome_id = Self::resolve_biome(chunk, *biome);
                PalettedContainer::Homogeneous(biome_id)
            }
            PersistentBiomeData::Heterogeneous {
                palette,
                bits_per_entry,
                biome_data,
            } => {
                let indices = unpack_indices(biome_data, *bits_per_entry, BIOMES_PER_SECTION);

                // Resolve section-local palette -> chunk palette -> runtime
                let runtime_palette: Vec<u8> = palette
                    .iter()
                    .map(|&idx| Self::resolve_biome(chunk, idx))
                    .collect();

                let mut cube = Box::new([[[0u8; 4]; 4]; 4]);
                for (i, &idx) in indices.iter().enumerate() {
                    let y = i / 16;
                    let z = (i / 4) % 4;
                    let x = i % 4;
                    cube[y][z][x] = runtime_palette.get(idx as usize).copied().unwrap_or(0);
                }

                PalettedContainer::from_cube(cube)
            }
        }
    }

    /// Resolves a chunk palette index to a runtime `BlockStateId`.
    fn resolve_block_state(chunk: &PersistentChunk, index: u16) -> BlockStateId {
        if let Some(state) = chunk.block_states.get(index as usize) {
            // Convert properties to the format expected by the registry
            let properties: Vec<(&str, &str)> = state
                .properties
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();

            if let Some(state_id) = REGISTRY
                .blocks
                .state_id_from_properties(&state.name, &properties)
            {
                return state_id;
            }
        }
        BlockStateId(0) // Air fallback
    }

    /// Resolves a chunk palette index to a runtime biome ID.
    fn resolve_biome(chunk: &PersistentChunk, index: u16) -> u8 {
        if let Some(biome_key) = chunk.biomes.get(index as usize)
            && let Some(id) = REGISTRY.biomes.id_from_key(biome_key)
        {
            return id as u8;
        }
        0 // Plains fallback
    }
}
