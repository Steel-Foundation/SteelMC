use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::heightmap::{ChunkHeightmaps, Heightmap, HeightmapType};
use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::paletted_container::PalettedContainer;
use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::section::{ChunkSection, SectionHolder, Sections};
use crate::chunk_saver::bit_pack::{bits_for_palette_len, pack_indices, unpack_indices};
use crate::entity::{ENTITIES, SharedEntity};
use crate::world::World;
use crate::world::tick_scheduler::{BlockTickList, FluidTickList, ScheduledTick, TickPriority};
use crate::worldgen::carving_mask::CarvingMask;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use std::cmp::Ordering as CmpOrdering;
use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{io, sync::Weak};
use steel_registry::structure::{LiquidSettingsData, TerrainAdjustment};
use steel_registry::template_pool::{PoolElement, ProcessorList, Projection};
use steel_registry::{REGISTRY, Registry, RegistryEntry, RegistryExt, vanilla_biomes};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, Identifier, PackedChunkPos, Rotation,
};

use crate::world::structure::jigsaw::{JigsawJunction, JigsawPieceData};
use crate::world::structure::{
    StructurePiece, StructureReferenceMap, StructureStart, StructureStartMap,
};

/// Converts `Option<Direction>` to the vanilla 2D data value encoding for persistence.
/// -1 = none, 0 = south, 1 = west, 2 = north, 3 = east.
const fn direction_to_2d(dir: Option<Direction>) -> i8 {
    match dir {
        Some(Direction::South) => 0,
        Some(Direction::West) => 1,
        Some(Direction::North) => 2,
        Some(Direction::East) => 3,
        None | Some(Direction::Down | Direction::Up) => -1,
    }
}

/// Converts a vanilla 2D data value to `Option<Direction>`.
const fn direction_from_2d(value: i8) -> Option<Direction> {
    match value {
        0 => Some(Direction::South),
        1 => Some(Direction::West),
        2 => Some(Direction::North),
        3 => Some(Direction::East),
        _ => None,
    }
}

const fn projection_to_persistent(projection: Option<Projection>) -> i8 {
    match projection {
        None => -1,
        Some(Projection::Rigid) => 0,
        Some(Projection::TerrainMatching) => 1,
    }
}

const fn projection_from_persistent(value: i8) -> Option<Projection> {
    match value {
        0 => Some(Projection::Rigid),
        1 => Some(Projection::TerrainMatching),
        _ => None,
    }
}

const fn required_projection_from_persistent(value: i8) -> Projection {
    match value {
        1 => Projection::TerrainMatching,
        _ => Projection::Rigid,
    }
}

const fn rotation_to_persistent(rotation: Rotation) -> i8 {
    match rotation {
        Rotation::None => 0,
        Rotation::Clockwise90 => 1,
        Rotation::Clockwise180 => 2,
        Rotation::CounterClockwise90 => 3,
    }
}

const fn rotation_from_persistent(value: i8) -> Rotation {
    match value {
        1 => Rotation::Clockwise90,
        2 => Rotation::Clockwise180,
        3 => Rotation::CounterClockwise90,
        _ => Rotation::None,
    }
}

const fn liquid_settings_to_persistent(settings: LiquidSettingsData) -> i8 {
    match settings {
        LiquidSettingsData::ApplyWaterlogging => 0,
        LiquidSettingsData::IgnoreWaterlogging => 1,
    }
}

const fn liquid_settings_from_persistent(value: i8) -> LiquidSettingsData {
    match value {
        1 => LiquidSettingsData::IgnoreWaterlogging,
        _ => LiquidSettingsData::ApplyWaterlogging,
    }
}

fn compare_identifiers(a: &Identifier, b: &Identifier) -> CmpOrdering {
    a.namespace
        .cmp(&b.namespace)
        .then_with(|| a.path.cmp(&b.path))
}

use super::ram_only::RamOnlyStorage;
use super::region_manager::RegionManager;
use super::{
    PersistentBiomeData, PersistentBlockEntity, PersistentBlockState, PersistentChunk,
    PersistentEntity, PersistentHeightmap, PersistentJigsawJunction, PersistentJigsawPieceData,
    PersistentPoi, PersistentPoolElement, PersistentProcessorList, PersistentSection,
    PersistentStructurePiece, PersistentStructureReference, PersistentStructureStart,
    PersistentTick, PreparedChunkSave,
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
            properties,
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
    fn ensure_biome(&mut self, biome_id: u16) -> u16 {
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
    #[expect(
        clippy::similar_names,
        reason = "`pois` vs `pos` are semantically distinct"
    )]
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

        // Serialize heightmaps
        let heightmaps = chunk
            .as_full()
            .map(|c| Self::heightmaps_to_persistent(&c.heightmaps.read()))
            .unwrap_or_default();

        // Serialize structure data (works for both proto and full chunks)
        let structure_starts = Self::structure_starts_to_persistent(&chunk.structure_starts());
        let structure_references =
            Self::structure_references_to_persistent(&chunk.structure_references());

        // Collect POI occupancy data from world storage
        let pois = chunk
            .as_full()
            .map(|c| Self::pois_to_persistent(c, pos))
            .unwrap_or_default();

        let carving_mask = match chunk {
            ChunkAccess::Proto(proto) => proto
                .carving_mask
                .read()
                .as_ref()
                .map(CarvingMask::to_packed_u64s),
            ChunkAccess::Full(_) => None,
            ChunkAccess::Unloaded => unreachable!(),
        };

        let postprocessing = match chunk {
            ChunkAccess::Proto(proto) => {
                proto.postprocessing.read().iter().map(Vec::clone).collect()
            }
            ChunkAccess::Full(_) => Vec::new(),
            ChunkAccess::Unloaded => unreachable!(),
        };

        let persistent = Self::to_persistent(
            chunk.sections(),
            &block_entities,
            &entities,
            block_ticks,
            fluid_ticks,
            heightmaps,
            carving_mask,
            postprocessing,
            structure_starts,
            structure_references,
            pois,
            pos,
        );

        Some(PreparedChunkSave { pos, persistent })
    }

    /// Converts chunk data to persistent format.
    #[expect(
        clippy::too_many_arguments,
        clippy::similar_names,
        reason = "chunk serialization requires all fields; `block_ticks`/`fluid_ticks` are distinct"
    )]
    fn to_persistent(
        sections: &Sections,
        block_entities: &[SharedBlockEntity],
        entities: &[SharedEntity],
        block_ticks: Vec<PersistentTick>,
        fluid_ticks: Vec<PersistentTick>,
        heightmaps: Vec<PersistentHeightmap>,
        carving_mask: Option<Vec<u64>>,
        postprocessing: Vec<Vec<u16>>,
        structure_starts: Vec<PersistentStructureStart>,
        structure_references: Vec<PersistentStructureReference>,
        pois: Vec<PersistentPoi>,
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
            heightmaps,
            carving_mask,
            postprocessing,
            structure_starts,
            structure_references,
            pois,
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
        biomes: &PalettedContainer<u16, 4>,
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

        // Reconstruct structure data
        let structure_starts = Self::persistent_to_structure_starts(&persistent.structure_starts);
        let structure_references =
            Self::persistent_to_structure_references(&persistent.structure_references);

        if status == ChunkStatus::Full {
            // Reconstruct scheduled ticks from persistent data
            let block_ticks = Self::persistent_to_block_ticks(&persistent.block_ticks, pos);
            let fluid_ticks = Self::persistent_to_fluid_ticks(&persistent.fluid_ticks, pos);

            // Reconstruct heightmaps from persistent data
            let heightmaps = Self::persistent_to_heightmaps(&persistent.heightmaps, min_y, height);

            let chunk = LevelChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                min_y,
                height,
                level.clone(),
                block_ticks,
                fluid_ticks,
                heightmaps,
                structure_starts,
                structure_references,
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
                if let Some(entity) = Self::persistent_to_entity(persistent_entity, pos, &chunk) {
                    chunk.add_and_register_entity(entity);
                }
            }

            // Restore POI ticket state (populate_poi ran in from_disk, now apply saved occupancy)
            if !persistent.pois.is_empty()
                && let Some(world) = level.upgrade()
            {
                let tickets: Vec<_> = persistent
                    .pois
                    .iter()
                    .map(|p| {
                        let block_pos = BlockPos::new(
                            pos.0.x * 16 + i32::from(p.x),
                            i32::from(p.y),
                            pos.0.y * 16 + i32::from(p.z),
                        );
                        (block_pos, p.free_tickets)
                    })
                    .collect();
                world.poi_storage.lock().restore_tickets(pos, &tickets);
            }

            // Clear dirty flag since we just loaded (add_and_register marks dirty)
            chunk.dirty.store(false, Ordering::Release);

            ChunkAccess::Full(chunk)
        } else {
            let carving_mask = persistent
                .carving_mask
                .as_deref()
                .map(|packed| CarvingMask::from_packed_u64s(height, min_y, packed));

            ChunkAccess::Proto(ProtoChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                status,
                min_y,
                height,
                structure_starts,
                structure_references,
                carving_mask,
                persistent.postprocessing.iter().map(Vec::clone).collect(),
            ))
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
        use glam::DVec3;
        use uuid::Uuid;

        // Reconstruct base fields
        let pos = DVec3::new(persistent.pos[0], persistent.pos[1], persistent.pos[2]);
        let mut velocity = DVec3::new(
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

    /// Converts chunk heightmaps to persistent format for saving.
    fn heightmaps_to_persistent(heightmaps: &ChunkHeightmaps) -> Vec<PersistentHeightmap> {
        HeightmapType::final_types()
            .iter()
            .enumerate()
            .map(|(i, &hm_type)| {
                let hm = heightmaps.get(hm_type);
                PersistentHeightmap {
                    heightmap_type: i as u8,
                    data: hm.raw_data().to_vec(),
                }
            })
            .collect()
    }

    /// Reconstructs chunk heightmaps from persistent data.
    fn persistent_to_heightmaps(
        persistent: &[PersistentHeightmap],
        min_y: i32,
        height: i32,
    ) -> ChunkHeightmaps {
        let final_types = HeightmapType::final_types();
        let mut heightmaps = ChunkHeightmaps::new(min_y, height);

        for ph in persistent {
            let Some(&hm_type) = final_types.get(ph.heightmap_type as usize) else {
                continue;
            };
            if ph.data.len() != 256 {
                tracing::warn!(
                    "Heightmap data length mismatch: expected 256, got {}. Skipping.",
                    ph.data.len()
                );
                continue;
            }
            let mut data = Box::new([0u16; 256]);
            data.copy_from_slice(&ph.data);
            *heightmaps.get_mut(hm_type) = Heightmap::from_raw_data(hm_type, min_y, height, data);
        }

        heightmaps
    }

    fn jigsaw_piece_data_to_persistent(data: &JigsawPieceData) -> PersistentJigsawPieceData {
        PersistentJigsawPieceData {
            pool_element: Self::pool_element_to_persistent(&data.pool_element),
            position: [data.position.0, data.position.1, data.position.2],
            rotation: rotation_to_persistent(data.rotation),
            liquid_settings: liquid_settings_to_persistent(data.liquid_settings),
        }
    }

    fn persistent_to_jigsaw_piece_data(data: &PersistentJigsawPieceData) -> JigsawPieceData {
        JigsawPieceData {
            pool_element: Self::persistent_to_pool_element(&data.pool_element),
            position: (data.position[0], data.position[1], data.position[2]),
            rotation: rotation_from_persistent(data.rotation),
            liquid_settings: liquid_settings_from_persistent(data.liquid_settings),
        }
    }

    fn pool_element_to_persistent(element: &PoolElement) -> PersistentPoolElement {
        match element {
            PoolElement::Single {
                location,
                processors,
                projection,
            } => PersistentPoolElement::Single {
                location: location.clone(),
                processors: Self::processors_to_persistent(processors),
                projection: projection_to_persistent(Some(*projection)),
            },
            PoolElement::LegacySingle {
                location,
                processors,
                projection,
            } => PersistentPoolElement::LegacySingle {
                location: location.clone(),
                processors: Self::processors_to_persistent(processors),
                projection: projection_to_persistent(Some(*projection)),
            },
            PoolElement::Empty => PersistentPoolElement::Empty,
            PoolElement::Feature {
                feature,
                projection,
            } => PersistentPoolElement::Feature {
                feature: feature.clone(),
                projection: projection_to_persistent(Some(*projection)),
            },
            PoolElement::List {
                elements,
                projection,
            } => PersistentPoolElement::List {
                elements: elements
                    .iter()
                    .map(Self::pool_element_to_persistent)
                    .collect(),
                projection: projection_to_persistent(Some(*projection)),
            },
        }
    }

    fn persistent_to_pool_element(element: &PersistentPoolElement) -> PoolElement {
        match element {
            PersistentPoolElement::Single {
                location,
                processors,
                projection,
            } => PoolElement::Single {
                location: location.clone(),
                processors: Self::persistent_to_processors(processors),
                projection: required_projection_from_persistent(*projection),
            },
            PersistentPoolElement::LegacySingle {
                location,
                processors,
                projection,
            } => PoolElement::LegacySingle {
                location: location.clone(),
                processors: Self::persistent_to_processors(processors),
                projection: required_projection_from_persistent(*projection),
            },
            PersistentPoolElement::Empty => PoolElement::Empty,
            PersistentPoolElement::Feature {
                feature,
                projection,
            } => PoolElement::Feature {
                feature: feature.clone(),
                projection: required_projection_from_persistent(*projection),
            },
            PersistentPoolElement::List {
                elements,
                projection,
            } => PoolElement::List {
                elements: elements
                    .iter()
                    .map(Self::persistent_to_pool_element)
                    .collect(),
                projection: required_projection_from_persistent(*projection),
            },
        }
    }

    fn processors_to_persistent(processors: &ProcessorList) -> PersistentProcessorList {
        match processors {
            ProcessorList::Empty => PersistentProcessorList::Empty,
            ProcessorList::Registry(id) => PersistentProcessorList::Registry(id.clone()),
        }
    }

    fn persistent_to_processors(processors: &PersistentProcessorList) -> ProcessorList {
        match processors {
            PersistentProcessorList::Empty => ProcessorList::Empty,
            PersistentProcessorList::Registry(id) => ProcessorList::Registry(id.clone()),
        }
    }

    /// Converts structure starts to persistent format for saving.
    fn structure_starts_to_persistent(starts: &StructureStartMap) -> Vec<PersistentStructureStart> {
        let mut persistent: Vec<_> = starts
            .values()
            .filter(|start| !start.pieces.is_empty())
            .map(|start| PersistentStructureStart {
                structure: start.structure.clone(),
                chunk_x: start.chunk_pos.0.x,
                chunk_z: start.chunk_pos.0.y,
                references: start.references,
                pieces: start
                    .pieces
                    .iter()
                    .map(|piece| PersistentStructurePiece {
                        piece_type: piece.piece_type.clone(),
                        bounding_box: piece.bounding_box,
                        gen_depth: piece.gen_depth,
                        orientation: direction_to_2d(piece.orientation),
                        nbt_data: piece.nbt_data.clone(),
                        jigsaw: piece
                            .jigsaw
                            .as_ref()
                            .map(Self::jigsaw_piece_data_to_persistent),
                        ground_level_delta: piece.ground_level_delta,
                        projection: projection_to_persistent(piece.projection),
                        junctions: piece
                            .junctions
                            .iter()
                            .map(|junction| PersistentJigsawJunction {
                                source_x: junction.source_x,
                                source_ground_y: junction.source_ground_y,
                                source_z: junction.source_z,
                                delta_y: junction.delta_y,
                                dest_projection: projection_to_persistent(Some(
                                    junction.dest_projection,
                                )),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();

        persistent.sort_by(|a, b| compare_identifiers(&a.structure, &b.structure));
        persistent
    }

    /// Converts structure references to persistent format for saving.
    fn structure_references_to_persistent(
        refs: &StructureReferenceMap,
    ) -> Vec<PersistentStructureReference> {
        let mut persistent: Vec<_> = refs
            .iter()
            .filter(|(_, positions)| !positions.is_empty())
            .map(|(structure, positions)| PersistentStructureReference {
                structure: structure.clone(),
                references: {
                    let mut packed: Vec<_> = positions
                        .iter()
                        .copied()
                        .map(PackedChunkPos::from)
                        .collect();
                    packed.sort_unstable();
                    packed
                },
            })
            .collect();

        persistent.sort_by(|a, b| compare_identifiers(&a.structure, &b.structure));
        persistent
    }

    /// Reconstructs structure starts from persistent data.
    fn persistent_to_structure_starts(
        persistent: &[PersistentStructureStart],
    ) -> StructureStartMap {
        persistent
            .iter()
            .map(|ps| {
                let pieces = ps
                    .pieces
                    .iter()
                    .map(|pp| StructurePiece {
                        piece_type: pp.piece_type.clone(),
                        bounding_box: pp.bounding_box,
                        gen_depth: pp.gen_depth,
                        orientation: direction_from_2d(pp.orientation),
                        nbt_data: pp.nbt_data.clone(),
                        jigsaw: pp
                            .jigsaw
                            .as_ref()
                            .map(Self::persistent_to_jigsaw_piece_data),
                        ground_level_delta: pp.ground_level_delta,
                        junctions: pp
                            .junctions
                            .iter()
                            .map(|junction| JigsawJunction {
                                source_x: junction.source_x,
                                source_ground_y: junction.source_ground_y,
                                source_z: junction.source_z,
                                delta_y: junction.delta_y,
                                dest_projection: required_projection_from_persistent(
                                    junction.dest_projection,
                                ),
                            })
                            .collect(),
                        projection: projection_from_persistent(pp.projection),
                    })
                    .collect();

                let terrain_adjustment = REGISTRY
                    .structures
                    .by_key(&ps.structure)
                    .map_or(TerrainAdjustment::None, |structure| {
                        structure.terrain_adjustment
                    });
                let mut start = StructureStart::new(
                    ps.structure.clone(),
                    ChunkPos::new(ps.chunk_x, ps.chunk_z),
                    pieces,
                    terrain_adjustment,
                );
                start.references = ps.references;
                (ps.structure.clone(), start)
            })
            .collect()
    }

    /// Reconstructs structure references from persistent data.
    fn persistent_to_structure_references(
        persistent: &[PersistentStructureReference],
    ) -> StructureReferenceMap {
        persistent
            .iter()
            .map(|pr| {
                let positions = pr
                    .references
                    .iter()
                    .map(|&packed| packed.to_chunk_pos())
                    .collect();
                (pr.structure.clone(), positions)
            })
            .collect()
    }

    /// Collects POI occupancy data from the world's POI storage for this chunk.
    fn pois_to_persistent(chunk: &LevelChunk, chunk_pos: ChunkPos) -> Vec<PersistentPoi> {
        let Some(world) = chunk.get_level() else {
            return Vec::new();
        };
        world
            .poi_storage
            .lock()
            .collect_for_chunk(chunk_pos)
            .into_iter()
            .map(|(pos, free_tickets)| PersistentPoi {
                x: (pos.0.x - chunk_pos.0.x * 16) as u8,
                y: pos.0.y as i16,
                z: (pos.0.z - chunk_pos.0.y * 16) as u8,
                free_tickets,
            })
            .collect()
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
                let mut indices = unpack_indices(block_data, *bits_per_entry);
                let runtime_palette: Vec<BlockStateId> = palette
                    .iter()
                    .map(|&idx| Self::resolve_block_state(chunk, idx))
                    .collect();
                let mut cube = Box::new([[[BlockStateId(0); 16]; 16]; 16]);
                for plane in cube.iter_mut() {
                    for row in plane {
                        for cell in row {
                            *cell = runtime_palette[indices.next().expect(
                                "this should never fail, we know the iterator is long enough",
                            ) as usize];
                        }
                    }
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
    ) -> PalettedContainer<u16, 4> {
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
                let mut indices = unpack_indices(biome_data, *bits_per_entry);
                let runtime_palette: Vec<u16> = palette
                    .iter()
                    .map(|&idx| Self::resolve_biome(chunk, idx))
                    .collect();
                let mut cube = [[[0u16; 4]; 4]; 4];
                for plane in &mut cube {
                    for row in plane {
                        for cell in row {
                            *cell = runtime_palette[indices.next().expect(
                                "this should never fail, we know the iterator is long enough",
                            ) as usize];
                        }
                    }
                }
                PalettedContainer::from_cube(Box::new(cube))
            }
        }
    }

    /// Resolves a chunk palette index to a runtime `BlockStateId`.
    fn resolve_block_state(chunk: &PersistentChunk, index: u16) -> BlockStateId {
        if let Some(state) = chunk.block_states.get(index as usize)
            && let Some(state_id) = REGISTRY
                .blocks
                .state_id_from_properties(&state.name, &state.properties)
        {
            return state_id;
        }
        BlockStateId(0) // Air fallback
    }

    /// Resolves a chunk palette index to a runtime biome ID.
    fn resolve_biome(chunk: &PersistentChunk, index: u16) -> u16 {
        if let Some(biome_key) = chunk.biomes.get(index as usize)
            && let Some(id) = REGISTRY.biomes.id_from_key(biome_key)
        {
            return id as u16;
        }
        vanilla_biomes::PLAINS.id() as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::{FxHashMap, FxHashSet};

    fn init_registry() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    }

    fn test_structure_piece() -> StructurePiece {
        StructurePiece {
            piece_type: Identifier::new_static("minecraft", "mscorridor"),
            bounding_box: steel_utils::BoundingBox::new(0, 64, 0, 1, 65, 1),
            gen_depth: 0,
            orientation: None,
            nbt_data: Vec::new(),
            jigsaw: None,
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        }
    }

    fn single_empty_section() -> Sections {
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice())
    }

    #[test]
    fn proto_carving_mask_presence_roundtrips_when_empty() {
        init_registry();

        let pos = ChunkPos::new(3, -4);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16);
        proto.set_status(ChunkStatus::Carvers);
        drop(proto.get_or_create_carving_mask());
        let chunk = ChunkAccess::Proto(proto);

        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk) else {
            panic!("dirty proto chunk should prepare for saving");
        };
        assert_eq!(prepared.persistent.carving_mask, Some(Vec::new()));

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Carvers,
            0,
            16,
            Weak::new(),
        );
        let ChunkAccess::Proto(loaded_proto) = loaded else {
            panic!("carvers status should load as proto chunk");
        };

        assert!(loaded_proto.carving_mask.read().is_some());
    }

    #[test]
    fn proto_carving_mask_bits_roundtrip_through_persistent_chunk() {
        init_registry();

        let pos = ChunkPos::new(3, -4);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16);
        proto.set_status(ChunkStatus::Carvers);
        {
            let mut mask = proto.get_or_create_carving_mask();
            mask.set(7, 5, 11);
        }
        let chunk = ChunkAccess::Proto(proto);

        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk) else {
            panic!("dirty proto chunk should prepare for saving");
        };
        assert!(
            prepared
                .persistent
                .carving_mask
                .as_ref()
                .is_some_and(|packed| !packed.is_empty())
        );

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Carvers,
            0,
            16,
            Weak::new(),
        );
        let ChunkAccess::Proto(loaded_proto) = loaded else {
            panic!("carvers status should load as proto chunk");
        };

        let mask_guard = loaded_proto.carving_mask.read();
        let Some(mask) = mask_guard.as_ref() else {
            panic!("carving mask should restore from persistent chunk");
        };
        assert!(mask.get(7, 5, 11));
        assert!(!mask.get(8, 5, 11));
    }

    #[test]
    fn proto_postprocessing_roundtrips_through_persistent_chunk() {
        init_registry();

        let pos = ChunkPos::new(-2, 1);
        let marked = BlockPos::new(-17, -63, 31);
        let proto = ProtoChunk::new(single_empty_section(), pos, -64, 16);
        proto.set_status(ChunkStatus::Noise);
        proto.mark_pos_for_postprocessing(marked);
        let packed = ProtoChunk::pack_postprocessing_offset(marked);
        let chunk = ChunkAccess::Proto(proto);

        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk) else {
            panic!("dirty proto chunk should prepare for saving");
        };

        assert_eq!(prepared.persistent.postprocessing, vec![vec![packed]]);

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Noise,
            -64,
            16,
            Weak::new(),
        );
        let ChunkAccess::Proto(loaded_proto) = loaded else {
            panic!("noise status should load as proto chunk");
        };

        assert_eq!(loaded_proto.postprocessing.read()[0], vec![packed]);
    }

    #[test]
    fn structure_persistence_filters_empty_starts_and_sorts_entries() {
        let alpha = Identifier::new_static("minecraft", "alpha");
        let empty = Identifier::new_static("minecraft", "empty");
        let zeta = Identifier::new_static("minecraft", "zeta");

        let mut starts = FxHashMap::default();
        starts.insert(
            zeta.clone(),
            StructureStart::new(
                zeta.clone(),
                ChunkPos::new(2, 0),
                vec![test_structure_piece()],
                TerrainAdjustment::None,
            ),
        );
        starts.insert(
            empty.clone(),
            StructureStart::new(
                empty,
                ChunkPos::new(1, 0),
                Vec::new(),
                TerrainAdjustment::None,
            ),
        );
        starts.insert(
            alpha.clone(),
            StructureStart::new(
                alpha.clone(),
                ChunkPos::new(0, 0),
                vec![test_structure_piece()],
                TerrainAdjustment::None,
            ),
        );

        let persistent_starts = ChunkStorage::structure_starts_to_persistent(&starts);
        assert_eq!(persistent_starts.len(), 2);
        assert_eq!(persistent_starts[0].structure, alpha);
        assert_eq!(persistent_starts[1].structure, zeta);

        let mut references = StructureReferenceMap::default();
        references.insert(
            Identifier::new_static("minecraft", "zeta"),
            [ChunkPos::new(2, 0), ChunkPos::new(1, 0)]
                .into_iter()
                .collect(),
        );
        references.insert(
            Identifier::new_static("minecraft", "alpha"),
            [ChunkPos::new(4, 0)].into_iter().collect(),
        );
        references.insert(
            Identifier::new_static("minecraft", "empty"),
            FxHashSet::default(),
        );

        let persistent_references = ChunkStorage::structure_references_to_persistent(&references);
        assert_eq!(persistent_references.len(), 2);
        assert_eq!(
            persistent_references[0].structure,
            Identifier::new_static("minecraft", "alpha")
        );
        assert_eq!(
            persistent_references[1].structure,
            Identifier::new_static("minecraft", "zeta")
        );
        assert_eq!(
            persistent_references[1].references,
            vec![
                PackedChunkPos::from(ChunkPos::new(1, 0)),
                PackedChunkPos::from(ChunkPos::new(2, 0))
            ]
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "single fixture verifies every persisted jigsaw field roundtrips together"
    )]
    fn structure_start_roundtrip_preserves_typed_jigsaw_state() {
        init_registry();

        let structure_id = Identifier::new_static("steel", "test_jigsaw_structure");
        let piece_type = Identifier::new_static("minecraft", "jigsaw");
        let template_id = Identifier::new_static("minecraft", "village/plains/houses/test_house");
        let processor_id = Identifier::new_static("minecraft", "street_plains");

        let piece = StructurePiece {
            piece_type: piece_type.clone(),
            bounding_box: steel_utils::BoundingBox::new(10, 64, 20, 15, 70, 25),
            gen_depth: 3,
            orientation: Some(Direction::North),
            nbt_data: vec![1, 2, 3],
            jigsaw: Some(JigsawPieceData {
                pool_element: PoolElement::List {
                    elements: vec![
                        PoolElement::LegacySingle {
                            location: template_id.clone(),
                            processors: ProcessorList::Registry(processor_id.clone()),
                            projection: Projection::Rigid,
                        },
                        PoolElement::Feature {
                            feature: Identifier::new_static("minecraft", "pile_hay"),
                            projection: Projection::TerrainMatching,
                        },
                    ],
                    projection: Projection::Rigid,
                },
                position: (10, 64, 20),
                rotation: Rotation::Clockwise90,
                liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
            }),
            ground_level_delta: 1,
            junctions: vec![JigsawJunction {
                source_x: 12,
                source_ground_y: 65,
                source_z: 24,
                delta_y: -1,
                dest_projection: Projection::TerrainMatching,
            }],
            projection: Some(Projection::Rigid),
        };
        let start = StructureStart::new(
            structure_id.clone(),
            ChunkPos::new(4, -2),
            vec![piece],
            TerrainAdjustment::None,
        );
        let mut starts = FxHashMap::default();
        starts.insert(structure_id.clone(), start);

        let persistent = ChunkStorage::structure_starts_to_persistent(&starts);
        let encoded = wincode::serialize(&persistent).expect("structure starts should serialize");
        let decoded: Vec<PersistentStructureStart> =
            wincode::deserialize(&encoded).expect("structure starts should deserialize");
        let loaded = ChunkStorage::persistent_to_structure_starts(&decoded);

        let loaded_start = loaded
            .get(&structure_id)
            .expect("structure start should roundtrip");
        assert_eq!(loaded_start.chunk_pos, ChunkPos::new(4, -2));
        assert_eq!(loaded_start.pieces.len(), 1);

        let loaded_piece = &loaded_start.pieces[0];
        assert_eq!(loaded_piece.piece_type, piece_type);
        assert_eq!(loaded_piece.gen_depth, 3);
        assert_eq!(loaded_piece.orientation, Some(Direction::North));
        assert_eq!(loaded_piece.nbt_data, [1, 2, 3]);
        assert_eq!(loaded_piece.ground_level_delta, 1);
        assert_eq!(loaded_piece.projection, Some(Projection::Rigid));
        assert_eq!(loaded_piece.junctions.len(), 1);
        assert_eq!(
            loaded_piece.junctions[0].dest_projection,
            Projection::TerrainMatching
        );

        let jigsaw = loaded_piece
            .jigsaw
            .as_ref()
            .expect("typed jigsaw state should roundtrip");
        assert_eq!(jigsaw.position, (10, 64, 20));
        assert_eq!(jigsaw.rotation, Rotation::Clockwise90);
        assert_eq!(
            jigsaw.liquid_settings,
            LiquidSettingsData::IgnoreWaterlogging
        );

        let PoolElement::List {
            elements,
            projection,
        } = &jigsaw.pool_element
        else {
            panic!("expected list pool element");
        };
        assert_eq!(*projection, Projection::Rigid);
        assert_eq!(elements.len(), 2);

        let PoolElement::LegacySingle {
            location,
            processors,
            projection,
        } = &elements[0]
        else {
            panic!("expected legacy single pool element");
        };
        assert_eq!(location, &template_id);
        assert_eq!(processors, &ProcessorList::Registry(processor_id));
        assert_eq!(*projection, Projection::Rigid);

        let PoolElement::Feature {
            feature,
            projection,
        } = &elements[1]
        else {
            panic!("expected feature pool element");
        };
        assert_eq!(feature, &Identifier::new_static("minecraft", "pile_hay"));
        assert_eq!(*projection, Projection::TerrainMatching);
    }
}
