use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::paletted_container::PalettedContainer;
use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::section::{ChunkSection, SectionHolder, Sections};
use crate::chunk_saver::bit_pack::{bits_for_palette_len, pack_indices, unpack_indices};
use crate::entity::{ENTITIES, SharedEntity};
use crate::world::World;
use crate::world::tick_scheduler::{BlockTickList, FluidTickList, ScheduledTick, TickPriority};
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{io, sync::Weak};
use steel_registry::{REGISTRY, Registry};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Identifier};

use super::ram_only::RamOnlyStorage;
use super::region_manager::RegionManager;
use super::{
    BIOMES_PER_SECTION, BLOCKS_PER_SECTION, PersistentBiomeData, PersistentBlockEntity,
    PersistentBlockState, PersistentChunk, PersistentEntity, PersistentSection, PersistentTick,
    PreparedChunkSave,
};

/// Builder for creating a persistent chunk with its own palettes.
struct ChunkBuilder<'a> {
    block_states: Vec<PersistentBlockState>,
    biomes: Vec<Identifier>,
    registry: &'a Registry,
}

impl<'a> ChunkBuilder<'a> {
    const fn new(registry: &'a Registry) -> Self {
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

/// Chunk storage backend.
///
/// This enum provides persistence for chunks, either to disk (region files)
/// or in-memory (for testing/minigames).
/// TODO: make it possible to give plugins the option to load a custom backend
pub enum ChunkStorage {
    /// Disk-based storage using region files.
    Disk(RegionManager),
    /// In-memory storage for testing and minigames.
    RamOnly(RamOnlyStorage),
}

impl ChunkStorage {
    /// Loads a chunk from storage.
    ///
    /// Returns `Ok(None)` if the chunk doesn't exist in storage.
    /// For `RamOnly` with `create_empty_on_miss=true`, this always
    /// returns an empty chunk (never `None`).
    pub async fn load_chunk(
        &self,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> io::Result<Option<(ChunkAccess, ChunkStatus)>> {
        match self {
            Self::Disk(rm) => rm.load_chunk(pos, min_y, height, level).await,
            Self::RamOnly(ram) => ram.load_chunk(pos, min_y, height, level).await,
        }
    }

    /// Saves prepared chunk data to storage.
    ///
    /// Returns `Ok(true)` if the chunk was saved, `Ok(false)` if it was a no-op.
    pub async fn save_chunk_data(
        &self,
        prepared: PreparedChunkSave,
        status: ChunkStatus,
    ) -> io::Result<bool> {
        match self {
            Self::Disk(rm) => rm.save_chunk_data(prepared, status).await,
            Self::RamOnly(ram) => ram.save_chunk_data(prepared, status).await,
        }
    }

    /// Checks if a chunk exists in storage.
    pub async fn chunk_exists(&self, pos: ChunkPos) -> io::Result<bool> {
        match self {
            Self::Disk(rm) => rm.chunk_exists(pos).await,
            Self::RamOnly(ram) => ram.chunk_exists(pos).await,
        }
    }

    /// Acquires a chunk for loading, preparing any necessary resources.
    ///
    /// For disk storage, this opens/creates the region file and returns
    /// whether the chunk exists. For RAM storage, this just checks existence.
    pub async fn acquire_chunk(&self, pos: ChunkPos) -> io::Result<bool> {
        match self {
            Self::Disk(rm) => rm.acquire_chunk(pos).await,
            Self::RamOnly(ram) => ram.chunk_exists(pos).await,
        }
    }

    /// Releases a loaded chunk, allowing the storage to clean up resources.
    pub async fn release_chunk(&self, pos: ChunkPos) -> io::Result<()> {
        match self {
            Self::Disk(rm) => rm.release_chunk(pos).await,
            Self::RamOnly(_) => Ok(()), // No-op for RAM storage
        }
    }

    /// Flushes all dirty data to storage.
    pub async fn flush_all(&self) -> io::Result<()> {
        match self {
            Self::Disk(rm) => rm.flush_all().await,
            Self::RamOnly(_) => Ok(()), // No-op for RAM storage
        }
    }

    /// Closes all storage handles and flushes pending data.
    pub async fn close_all(&self) -> io::Result<()> {
        match self {
            Self::Disk(rm) => rm.close_all().await,
            Self::RamOnly(_) => Ok(()), // No-op for RAM storage
        }
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

        // Get block entities if this is a full chunk
        let block_entities: Vec<SharedBlockEntity> = chunk
            .as_full()
            .map(LevelChunk::get_block_entities)
            .unwrap_or_default();

        // Get saveable entities if this is a full chunk
        let entities: Vec<SharedEntity> = chunk
            .as_full()
            .map(|c| c.entities.get_saveable_entities())
            .unwrap_or_default();

        // Serialize scheduled ticks
        let (block_ticks, fluid_ticks) = chunk
            .as_full()
            .map(|c| {
                let bt = Self::block_ticks_to_persistent(&c.block_ticks.lock(), pos);
                let ft = Self::fluid_ticks_to_persistent(&c.fluid_ticks.lock(), pos);
                (bt, ft)
            })
            .unwrap_or_default();

        let persistent = Self::to_persistent(
            chunk.sections(),
            &block_entities,
            &entities,
            block_ticks,
            fluid_ticks,
            pos,
        );

        Some(PreparedChunkSave { pos, persistent })
    }

    /// Converts chunk data to persistent format.
    fn to_persistent(
        sections: &Sections,
        block_entities: &[SharedBlockEntity],
        entities: &[SharedEntity],
        block_ticks: Vec<PersistentTick>,
        fluid_ticks: Vec<PersistentTick>,
        chunk_pos: ChunkPos,
    ) -> PersistentChunk {
        let mut builder = ChunkBuilder::new(&REGISTRY);

        let persistent_sections = sections
            .sections
            .iter()
            .map(|section| Self::section_to_persistent(section, &mut builder))
            .collect();

        // Serialize block entities
        let persistent_block_entities: Vec<PersistentBlockEntity> = block_entities
            .iter()
            .map(|entity| {
                let guard = entity.lock();
                let pos = guard.get_block_pos();

                // Serialize NBT data
                let mut nbt = NbtCompound::new();
                guard.save_additional(&mut nbt);
                let mut nbt_bytes = Vec::new();
                nbt.write(&mut nbt_bytes);

                PersistentBlockEntity {
                    x: (pos.0.x - chunk_pos.0.x * 16) as u8,
                    y: pos.0.y as i16,
                    z: (pos.0.z - chunk_pos.0.y * 16) as u8,
                    entity_type: guard.get_type().key.clone(),
                    nbt_data: nbt_bytes,
                }
            })
            .collect();

        // Serialize entities
        let persistent_entities: Vec<PersistentEntity> = entities
            .iter()
            .filter_map(|entity| {
                let pos = entity.position();
                let vel = entity.velocity();
                let (yaw, pitch) = entity.rotation();

                // Validate position is finite (discard corrupted entities)
                if !pos.x.is_finite() || !pos.y.is_finite() || !pos.z.is_finite() {
                    tracing::warn!(
                        uuid = ?entity.uuid(),
                        "Entity has non-finite position {:?}, skipping save",
                        pos
                    );
                    return None;
                }

                // Serialize type-specific NBT data
                let mut nbt = NbtCompound::new();
                entity.save_additional(&mut nbt);
                let mut nbt_bytes = Vec::new();
                nbt.write(&mut nbt_bytes);

                Some(PersistentEntity {
                    entity_type: entity.entity_type().key.clone(),
                    uuid: *entity.uuid().as_bytes(),
                    pos: [pos.x, pos.y, pos.z],
                    motion: [vel.x, vel.y, vel.z],
                    rotation: [yaw, pitch],
                    on_ground: entity.on_ground(),
                    nbt_data: nbt_bytes,
                })
            })
            .collect();

        PersistentChunk {
            last_modified: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as u32),
            block_states: builder.block_states,
            biomes: builder.biomes,
            sections: persistent_sections,
            block_entities: persistent_block_entities,
            entities: persistent_entities,
            block_ticks,
            fluid_ticks,
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
    pub(crate) fn persistent_to_chunk(
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
            ChunkStatus::Full => {
                // Reconstruct scheduled ticks from persistent data
                let block_ticks = Self::persistent_to_block_ticks(&persistent.block_ticks, pos);
                let fluid_ticks = Self::persistent_to_fluid_ticks(&persistent.fluid_ticks, pos);

                let chunk = LevelChunk::from_disk(
                    Sections::from_owned(sections.into_boxed_slice()),
                    pos,
                    min_y,
                    height,
                    level.clone(),
                    block_ticks,
                    fluid_ticks,
                );

                // Load block entities
                for persistent_be in &persistent.block_entities {
                    if let Some(block_entity) =
                        Self::persistent_to_block_entity(persistent_be, pos, &chunk)
                    {
                        chunk.add_and_register_block_entity(block_entity);
                    }
                }

                // Load entities
                for persistent_entity in &persistent.entities {
                    if let Some(entity) = Self::persistent_to_entity(persistent_entity, pos, &chunk)
                    {
                        chunk.add_and_register_entity(entity);
                    }
                }

                // Clear dirty flag since we just loaded (add_and_register marks dirty)
                chunk.dirty.store(false, Ordering::Release);

                ChunkAccess::Full(chunk)
            }
            _ => ChunkAccess::Proto(ProtoChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                status,
                min_y,
                height,
            )),
        }
    }

    /// Converts a persistent block entity to runtime format.
    fn persistent_to_block_entity(
        persistent: &PersistentBlockEntity,
        chunk_pos: ChunkPos,
        chunk: &LevelChunk,
    ) -> Option<SharedBlockEntity> {
        // Calculate absolute position
        let abs_x = chunk_pos.0.x * 16 + i32::from(persistent.x);
        let abs_z = chunk_pos.0.y * 16 + i32::from(persistent.z);
        let pos = BlockPos::new(abs_x, i32::from(persistent.y), abs_z);

        // Get the block state at this position
        let state = chunk.get_block_state(pos);

        // Look up the block entity type
        let block_entity_type = REGISTRY
            .block_entity_types
            .by_key(&persistent.entity_type)?;

        // Get the world reference from the chunk
        let level = chunk.level_weak();

        // Parse and load NBT data
        if persistent.nbt_data.is_empty() {
            // No NBT data, just create the entity without loading
            BLOCK_ENTITIES.create(block_entity_type, level, pos, state)
        } else {
            // Parse NBT from bytes as borrowed
            let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(&persistent.nbt_data)) else {
                return BLOCK_ENTITIES.create(block_entity_type, level, pos, state);
            };

            // Create the block entity and load NBT
            BLOCK_ENTITIES.create_and_load(block_entity_type, level, pos, state, &nbt)
        }
    }

    /// Converts a persistent entity to runtime format.
    fn persistent_to_entity(
        persistent: &PersistentEntity,
        chunk_pos: ChunkPos,
        chunk: &LevelChunk,
    ) -> Option<SharedEntity> {
        use steel_utils::math::Vector3;
        use uuid::Uuid;

        // Reconstruct base fields
        let pos = Vector3::new(persistent.pos[0], persistent.pos[1], persistent.pos[2]);
        let mut velocity = Vector3::new(
            persistent.motion[0],
            persistent.motion[1],
            persistent.motion[2],
        );
        let rotation = (persistent.rotation[0], persistent.rotation[1]);
        let uuid = Uuid::from_bytes(persistent.uuid);

        // Validate position is finite
        if !pos.x.is_finite() || !pos.y.is_finite() || !pos.z.is_finite() {
            tracing::warn!(
                ?uuid,
                "Entity has non-finite position {:?}, skipping load",
                pos
            );
            return None;
        }

        // Validate position is within expected chunk (sanity check)
        let expected_chunk_x = (pos.x as i32) >> 4;
        let expected_chunk_z = (pos.z as i32) >> 4;
        if chunk_pos.0.x != expected_chunk_x || chunk_pos.0.y != expected_chunk_z {
            tracing::warn!(
                ?uuid,
                "Entity position {:?} doesn't match chunk {:?}, loading anyway",
                pos,
                chunk_pos
            );
        }

        // Clamp motion values > 10.0 to 0 (vanilla behavior to prevent corruption)
        if velocity.x.abs() > 10.0 {
            velocity.x = 0.0;
        }
        if velocity.y.abs() > 10.0 {
            velocity.y = 0.0;
        }
        if velocity.z.abs() > 10.0 {
            velocity.z = 0.0;
        }

        // Look up entity type
        let entity_type = REGISTRY.entity_types.by_key(&persistent.entity_type)?;

        // Check if we have a load factory for this entity type
        if !ENTITIES.has_load_factory(entity_type) {
            tracing::debug!(
                entity_type = %persistent.entity_type,
                "No load factory for entity type, skipping"
            );
            return None;
        }

        // Get world reference
        let level = chunk.level_weak();

        // Parse NBT from bytes (or use empty compound data)
        let nbt_bytes = if persistent.nbt_data.is_empty() {
            // Empty NBT compound: type byte (10 = compound), empty name (2 zero bytes), end tag (0)
            &[0x0a, 0x00, 0x00, 0x00][..]
        } else {
            &persistent.nbt_data[..]
        };

        let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(nbt_bytes)) else {
            tracing::warn!(?uuid, "Failed to parse entity NBT, skipping");
            return None;
        };

        ENTITIES.create_and_load(
            entity_type,
            pos,
            uuid,
            velocity,
            rotation,
            persistent.on_ground,
            level,
            &nbt,
        )
    }

    /// Converts block ticks to persistent format for saving.
    fn block_ticks_to_persistent(
        ticks: &BlockTickList,
        chunk_pos: ChunkPos,
    ) -> Vec<PersistentTick> {
        ticks
            .iter()
            .map(|t| PersistentTick {
                x: (t.pos.0.x - chunk_pos.0.x * 16) as u8,
                y: t.pos.0.y as i16,
                z: (t.pos.0.z - chunk_pos.0.y * 16) as u8,
                delay: t.delay,
                priority: t.priority as i8,
                sub_tick_order: t.sub_tick_order,
                tick_type: t.tick_type.key.clone(),
            })
            .collect()
    }

    /// Converts fluid ticks to persistent format for saving.
    fn fluid_ticks_to_persistent(
        ticks: &FluidTickList,
        chunk_pos: ChunkPos,
    ) -> Vec<PersistentTick> {
        ticks
            .iter()
            .map(|t| PersistentTick {
                x: (t.pos.0.x - chunk_pos.0.x * 16) as u8,
                y: t.pos.0.y as i16,
                z: (t.pos.0.z - chunk_pos.0.y * 16) as u8,
                delay: t.delay,
                priority: t.priority as i8,
                sub_tick_order: t.sub_tick_order,
                tick_type: t.tick_type.key.clone(),
            })
            .collect()
    }

    /// Reconstructs block tick list from persistent data.
    fn persistent_to_block_ticks(
        persistent: &[PersistentTick],
        chunk_pos: ChunkPos,
    ) -> BlockTickList {
        let ticks: Vec<_> = persistent
            .iter()
            .filter_map(|pt| {
                let block = REGISTRY.blocks.by_key(&pt.tick_type)?;
                let pos = BlockPos::new(
                    chunk_pos.0.x * 16 + i32::from(pt.x),
                    i32::from(pt.y),
                    chunk_pos.0.y * 16 + i32::from(pt.z),
                );
                let priority = TickPriority::from_i8(pt.priority).unwrap_or(TickPriority::Normal);
                Some(ScheduledTick {
                    tick_type: block,
                    pos,
                    delay: pt.delay,
                    priority,
                    sub_tick_order: pt.sub_tick_order,
                })
            })
            .collect();
        BlockTickList::from_ticks(ticks)
    }

    /// Reconstructs fluid tick list from persistent data.
    fn persistent_to_fluid_ticks(
        persistent: &[PersistentTick],
        chunk_pos: ChunkPos,
    ) -> FluidTickList {
        let ticks: Vec<_> = persistent
            .iter()
            .filter_map(|pt| {
                let fluid = REGISTRY.fluids.by_key(&pt.tick_type)?;
                let pos = BlockPos::new(
                    chunk_pos.0.x * 16 + i32::from(pt.x),
                    i32::from(pt.y),
                    chunk_pos.0.y * 16 + i32::from(pt.z),
                );
                let priority = TickPriority::from_i8(pt.priority).unwrap_or(TickPriority::Normal);
                Some(ScheduledTick {
                    tick_type: fluid,
                    pos,
                    delay: pt.delay,
                    priority,
                    sub_tick_order: pt.sub_tick_order,
                })
            })
            .collect();
        FluidTickList::from_ticks(ticks)
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
