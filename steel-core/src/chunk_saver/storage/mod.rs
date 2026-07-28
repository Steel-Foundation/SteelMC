use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::heightmap::{ChunkHeightmaps, Heightmap, HeightmapType};
use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::light::{
    ChunkLightData, ChunkLightLayerStorage, DATA_LAYER_SIZE, LightSection, LightSectionData,
};
use crate::chunk::paletted_container::PalettedContainer;
use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::section::{ChunkSection, SectionHolder, Sections};
use crate::chunk_saver::bit_pack::{bits_for_palette_len, pack_indices, unpack_indices};
use crate::entity::{
    ENTITIES, Entity, EntityBase, EntityBaseSaveData, EntityFireFreezeState, EntityLoadRequest,
    MAX_ENTITY_TAGS, RemovalReason, SharedEntity,
};
use crate::world::World;
use crate::world::tick_scheduler::{BlockTickList, FluidTickList, SavedTick, TickPriority};
use crate::worldgen::carving_mask::CarvingMask;
use glam::{DVec3, IVec3};
use rustc_hash::FxHashSet;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use std::cmp::Ordering as CmpOrdering;
use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    io,
    sync::{Arc, Weak},
};
use steel_registry::structure::{
    LiquidSettingsData, OceanRuinBiomeTempData, RuinedPortalPlacementData, TerrainAdjustment,
};
use steel_registry::template_pool::{PoolElement, ProcessorList, Projection};
use steel_registry::{
    REGISTRY, Registry, RegistryEntry, RegistryExt,
    blocks::{BlockRef, block_state_ext::BlockStateExt as _},
    fluid::FluidRef,
    vanilla_biomes,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, Identifier, PackedChunkPos, Rotation,
};
use text_components::TextComponent;

use steel_worldgen::structure::desert_pyramid::DesertPyramidPieceData;
use steel_worldgen::structure::fortress::FortressPieceData;
use steel_worldgen::structure::jigsaw::{JigsawJunction, JigsawPieceData};
use steel_worldgen::structure::jungle_temple::JungleTemplePieceData;
use steel_worldgen::structure::mineshaft::{
    MineshaftPieceKind, MineshaftPiecePayload, MineshaftType,
};
use steel_worldgen::structure::ocean_monument::{
    OceanMonumentChildPiece, OceanMonumentChildPieceKind, OceanMonumentPieceData,
    OceanMonumentRoomData,
};
use steel_worldgen::structure::stronghold::{StrongholdPieceData, StrongholdSmallDoorType};
use steel_worldgen::structure::swamp_hut::SwampHutPieceData;
use steel_worldgen::structure::{
    ProceduralPieceData, RuinedPortalProperties, StructureBlockIgnore, StructureMirror,
    StructurePiece, StructurePiecePayload, StructureReferenceMap, StructureStart,
    StructureStartMap, TemplateMarkerHandling, TemplatePieceData, TemplatePlacementAdjustment,
    TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

mod chunk;
mod entities;
mod light;
mod structures;
mod ticks;

#[cfg(test)]
mod tests;

const ENTITY_LOAD_MAX_HORIZONTAL_POSITION: f64 = 3.000_051_2E7;
const ENTITY_LOAD_MAX_VERTICAL_POSITION: f64 = 2.0E7;

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

const fn required_direction_from_2d(value: i8) -> Direction {
    match value {
        1 => Direction::West,
        2 => Direction::North,
        3 => Direction::East,
        _ => Direction::South,
    }
}

const fn mineshaft_type_to_persistent(mineshaft_type: MineshaftType) -> i8 {
    match mineshaft_type {
        MineshaftType::Normal => 0,
        MineshaftType::Mesa => 1,
    }
}

const fn mineshaft_type_from_persistent(value: i8) -> MineshaftType {
    match value {
        1 => MineshaftType::Mesa,
        _ => MineshaftType::Normal,
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

const fn ruined_portal_placement_to_persistent(placement: RuinedPortalPlacementData) -> i8 {
    match placement {
        RuinedPortalPlacementData::OnLandSurface => 0,
        RuinedPortalPlacementData::PartlyBuried => 1,
        RuinedPortalPlacementData::Underground => 2,
        RuinedPortalPlacementData::InMountain => 3,
        RuinedPortalPlacementData::OnOceanFloor => 4,
        RuinedPortalPlacementData::InNether => 5,
    }
}

const fn ruined_portal_placement_from_persistent(value: i8) -> RuinedPortalPlacementData {
    match value {
        1 => RuinedPortalPlacementData::PartlyBuried,
        2 => RuinedPortalPlacementData::Underground,
        3 => RuinedPortalPlacementData::InMountain,
        4 => RuinedPortalPlacementData::OnOceanFloor,
        5 => RuinedPortalPlacementData::InNether,
        _ => RuinedPortalPlacementData::OnLandSurface,
    }
}

const fn mirror_to_persistent(mirror: StructureMirror) -> i8 {
    match mirror {
        StructureMirror::None => 0,
        StructureMirror::FrontBack => 1,
        StructureMirror::LeftRight => 2,
    }
}

const fn mirror_from_persistent(value: i8) -> StructureMirror {
    match value {
        1 => StructureMirror::FrontBack,
        2 => StructureMirror::LeftRight,
        _ => StructureMirror::None,
    }
}

const fn block_ignore_to_persistent(block_ignore: StructureBlockIgnore) -> i8 {
    match block_ignore {
        StructureBlockIgnore::None => 0,
        StructureBlockIgnore::StructureBlock => 1,
        StructureBlockIgnore::StructureAndAir => 2,
    }
}

const fn block_ignore_from_persistent(value: i8) -> StructureBlockIgnore {
    match value {
        1 => StructureBlockIgnore::StructureBlock,
        2 => StructureBlockIgnore::StructureAndAir,
        _ => StructureBlockIgnore::None,
    }
}

const fn marker_handling_to_persistent(marker_handling: TemplateMarkerHandling) -> i8 {
    match marker_handling {
        TemplateMarkerHandling::Ignore => 0,
        TemplateMarkerHandling::DataMarkers => 1,
        TemplateMarkerHandling::Shipwreck => 2,
        TemplateMarkerHandling::Igloo => 3,
        TemplateMarkerHandling::OceanRuin { is_large: false } => 4,
        TemplateMarkerHandling::OceanRuin { is_large: true } => 5,
        TemplateMarkerHandling::EndCity => 6,
        TemplateMarkerHandling::WoodlandMansion => 7,
    }
}

const fn marker_handling_from_persistent(value: i8) -> TemplateMarkerHandling {
    match value {
        1 => TemplateMarkerHandling::DataMarkers,
        2 => TemplateMarkerHandling::Shipwreck,
        3 => TemplateMarkerHandling::Igloo,
        4 => TemplateMarkerHandling::OceanRuin { is_large: false },
        5 => TemplateMarkerHandling::OceanRuin { is_large: true },
        6 => TemplateMarkerHandling::EndCity,
        7 => TemplateMarkerHandling::WoodlandMansion,
        _ => TemplateMarkerHandling::Ignore,
    }
}

const fn ocean_ruin_biome_temp_to_persistent(biome_temp: OceanRuinBiomeTempData) -> i8 {
    match biome_temp {
        OceanRuinBiomeTempData::Warm => 0,
        OceanRuinBiomeTempData::Cold => 1,
    }
}

const fn ocean_ruin_biome_temp_from_persistent(value: i8) -> OceanRuinBiomeTempData {
    match value {
        1 => OceanRuinBiomeTempData::Cold,
        _ => OceanRuinBiomeTempData::Warm,
    }
}

const fn placement_adjustment_to_persistent(
    adjustment: TemplatePlacementAdjustment,
) -> PersistentTemplatePlacementAdjustment {
    match adjustment {
        TemplatePlacementAdjustment::None => PersistentTemplatePlacementAdjustment::None,
        TemplatePlacementAdjustment::Shipwreck {
            is_beached,
            height_adjusted,
        } => PersistentTemplatePlacementAdjustment::Shipwreck {
            is_beached,
            height_adjusted,
        },
        TemplatePlacementAdjustment::Igloo { template_offset } => {
            PersistentTemplatePlacementAdjustment::Igloo {
                template_offset: [template_offset.0, template_offset.1, template_offset.2],
            }
        }
        TemplatePlacementAdjustment::OceanRuin => PersistentTemplatePlacementAdjustment::OceanRuin,
    }
}

const fn placement_adjustment_from_persistent(
    adjustment: &PersistentTemplatePlacementAdjustment,
) -> TemplatePlacementAdjustment {
    match adjustment {
        PersistentTemplatePlacementAdjustment::None => TemplatePlacementAdjustment::None,
        PersistentTemplatePlacementAdjustment::Shipwreck {
            is_beached,
            height_adjusted,
        } => TemplatePlacementAdjustment::Shipwreck {
            is_beached: *is_beached,
            height_adjusted: *height_adjusted,
        },
        PersistentTemplatePlacementAdjustment::Igloo { template_offset } => {
            TemplatePlacementAdjustment::Igloo {
                template_offset: (template_offset[0], template_offset[1], template_offset[2]),
            }
        }
        PersistentTemplatePlacementAdjustment::OceanRuin => TemplatePlacementAdjustment::OceanRuin,
    }
}

const fn placement_clip_to_persistent(placement_clip: TemplatePlacementClip) -> i8 {
    match placement_clip {
        TemplatePlacementClip::CenterChunk => 0,
        TemplatePlacementClip::CenterChunkExpandedToTemplate => 1,
        TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate => 2,
    }
}

const fn placement_clip_from_persistent(value: i8) -> TemplatePlacementClip {
    match value {
        1 => TemplatePlacementClip::CenterChunkExpandedToTemplate,
        2 => TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
        _ => TemplatePlacementClip::CenterChunk,
    }
}

const fn post_process_to_persistent(post_process: TemplatePostProcess) -> i8 {
    match post_process {
        TemplatePostProcess::None => 0,
        TemplatePostProcess::NetherFossil => 1,
        TemplatePostProcess::IglooTop => 2,
        TemplatePostProcess::RuinedPortal => 3,
    }
}

const fn post_process_from_persistent(value: i8) -> TemplatePostProcess {
    match value {
        1 => TemplatePostProcess::NetherFossil,
        2 => TemplatePostProcess::IglooTop,
        3 => TemplatePostProcess::RuinedPortal,
        _ => TemplatePostProcess::None,
    }
}

fn compare_identifiers(a: &Identifier, b: &Identifier) -> CmpOrdering {
    a.namespace
        .cmp(&b.namespace)
        .then_with(|| a.path.cmp(&b.path))
}

fn homogeneous_packed_light_value(data: &[u8; DATA_LAYER_SIZE]) -> Option<u8> {
    let first = data[0];
    let value = first & 0x0F;
    if first >> 4 != value {
        return None;
    }
    data.iter().all(|byte| *byte == first).then_some(value)
}

#[derive(Clone, Copy)]
enum EntityPersistenceMode {
    ChunkSave,
    DimensionTransition,
}

use super::ram_only::RamOnlyStorage;
use super::region_manager::RegionManager;
use super::{
    PersistentBiomeData, PersistentBlockEntity, PersistentBlockState, PersistentBoundingBox,
    PersistentChunk, PersistentDesertPyramidPieceData, PersistentEntity, PersistentHeightmap,
    PersistentJigsawJunction, PersistentJigsawPieceData, PersistentJungleTemplePieceData,
    PersistentLightData, PersistentLightSection, PersistentMineshaftPieceData,
    PersistentMineshaftPieceKind, PersistentNetherFortressPieceData,
    PersistentOceanMonumentChildPiece, PersistentOceanMonumentChildPieceKind,
    PersistentOceanMonumentPieceData, PersistentOceanMonumentRoomData, PersistentPoi,
    PersistentPoolElement, PersistentProceduralPieceData, PersistentProcessorList,
    PersistentSection, PersistentStrongholdPieceData, PersistentStrongholdSmallDoorType,
    PersistentStructurePiece, PersistentStructurePiecePayload, PersistentStructureReference,
    PersistentStructureStart, PersistentSwampHutPieceData, PersistentTemplatePieceData,
    PersistentTemplatePlacementAdjustment, PersistentTemplateProcessorList, PersistentTick,
    PreparedChunkSave,
};

/// Builder for creating a persistent chunk with its own palettes.
struct ChunkBuilder<'a> {
    block_states: Vec<PersistentBlockState<'static>>,
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

/// Runtime chunk data loaded from persistence.
pub struct LoadedChunk {
    /// The deserialized chunk.
    pub chunk: ChunkAccess,
    /// The highest persisted status for the chunk.
    pub status: ChunkStatus,
    /// Full-chunk entities waiting for lifecycle-approved world registration.
    pub pending_entities: Vec<SharedEntity>,
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
        thread_pool: &rayon::ThreadPool,
    ) -> io::Result<Option<LoadedChunk>> {
        match self {
            Self::Disk(rm) => rm.load_chunk(pos, min_y, height, level, thread_pool).await,
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
        thread_pool: &rayon::ThreadPool,
    ) -> io::Result<bool> {
        match self {
            Self::Disk(rm) => rm.save_chunk_data(prepared, status, thread_pool).await,
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
    /// If the chunk is not dirty and `force` is false, this is a no-op.
    /// Returns `Ok(true)` if the chunk was saved.
    /// Prepares chunk data for saving. Call this while holding the chunk lock,
    /// then pass the result to `save_chunk_data` after releasing the lock.
    #[must_use]
    #[expect(
        clippy::similar_names,
        reason = "`pois` vs `pos` are semantically distinct"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "chunk save preparation keeps related serialization setup in one pass"
    )]
    pub fn prepare_chunk_save(
        chunk: &ChunkAccess,
        runtime_entities: &[SharedEntity],
        force: bool,
    ) -> Option<PreparedChunkSave> {
        if !force && !chunk.is_dirty() {
            return None;
        }

        // Finalize any sections still in worldgen Building mode. Proto chunks
        // can be saved before being upgraded to `LevelChunk::from_proto`
        // (which is where `recalculate_counts` normally runs and implicitly
        // finalizes). Without this, `section_to_persistent` would panic on
        // the Building variant.
        for section_holder in &chunk.sections().sections {
            let mut guard = section_holder.write();
            if matches!(&guard.states, PalettedContainer::Building(_)) {
                guard.recalculate_counts();
            }
        }

        let pos = chunk.pos();

        let (block_entities, pending_block_entities) = chunk.block_entity_save_snapshot();

        let mut seen_entity_ids = FxHashSet::default();
        let mut seen_entity_uuids = FxHashSet::default();
        let mut entities = Vec::new();
        for entity in chunk.get_saveable_entities() {
            if !Self::entity_position_is_finite(entity.as_ref()) {
                Self::warn_skipping_non_finite_entity(entity.as_ref());
                continue;
            }
            if seen_entity_ids.insert(entity.id()) {
                Self::assert_unique_save_uuid(
                    &mut seen_entity_uuids,
                    entity.uuid(),
                    entity.id(),
                    pos,
                );
                entities.push(entity);
            }
        }
        let mut handled_runtime_entity_ids = Vec::new();
        for entity in runtime_entities {
            handled_runtime_entity_ids.push(entity.id());
            if !Self::entity_position_is_finite(entity.as_ref()) {
                Self::warn_skipping_non_finite_entity(entity.as_ref());
                continue;
            }
            if seen_entity_ids.insert(entity.id()) {
                Self::assert_unique_save_uuid(
                    &mut seen_entity_uuids,
                    entity.uuid(),
                    entity.id(),
                    pos,
                );
                entities.push(Arc::clone(entity));
            }
        }

        // Serialize scheduled ticks
        let (block_ticks, fluid_ticks) = match chunk {
            ChunkAccess::Full(c) => {
                let snapshot = c.scheduled_tick_snapshot();
                let bt = Self::block_ticks_to_persistent(snapshot.block, pos);
                let ft = Self::fluid_ticks_to_persistent(snapshot.fluid, pos);
                (bt, ft)
            }
            ChunkAccess::Proto(c) => {
                // Proto ticks are pending, so Vanilla ignores the current game
                // time when serializing their already-relative delays.
                let bt = Self::block_ticks_to_persistent(c.block_ticks.lock().pack(0), pos);
                let ft = Self::fluid_ticks_to_persistent(c.fluid_ticks.lock().pack(0), pos);
                (bt, ft)
            }
            ChunkAccess::Unloaded => unreachable!(),
        };

        // Serialize heightmaps
        let heightmaps = chunk
            .as_full()
            .map(|c| Self::heightmaps_to_persistent(&c.heightmaps.read()))
            .unwrap_or_default();

        let light = match chunk {
            ChunkAccess::Full(c) => Self::light_to_persistent(&c.light.read()),
            ChunkAccess::Proto(c) => Self::light_to_persistent(&c.light.read()),
            ChunkAccess::Unloaded => unreachable!(),
        };

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
            ChunkAccess::Full(full) => full.postprocessing_for_serialization(),
            ChunkAccess::Unloaded => unreachable!(),
        };

        let persistent = Self::to_persistent(
            chunk.sections(),
            &block_entities,
            &pending_block_entities,
            &entities,
            block_ticks,
            fluid_ticks,
            heightmaps,
            light,
            carving_mask,
            postprocessing,
            structure_starts,
            structure_references,
            pois,
            pos,
        );

        Some(PreparedChunkSave {
            pos,
            persistent,
            handled_runtime_entity_ids,
        })
    }

    fn entity_position_is_finite(entity: &dyn Entity) -> bool {
        let pos = entity.position();
        pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite()
    }

    fn warn_skipping_non_finite_entity(entity: &dyn Entity) {
        tracing::warn!(
            uuid = ?entity.uuid(),
            "Entity has non-finite position {:?}, skipping save",
            entity.position()
        );
    }

    fn assert_unique_save_uuid(
        seen_uuids: &mut FxHashSet<uuid::Uuid>,
        uuid: uuid::Uuid,
        entity_id: i32,
        chunk_pos: ChunkPos,
    ) {
        assert!(
            seen_uuids.insert(uuid),
            "duplicate saveable entity uuid {uuid} while preparing chunk {chunk_pos:?} for save; latest entity id {entity_id}"
        );
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
        pending_block_entities: &[BlockPos],
        entities: &[SharedEntity],
        block_ticks: Vec<PersistentTick>,
        fluid_ticks: Vec<PersistentTick>,
        heightmaps: Vec<PersistentHeightmap>,
        light: PersistentLightData,
        carving_mask: Option<Vec<u64>>,
        postprocessing: Vec<Vec<u16>>,
        structure_starts: Vec<PersistentStructureStart>,
        structure_references: Vec<PersistentStructureReference>,
        pois: Vec<PersistentPoi>,
        chunk_pos: ChunkPos,
    ) -> PersistentChunk<'static> {
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
                let pos = entity.get_block_pos();

                // Serialize NBT data
                let mut nbt = NbtCompound::new();
                entity.save_additional(&mut nbt);
                let mut nbt_bytes = Vec::new();
                nbt.write(&mut nbt_bytes);

                PersistentBlockEntity {
                    x: (pos.0.x - chunk_pos.0.x * 16) as u8,
                    y: pos.0.y as i16,
                    z: (pos.0.z - chunk_pos.0.y * 16) as u8,
                    entity_type: Some(entity.get_type().key.clone()),
                    nbt_data: nbt_bytes,
                }
            })
            .chain(
                pending_block_entities
                    .iter()
                    .map(|pos| PersistentBlockEntity {
                        x: (pos.0.x - chunk_pos.0.x * 16) as u8,
                        y: pos.0.y as i16,
                        z: (pos.0.z - chunk_pos.0.y * 16) as u8,
                        entity_type: None,
                        nbt_data: Vec::new(),
                    }),
            )
            .collect();

        let persistent_entities = Self::entities_to_persistent(entities);

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
            light,
            carving_mask,
            postprocessing,
            structure_starts,
            structure_references,
            pois,
        }
    }
}
