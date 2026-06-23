use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::heightmap::{ChunkHeightmaps, Heightmap, HeightmapType};
use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::paletted_container::PalettedContainer;
use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::section::{ChunkSection, SectionHolder, Sections};
use crate::chunk_saver::bit_pack::{bits_for_palette_len, pack_indices, unpack_indices};
use crate::entity::{
    ENTITIES, Entity, EntityBase, EntityBaseSaveData, EntityFireFreezeState, EntityLoadRequest,
    MAX_ENTITY_TAGS, RemovalReason, SharedEntity,
};
use crate::world::World;
use crate::world::tick_scheduler::{BlockTickList, FluidTickList, ScheduledTick, TickPriority};
use crate::worldgen::carving_mask::CarvingMask;
use glam::{DVec3, IVec3};
use rustc_hash::FxHashSet;
use simdnbt::ToNbtTag;
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
use steel_registry::{REGISTRY, Registry, RegistryEntry, RegistryExt, vanilla_biomes};
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

use super::ram_only::RamOnlyStorage;
use super::region_manager::RegionManager;
use super::{
    PersistentBiomeData, PersistentBlockEntity, PersistentBlockState, PersistentBoundingBox,
    PersistentChunk, PersistentDesertPyramidPieceData, PersistentEntity, PersistentHeightmap,
    PersistentJigsawJunction, PersistentJigsawPieceData, PersistentJungleTemplePieceData,
    PersistentMineshaftPieceData, PersistentMineshaftPieceKind, PersistentNetherFortressPieceData,
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
    ) -> io::Result<Option<LoadedChunk>> {
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

        let block_entities = chunk.get_block_entities();

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
                let bt = Self::block_ticks_to_persistent(&c.block_ticks.lock(), pos);
                let ft = Self::fluid_ticks_to_persistent(&c.fluid_ticks.lock(), pos);
                (bt, ft)
            }
            ChunkAccess::Proto(c) => {
                let bt = Self::block_ticks_to_persistent(&c.block_ticks.lock(), pos);
                let ft = Self::fluid_ticks_to_persistent(&c.fluid_ticks.lock(), pos);
                (bt, ft)
            }
            ChunkAccess::Unloaded => unreachable!(),
        };

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
            carving_mask,
            postprocessing,
            structure_starts,
            structure_references,
            pois,
        }
    }

    fn entities_to_persistent(entities: &[SharedEntity]) -> Vec<PersistentEntity> {
        let mut visited = FxHashSet::default();
        entities
            .iter()
            .filter(|entity| !entity.is_passenger())
            .filter_map(|entity| Self::entity_to_persistent(entity, &mut visited))
            .collect()
    }

    pub(crate) fn entity_tree_to_persistent(entity: &SharedEntity) -> Option<PersistentEntity> {
        let mut visited = FxHashSet::default();
        Self::entity_to_persistent(entity, &mut visited)
    }

    fn custom_name_to_persistent(custom_name: Option<&TextComponent>) -> Vec<u8> {
        let Some(custom_name) = custom_name else {
            return Vec::new();
        };

        let mut root = NbtCompound::new();
        root.insert("CustomName", custom_name.to_nbt_tag());
        let mut bytes = Vec::new();
        root.write(&mut bytes);
        bytes
    }

    fn custom_name_from_persistent(bytes: &[u8], uuid: uuid::Uuid) -> Option<TextComponent> {
        if bytes.is_empty() {
            return None;
        }

        let Ok(root) = read_borrowed_compound(&mut Cursor::new(bytes)) else {
            tracing::warn!(
                ?uuid,
                "Failed to parse entity custom name NBT, defaulting to no custom name"
            );
            return None;
        };
        let root = simdnbt::borrow::NbtCompound::from(&root);
        let tag = root.get("CustomName")?;
        let custom_name = TextComponent::from_nbt(&tag.to_owned());
        if custom_name.is_none() {
            tracing::warn!(
                ?uuid,
                "Failed to decode entity custom name, defaulting to no custom name"
            );
            return None;
        }
        custom_name
    }

    fn compound_to_persistent(compound: &NbtCompound) -> Vec<u8> {
        if compound.is_empty() {
            return Vec::new();
        }

        let mut bytes = Vec::new();
        compound.write(&mut bytes);
        bytes
    }

    fn compound_from_persistent(bytes: &[u8], uuid: uuid::Uuid) -> NbtCompound {
        if bytes.is_empty() {
            return NbtCompound::new();
        }

        let Ok(compound) = read_borrowed_compound(&mut Cursor::new(bytes)) else {
            tracing::warn!(
                ?uuid,
                "Failed to parse entity custom data NBT, defaulting to empty custom data"
            );
            return NbtCompound::new();
        };
        simdnbt::borrow::NbtCompound::from(&compound).to_owned()
    }

    fn save_data_from_persistent(
        persistent: &PersistentEntity,
        uuid: uuid::Uuid,
    ) -> EntityBaseSaveData {
        EntityBaseSaveData {
            air_supply: persistent.air_supply,
            portal_cooldown: persistent.portal_cooldown,
            no_gravity: persistent.no_gravity,
            invulnerable: persistent.invulnerable,
            custom_name: Self::custom_name_from_persistent(&persistent.custom_name_nbt, uuid),
            custom_name_visible: persistent.custom_name_visible,
            silent: persistent.silent,
            glowing: persistent.glowing,
            tags: persistent
                .tags
                .iter()
                .take(MAX_ENTITY_TAGS)
                .cloned()
                .collect(),
            custom_data: Self::compound_from_persistent(&persistent.custom_data_nbt, uuid),
        }
    }

    fn clamp_loaded_entity_position(pos: DVec3) -> DVec3 {
        DVec3::new(
            pos.x.clamp(
                -ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
                ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
            ),
            pos.y.clamp(
                -ENTITY_LOAD_MAX_VERTICAL_POSITION,
                ENTITY_LOAD_MAX_VERTICAL_POSITION,
            ),
            pos.z.clamp(
                -ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
                ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
            ),
        )
    }

    fn entity_to_persistent(
        entity: &SharedEntity,
        visited: &mut FxHashSet<i32>,
    ) -> Option<PersistentEntity> {
        if !Self::entity_should_save(entity.as_ref()) {
            return None;
        }

        if !visited.insert(entity.id()) {
            tracing::warn!(
                uuid = ?entity.uuid(),
                "Entity passenger tree contains duplicate entity id {}, skipping duplicate save",
                entity.id()
            );
            return None;
        }

        let pos = entity.position();
        let stored_pos = if let Some(vehicle) = entity.vehicle() {
            let vehicle_pos = vehicle.position();
            DVec3::new(vehicle_pos.x, pos.y, vehicle_pos.z)
        } else {
            pos
        };
        let vel = entity.velocity();
        let (yaw, pitch) = entity.rotation();
        let fire_freeze = entity.fire_freeze_state();
        let save_data = entity.base().save_data();

        if !stored_pos.x.is_finite() || !stored_pos.y.is_finite() || !stored_pos.z.is_finite() {
            tracing::warn!(
                uuid = ?entity.uuid(),
                "Entity has non-finite position {:?}, skipping save",
                stored_pos
            );
            return None;
        }

        let mut nbt = NbtCompound::new();
        entity.save_additional(&mut nbt);
        let mut nbt_bytes = Vec::new();
        nbt.write(&mut nbt_bytes);

        let passengers = entity
            .passengers()
            .iter()
            .filter_map(|passenger| Self::entity_to_persistent(passenger, visited))
            .collect();

        Some(PersistentEntity {
            entity_type: entity.entity_type().key.clone(),
            uuid: *entity.uuid().as_bytes(),
            pos: [stored_pos.x, stored_pos.y, stored_pos.z],
            motion: [vel.x, vel.y, vel.z],
            rotation: [yaw, pitch],
            fall_distance: entity.fall_distance(),
            remaining_fire_ticks: fire_freeze.remaining_fire_ticks(),
            ticks_frozen: fire_freeze.ticks_frozen(),
            is_in_powder_snow: fire_freeze.is_in_powder_snow(),
            was_in_powder_snow: fire_freeze.was_in_powder_snow(),
            has_visual_fire: fire_freeze.has_visual_fire(),
            on_ground: entity.on_ground(),
            no_gravity: save_data.no_gravity,
            invulnerable: save_data.invulnerable,
            air_supply: save_data.air_supply,
            portal_cooldown: save_data.portal_cooldown,
            custom_name_nbt: Self::custom_name_to_persistent(save_data.custom_name.as_ref()),
            custom_name_visible: save_data.custom_name_visible,
            silent: save_data.silent,
            glowing: save_data.glowing,
            tags: save_data.tags.iter().cloned().collect(),
            custom_data_nbt: Self::compound_to_persistent(&save_data.custom_data),
            nbt_data: nbt_bytes,
            passengers,
        })
    }

    fn entity_should_save(entity: &dyn Entity) -> bool {
        (!entity.is_removed()
            || entity
                .removal_reason()
                .is_some_and(RemovalReason::should_save))
            && entity.entity_type().can_serialize
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
            PalettedContainer::Building(_) => panic!(
                "section_to_persistent called on a section still in worldgen Building mode; \
                 finalize_building must be called before serialization"
            ),
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
            PalettedContainer::Building(_) => panic!(
                "biomes_to_persistent called on a section still in worldgen Building mode; \
                 finalize_building must be called before serialization"
            ),
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
    #[expect(
        clippy::too_many_lines,
        reason = "chunk persistence conversion is a linear field-by-field transform"
    )]
    pub(crate) fn persistent_to_chunk(
        persistent: &PersistentChunk,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> LoadedChunk {
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

            let mut pending_entities = Vec::with_capacity(persistent.entities.len());
            let level_weak = chunk.level_weak();
            for persistent_entity in &persistent.entities {
                let mut loaded_entities =
                    Self::persistent_to_entity_tree_at_level(persistent_entity, pos, &level_weak);
                pending_entities.append(&mut loaded_entities);
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

            LoadedChunk {
                chunk: ChunkAccess::Full(chunk),
                status,
                pending_entities,
            }
        } else {
            let block_ticks = Self::persistent_to_block_ticks(&persistent.block_ticks, pos);
            let fluid_ticks = Self::persistent_to_fluid_ticks(&persistent.fluid_ticks, pos);
            let carving_mask = persistent
                .carving_mask
                .as_deref()
                .map(|packed| CarvingMask::from_packed_u64s(height, min_y, packed));

            let chunk = ProtoChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                status,
                min_y,
                height,
                structure_starts,
                structure_references,
                carving_mask,
                persistent.postprocessing.iter().map(Vec::clone).collect(),
                block_ticks,
                fluid_ticks,
                level.clone(),
            );

            for persistent_be in &persistent.block_entities {
                let block_entity_pos = Self::persistent_block_entity_pos(persistent_be, pos);
                let state = chunk.get_block_state(block_entity_pos);
                if let Some(block_entity) = Self::persistent_to_block_entity_at(
                    persistent_be,
                    block_entity_pos,
                    level.clone(),
                    state,
                ) {
                    chunk.add_and_register_block_entity(block_entity);
                }
            }

            for persistent_entity in &persistent.entities {
                let loaded_entities =
                    Self::persistent_to_entity_tree_at_level(persistent_entity, pos, &level);
                for entity in loaded_entities {
                    chunk.add_entity(entity);
                }
            }

            chunk.dirty.store(false, Ordering::Release);

            LoadedChunk {
                chunk: ChunkAccess::Proto(chunk),
                status,
                pending_entities: Vec::new(),
            }
        }
    }

    fn persistent_block_entity_pos(
        persistent: &PersistentBlockEntity,
        chunk_pos: ChunkPos,
    ) -> BlockPos {
        let abs_x = chunk_pos.0.x * 16 + i32::from(persistent.x);
        let abs_z = chunk_pos.0.y * 16 + i32::from(persistent.z);
        BlockPos::new(abs_x, i32::from(persistent.y), abs_z)
    }

    /// Converts a persistent block entity to runtime format.
    fn persistent_to_block_entity(
        persistent: &PersistentBlockEntity,
        chunk_pos: ChunkPos,
        chunk: &LevelChunk,
    ) -> Option<SharedBlockEntity> {
        let pos = Self::persistent_block_entity_pos(persistent, chunk_pos);
        let state = chunk.get_block_state(pos);
        Self::persistent_to_block_entity_at(persistent, pos, chunk.level_weak(), state)
    }

    fn persistent_to_block_entity_at(
        persistent: &PersistentBlockEntity,
        pos: BlockPos,
        level: Weak<World>,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        // Look up the block entity type
        let block_entity_type = REGISTRY
            .block_entity_types
            .by_key(&persistent.entity_type)?;

        // Parse and load NBT data
        if persistent.nbt_data.is_empty() {
            // No NBT data, just create the entity without loading
            Some(BLOCK_ENTITIES.create_or_raw(block_entity_type, level, pos, state))
        } else {
            // Parse NBT from bytes as borrowed
            let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(&persistent.nbt_data)) else {
                return Some(BLOCK_ENTITIES.create_or_raw(block_entity_type, level, pos, state));
            };

            // Create the block entity and load NBT
            Some(BLOCK_ENTITIES.create_and_load_or_raw(block_entity_type, level, pos, state, &nbt))
        }
    }

    /// Converts a persistent entity tree to runtime format.
    pub(crate) fn persistent_to_entity_tree_at_level(
        persistent: &PersistentEntity,
        chunk_pos: ChunkPos,
        level: &Weak<World>,
    ) -> Vec<SharedEntity> {
        let mut entities = Vec::new();
        let Some(entity) = Self::persistent_to_entity_at_level(persistent, chunk_pos, level) else {
            return entities;
        };

        entities.push(Arc::clone(&entity));
        for persistent_passenger in &persistent.passengers {
            Self::load_persistent_passenger_tree(
                persistent_passenger,
                chunk_pos,
                level,
                &entity,
                &mut entities,
            );
        }
        entities
    }

    fn load_persistent_passenger_tree(
        persistent: &PersistentEntity,
        chunk_pos: ChunkPos,
        level: &Weak<World>,
        vehicle: &SharedEntity,
        entities: &mut Vec<SharedEntity>,
    ) {
        let Some(passenger) = Self::persistent_to_entity_at_level(persistent, chunk_pos, level)
        else {
            return;
        };

        EntityBase::restore_passenger_relationship(vehicle, &passenger);
        entities.push(Arc::clone(&passenger));
        for persistent_passenger in &persistent.passengers {
            Self::load_persistent_passenger_tree(
                persistent_passenger,
                chunk_pos,
                level,
                &passenger,
                entities,
            );
        }
    }

    /// Converts one persistent entity to runtime format without loading passengers.
    fn persistent_to_entity_at_level(
        persistent: &PersistentEntity,
        chunk_pos: ChunkPos,
        level: &Weak<World>,
    ) -> Option<SharedEntity> {
        use uuid::Uuid;

        // Reconstruct base fields
        let stored_pos = DVec3::new(persistent.pos[0], persistent.pos[1], persistent.pos[2]);
        let mut velocity = DVec3::new(
            persistent.motion[0],
            persistent.motion[1],
            persistent.motion[2],
        );
        let rotation = (persistent.rotation[0], persistent.rotation[1]);
        let uuid = Uuid::from_bytes(persistent.uuid);

        // Validate position is finite
        if !stored_pos.x.is_finite() || !stored_pos.y.is_finite() || !stored_pos.z.is_finite() {
            tracing::warn!(
                ?uuid,
                "Entity has non-finite position {:?}, skipping load",
                stored_pos
            );
            return None;
        }

        if !rotation.0.is_finite() || !rotation.1.is_finite() {
            tracing::warn!(
                ?uuid,
                "Entity has non-finite rotation {rotation:?}, skipping load"
            );
            return None;
        }

        let pos = Self::clamp_loaded_entity_position(stored_pos);

        // Validate position is within expected chunk (sanity check)
        let expected_chunk = ChunkPos::from_entity_pos(pos);
        if chunk_pos != expected_chunk {
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
        let save_data = Self::save_data_from_persistent(persistent, uuid);

        // Parse NBT from bytes (or use empty compound data)
        let nbt_bytes = if persistent.nbt_data.is_empty() {
            // Empty compound body for `simdnbt::borrow::read_compound`.
            &[0x00][..]
        } else {
            &persistent.nbt_data[..]
        };

        let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(nbt_bytes)) else {
            tracing::warn!(?uuid, "Failed to parse entity NBT, skipping");
            return None;
        };

        Some(ENTITIES.create_and_load_or_raw(
            EntityLoadRequest {
                entity_type,
                position: pos,
                uuid,
                velocity,
                rotation,
                fall_distance: persistent.fall_distance,
                fire_freeze: EntityFireFreezeState::from_parts(
                    persistent.remaining_fire_ticks,
                    persistent.ticks_frozen,
                    persistent.is_in_powder_snow,
                    persistent.was_in_powder_snow,
                    persistent.has_visual_fire,
                ),
                on_ground: persistent.on_ground,
                save_data,
                world: Weak::clone(level),
            },
            &nbt,
        ))
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
            position: [data.position.x, data.position.y, data.position.z],
            rotation: rotation_to_persistent(data.rotation),
            liquid_settings: liquid_settings_to_persistent(data.liquid_settings),
        }
    }

    fn persistent_to_jigsaw_piece_data(data: &PersistentJigsawPieceData) -> JigsawPieceData {
        JigsawPieceData {
            pool_element: Self::persistent_to_pool_element(&data.pool_element),
            position: IVec3::new(data.position[0], data.position[1], data.position[2]),
            rotation: rotation_from_persistent(data.rotation),
            liquid_settings: liquid_settings_from_persistent(data.liquid_settings),
        }
    }

    fn procedural_piece_data_to_persistent(
        data: &ProceduralPieceData,
    ) -> PersistentProceduralPieceData {
        match data {
            ProceduralPieceData::Unimplemented => PersistentProceduralPieceData::Unimplemented,
            ProceduralPieceData::BuriedTreasure => PersistentProceduralPieceData::BuriedTreasure,
            ProceduralPieceData::DesertPyramid(data) => {
                PersistentProceduralPieceData::DesertPyramid(PersistentDesertPyramidPieceData {
                    height_position: data.height_position.unwrap_or(-1),
                    has_placed_chest: data.has_placed_chest,
                })
            }
            ProceduralPieceData::JungleTemple(data) => {
                PersistentProceduralPieceData::JungleTemple(PersistentJungleTemplePieceData {
                    height_position: data.height_position.unwrap_or(-1),
                    placed_main_chest: data.placed_main_chest,
                    placed_hidden_chest: data.placed_hidden_chest,
                    placed_trap1: data.placed_trap1,
                    placed_trap2: data.placed_trap2,
                })
            }
            ProceduralPieceData::Mineshaft(data) => {
                PersistentProceduralPieceData::Mineshaft(PersistentMineshaftPieceData {
                    mineshaft_type: mineshaft_type_to_persistent(data.mineshaft_type),
                    kind: Self::mineshaft_kind_to_persistent(&data.kind),
                })
            }
            ProceduralPieceData::NetherFortress(data) => {
                PersistentProceduralPieceData::NetherFortress(
                    Self::fortress_piece_data_to_persistent(*data),
                )
            }
            ProceduralPieceData::OceanMonument(data) => {
                PersistentProceduralPieceData::OceanMonument(
                    Self::ocean_monument_data_to_persistent(data),
                )
            }
            ProceduralPieceData::Stronghold(data) => PersistentProceduralPieceData::Stronghold(
                Self::stronghold_piece_data_to_persistent(*data),
            ),
            ProceduralPieceData::SwampHut(data) => {
                PersistentProceduralPieceData::SwampHut(PersistentSwampHutPieceData {
                    height_position: data.height_position.unwrap_or(-1),
                    spawned_witch: data.spawned_witch,
                    spawned_cat: data.spawned_cat,
                })
            }
        }
    }

    fn persistent_to_procedural_piece_data(
        data: &PersistentProceduralPieceData,
    ) -> ProceduralPieceData {
        match data {
            PersistentProceduralPieceData::Unimplemented => ProceduralPieceData::Unimplemented,
            PersistentProceduralPieceData::BuriedTreasure => ProceduralPieceData::BuriedTreasure,
            PersistentProceduralPieceData::DesertPyramid(data) => {
                ProceduralPieceData::DesertPyramid(DesertPyramidPieceData {
                    height_position: (data.height_position >= 0).then_some(data.height_position),
                    has_placed_chest: data.has_placed_chest,
                    potential_suspicious_sand_world_positions: Vec::new(),
                    random_collapsed_roof_pos: BlockPos::new(0, 0, 0),
                })
            }
            PersistentProceduralPieceData::JungleTemple(data) => {
                ProceduralPieceData::JungleTemple(JungleTemplePieceData {
                    height_position: (data.height_position >= 0).then_some(data.height_position),
                    placed_main_chest: data.placed_main_chest,
                    placed_hidden_chest: data.placed_hidden_chest,
                    placed_trap1: data.placed_trap1,
                    placed_trap2: data.placed_trap2,
                })
            }
            PersistentProceduralPieceData::Mineshaft(data) => {
                ProceduralPieceData::Mineshaft(MineshaftPiecePayload {
                    mineshaft_type: mineshaft_type_from_persistent(data.mineshaft_type),
                    kind: Self::persistent_to_mineshaft_kind(&data.kind),
                })
            }
            PersistentProceduralPieceData::NetherFortress(data) => {
                ProceduralPieceData::NetherFortress(Self::persistent_to_fortress_piece_data(data))
            }
            PersistentProceduralPieceData::OceanMonument(data) => {
                ProceduralPieceData::OceanMonument(Self::persistent_to_ocean_monument_data(data))
            }
            PersistentProceduralPieceData::Stronghold(data) => {
                ProceduralPieceData::Stronghold(Self::persistent_to_stronghold_piece_data(data))
            }
            PersistentProceduralPieceData::SwampHut(data) => {
                ProceduralPieceData::SwampHut(SwampHutPieceData {
                    height_position: (data.height_position >= 0).then_some(data.height_position),
                    spawned_witch: data.spawned_witch,
                    spawned_cat: data.spawned_cat,
                })
            }
        }
    }

    fn ocean_monument_data_to_persistent(
        data: &OceanMonumentPieceData,
    ) -> PersistentOceanMonumentPieceData {
        PersistentOceanMonumentPieceData {
            child_pieces: data
                .child_pieces
                .iter()
                .map(Self::ocean_monument_child_to_persistent)
                .collect(),
        }
    }

    fn persistent_to_ocean_monument_data(
        data: &PersistentOceanMonumentPieceData,
    ) -> OceanMonumentPieceData {
        OceanMonumentPieceData {
            child_pieces: data
                .child_pieces
                .iter()
                .map(Self::persistent_to_ocean_monument_child)
                .collect(),
        }
    }

    const fn ocean_monument_child_to_persistent(
        child: &OceanMonumentChildPiece,
    ) -> PersistentOceanMonumentChildPiece {
        PersistentOceanMonumentChildPiece {
            bounding_box: PersistentBoundingBox::from_bounding_box(child.bounding_box),
            kind: Self::ocean_monument_child_kind_to_persistent(&child.kind),
        }
    }

    const fn persistent_to_ocean_monument_child(
        child: &PersistentOceanMonumentChildPiece,
    ) -> OceanMonumentChildPiece {
        OceanMonumentChildPiece {
            bounding_box: child.bounding_box.to_bounding_box(),
            kind: Self::persistent_to_ocean_monument_child_kind(&child.kind),
        }
    }

    const fn ocean_monument_child_kind_to_persistent(
        kind: &OceanMonumentChildPieceKind,
    ) -> PersistentOceanMonumentChildPieceKind {
        match kind {
            OceanMonumentChildPieceKind::EntryRoom { room } => {
                PersistentOceanMonumentChildPieceKind::EntryRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                }
            }
            OceanMonumentChildPieceKind::CoreRoom => {
                PersistentOceanMonumentChildPieceKind::CoreRoom
            }
            OceanMonumentChildPieceKind::DoubleXRoom { west, east } => {
                PersistentOceanMonumentChildPieceKind::DoubleXRoom {
                    west: Self::ocean_monument_room_to_persistent(*west),
                    east: Self::ocean_monument_room_to_persistent(*east),
                }
            }
            OceanMonumentChildPieceKind::DoubleXYRoom {
                west,
                east,
                west_up,
                east_up,
            } => PersistentOceanMonumentChildPieceKind::DoubleXYRoom {
                west: Self::ocean_monument_room_to_persistent(*west),
                east: Self::ocean_monument_room_to_persistent(*east),
                west_up: Self::ocean_monument_room_to_persistent(*west_up),
                east_up: Self::ocean_monument_room_to_persistent(*east_up),
            },
            OceanMonumentChildPieceKind::DoubleYRoom { room, above } => {
                PersistentOceanMonumentChildPieceKind::DoubleYRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                    above: Self::ocean_monument_room_to_persistent(*above),
                }
            }
            OceanMonumentChildPieceKind::DoubleYZRoom {
                south,
                north,
                south_up,
                north_up,
            } => PersistentOceanMonumentChildPieceKind::DoubleYZRoom {
                south: Self::ocean_monument_room_to_persistent(*south),
                north: Self::ocean_monument_room_to_persistent(*north),
                south_up: Self::ocean_monument_room_to_persistent(*south_up),
                north_up: Self::ocean_monument_room_to_persistent(*north_up),
            },
            OceanMonumentChildPieceKind::DoubleZRoom { south, north } => {
                PersistentOceanMonumentChildPieceKind::DoubleZRoom {
                    south: Self::ocean_monument_room_to_persistent(*south),
                    north: Self::ocean_monument_room_to_persistent(*north),
                }
            }
            OceanMonumentChildPieceKind::SimpleRoom { room, main_design } => {
                PersistentOceanMonumentChildPieceKind::SimpleRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                    main_design: *main_design,
                }
            }
            OceanMonumentChildPieceKind::SimpleTopRoom { room } => {
                PersistentOceanMonumentChildPieceKind::SimpleTopRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                }
            }
            OceanMonumentChildPieceKind::WingRoom { main_design } => {
                PersistentOceanMonumentChildPieceKind::WingRoom {
                    main_design: *main_design,
                }
            }
            OceanMonumentChildPieceKind::Penthouse => {
                PersistentOceanMonumentChildPieceKind::Penthouse
            }
        }
    }

    const fn persistent_to_ocean_monument_child_kind(
        kind: &PersistentOceanMonumentChildPieceKind,
    ) -> OceanMonumentChildPieceKind {
        match kind {
            PersistentOceanMonumentChildPieceKind::EntryRoom { room } => {
                OceanMonumentChildPieceKind::EntryRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                }
            }
            PersistentOceanMonumentChildPieceKind::CoreRoom => {
                OceanMonumentChildPieceKind::CoreRoom
            }
            PersistentOceanMonumentChildPieceKind::DoubleXRoom { west, east } => {
                OceanMonumentChildPieceKind::DoubleXRoom {
                    west: Self::persistent_to_ocean_monument_room(west),
                    east: Self::persistent_to_ocean_monument_room(east),
                }
            }
            PersistentOceanMonumentChildPieceKind::DoubleXYRoom {
                west,
                east,
                west_up,
                east_up,
            } => OceanMonumentChildPieceKind::DoubleXYRoom {
                west: Self::persistent_to_ocean_monument_room(west),
                east: Self::persistent_to_ocean_monument_room(east),
                west_up: Self::persistent_to_ocean_monument_room(west_up),
                east_up: Self::persistent_to_ocean_monument_room(east_up),
            },
            PersistentOceanMonumentChildPieceKind::DoubleYRoom { room, above } => {
                OceanMonumentChildPieceKind::DoubleYRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                    above: Self::persistent_to_ocean_monument_room(above),
                }
            }
            PersistentOceanMonumentChildPieceKind::DoubleYZRoom {
                south,
                north,
                south_up,
                north_up,
            } => OceanMonumentChildPieceKind::DoubleYZRoom {
                south: Self::persistent_to_ocean_monument_room(south),
                north: Self::persistent_to_ocean_monument_room(north),
                south_up: Self::persistent_to_ocean_monument_room(south_up),
                north_up: Self::persistent_to_ocean_monument_room(north_up),
            },
            PersistentOceanMonumentChildPieceKind::DoubleZRoom { south, north } => {
                OceanMonumentChildPieceKind::DoubleZRoom {
                    south: Self::persistent_to_ocean_monument_room(south),
                    north: Self::persistent_to_ocean_monument_room(north),
                }
            }
            PersistentOceanMonumentChildPieceKind::SimpleRoom { room, main_design } => {
                OceanMonumentChildPieceKind::SimpleRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                    main_design: *main_design,
                }
            }
            PersistentOceanMonumentChildPieceKind::SimpleTopRoom { room } => {
                OceanMonumentChildPieceKind::SimpleTopRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                }
            }
            PersistentOceanMonumentChildPieceKind::WingRoom { main_design } => {
                OceanMonumentChildPieceKind::WingRoom {
                    main_design: *main_design,
                }
            }
            PersistentOceanMonumentChildPieceKind::Penthouse => {
                OceanMonumentChildPieceKind::Penthouse
            }
        }
    }

    const fn ocean_monument_room_to_persistent(
        room: OceanMonumentRoomData,
    ) -> PersistentOceanMonumentRoomData {
        PersistentOceanMonumentRoomData {
            index: room.index,
            has_opening: room.has_opening,
            has_up_connection: room.has_up_connection,
        }
    }

    const fn persistent_to_ocean_monument_room(
        room: &PersistentOceanMonumentRoomData,
    ) -> OceanMonumentRoomData {
        OceanMonumentRoomData {
            index: room.index,
            has_opening: room.has_opening,
            has_up_connection: room.has_up_connection,
        }
    }

    const fn fortress_piece_data_to_persistent(
        data: FortressPieceData,
    ) -> PersistentNetherFortressPieceData {
        match data {
            FortressPieceData::BridgeCrossing => PersistentNetherFortressPieceData::BridgeCrossing,
            FortressPieceData::BridgeEndFiller { self_seed } => {
                PersistentNetherFortressPieceData::BridgeEndFiller { self_seed }
            }
            FortressPieceData::BridgeStraight => PersistentNetherFortressPieceData::BridgeStraight,
            FortressPieceData::CastleCorridorStairs => {
                PersistentNetherFortressPieceData::CastleCorridorStairs
            }
            FortressPieceData::CastleCorridorTBalcony => {
                PersistentNetherFortressPieceData::CastleCorridorTBalcony
            }
            FortressPieceData::CastleEntrance => PersistentNetherFortressPieceData::CastleEntrance,
            FortressPieceData::CastleSmallCorridorCrossing => {
                PersistentNetherFortressPieceData::CastleSmallCorridorCrossing
            }
            FortressPieceData::CastleSmallCorridorLeftTurn { is_needing_chest } => {
                PersistentNetherFortressPieceData::CastleSmallCorridorLeftTurn { is_needing_chest }
            }
            FortressPieceData::CastleSmallCorridor => {
                PersistentNetherFortressPieceData::CastleSmallCorridor
            }
            FortressPieceData::CastleSmallCorridorRightTurn { is_needing_chest } => {
                PersistentNetherFortressPieceData::CastleSmallCorridorRightTurn { is_needing_chest }
            }
            FortressPieceData::CastleStalkRoom => {
                PersistentNetherFortressPieceData::CastleStalkRoom
            }
            FortressPieceData::MonsterThrone { has_placed_spawner } => {
                PersistentNetherFortressPieceData::MonsterThrone { has_placed_spawner }
            }
            FortressPieceData::RoomCrossing => PersistentNetherFortressPieceData::RoomCrossing,
            FortressPieceData::StairsRoom => PersistentNetherFortressPieceData::StairsRoom,
        }
    }

    const fn persistent_to_fortress_piece_data(
        data: &PersistentNetherFortressPieceData,
    ) -> FortressPieceData {
        match data {
            PersistentNetherFortressPieceData::BridgeCrossing => FortressPieceData::BridgeCrossing,
            PersistentNetherFortressPieceData::BridgeEndFiller { self_seed } => {
                FortressPieceData::BridgeEndFiller {
                    self_seed: *self_seed,
                }
            }
            PersistentNetherFortressPieceData::BridgeStraight => FortressPieceData::BridgeStraight,
            PersistentNetherFortressPieceData::CastleCorridorStairs => {
                FortressPieceData::CastleCorridorStairs
            }
            PersistentNetherFortressPieceData::CastleCorridorTBalcony => {
                FortressPieceData::CastleCorridorTBalcony
            }
            PersistentNetherFortressPieceData::CastleEntrance => FortressPieceData::CastleEntrance,
            PersistentNetherFortressPieceData::CastleSmallCorridorCrossing => {
                FortressPieceData::CastleSmallCorridorCrossing
            }
            PersistentNetherFortressPieceData::CastleSmallCorridorLeftTurn { is_needing_chest } => {
                FortressPieceData::CastleSmallCorridorLeftTurn {
                    is_needing_chest: *is_needing_chest,
                }
            }
            PersistentNetherFortressPieceData::CastleSmallCorridor => {
                FortressPieceData::CastleSmallCorridor
            }
            PersistentNetherFortressPieceData::CastleSmallCorridorRightTurn {
                is_needing_chest,
            } => FortressPieceData::CastleSmallCorridorRightTurn {
                is_needing_chest: *is_needing_chest,
            },
            PersistentNetherFortressPieceData::CastleStalkRoom => {
                FortressPieceData::CastleStalkRoom
            }
            PersistentNetherFortressPieceData::MonsterThrone { has_placed_spawner } => {
                FortressPieceData::MonsterThrone {
                    has_placed_spawner: *has_placed_spawner,
                }
            }
            PersistentNetherFortressPieceData::RoomCrossing => FortressPieceData::RoomCrossing,
            PersistentNetherFortressPieceData::StairsRoom => FortressPieceData::StairsRoom,
        }
    }

    const fn stronghold_door_to_persistent(
        door: StrongholdSmallDoorType,
    ) -> PersistentStrongholdSmallDoorType {
        match door {
            StrongholdSmallDoorType::Opening => PersistentStrongholdSmallDoorType::Opening,
            StrongholdSmallDoorType::WoodDoor => PersistentStrongholdSmallDoorType::WoodDoor,
            StrongholdSmallDoorType::Grates => PersistentStrongholdSmallDoorType::Grates,
            StrongholdSmallDoorType::IronDoor => PersistentStrongholdSmallDoorType::IronDoor,
        }
    }

    const fn persistent_to_stronghold_door(
        door: &PersistentStrongholdSmallDoorType,
    ) -> StrongholdSmallDoorType {
        match door {
            PersistentStrongholdSmallDoorType::Opening => StrongholdSmallDoorType::Opening,
            PersistentStrongholdSmallDoorType::WoodDoor => StrongholdSmallDoorType::WoodDoor,
            PersistentStrongholdSmallDoorType::Grates => StrongholdSmallDoorType::Grates,
            PersistentStrongholdSmallDoorType::IronDoor => StrongholdSmallDoorType::IronDoor,
        }
    }

    const fn stronghold_piece_data_to_persistent(
        data: StrongholdPieceData,
    ) -> PersistentStrongholdPieceData {
        match data {
            StrongholdPieceData::Straight {
                entry_door,
                left_child,
                right_child,
            } => PersistentStrongholdPieceData::Straight {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                left_child,
                right_child,
            },
            StrongholdPieceData::PrisonHall { entry_door } => {
                PersistentStrongholdPieceData::PrisonHall {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::LeftTurn { entry_door } => {
                PersistentStrongholdPieceData::LeftTurn {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::RightTurn { entry_door } => {
                PersistentStrongholdPieceData::RightTurn {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::RoomCrossing {
                entry_door,
                crossing_type,
            } => PersistentStrongholdPieceData::RoomCrossing {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                crossing_type,
            },
            StrongholdPieceData::StraightStairsDown { entry_door } => {
                PersistentStrongholdPieceData::StraightStairsDown {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::StairsDown {
                entry_door,
                is_source,
            } => PersistentStrongholdPieceData::StairsDown {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                is_source,
            },
            StrongholdPieceData::FiveCrossing {
                entry_door,
                left_low,
                left_high,
                right_low,
                right_high,
            } => PersistentStrongholdPieceData::FiveCrossing {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                left_low,
                left_high,
                right_low,
                right_high,
            },
            StrongholdPieceData::ChestCorridor {
                entry_door,
                has_placed_chest,
            } => PersistentStrongholdPieceData::ChestCorridor {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                has_placed_chest,
            },
            StrongholdPieceData::Library {
                entry_door,
                is_tall,
            } => PersistentStrongholdPieceData::Library {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                is_tall,
            },
            StrongholdPieceData::PortalRoom { has_placed_spawner } => {
                PersistentStrongholdPieceData::PortalRoom { has_placed_spawner }
            }
            StrongholdPieceData::FillerCorridor { steps } => {
                PersistentStrongholdPieceData::FillerCorridor { steps }
            }
        }
    }

    const fn persistent_to_stronghold_piece_data(
        data: &PersistentStrongholdPieceData,
    ) -> StrongholdPieceData {
        match data {
            PersistentStrongholdPieceData::Straight {
                entry_door,
                left_child,
                right_child,
            } => StrongholdPieceData::Straight {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                left_child: *left_child,
                right_child: *right_child,
            },
            PersistentStrongholdPieceData::PrisonHall { entry_door } => {
                StrongholdPieceData::PrisonHall {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::LeftTurn { entry_door } => {
                StrongholdPieceData::LeftTurn {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::RightTurn { entry_door } => {
                StrongholdPieceData::RightTurn {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::RoomCrossing {
                entry_door,
                crossing_type,
            } => StrongholdPieceData::RoomCrossing {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                crossing_type: *crossing_type,
            },
            PersistentStrongholdPieceData::StraightStairsDown { entry_door } => {
                StrongholdPieceData::StraightStairsDown {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::StairsDown {
                entry_door,
                is_source,
            } => StrongholdPieceData::StairsDown {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                is_source: *is_source,
            },
            PersistentStrongholdPieceData::FiveCrossing {
                entry_door,
                left_low,
                left_high,
                right_low,
                right_high,
            } => StrongholdPieceData::FiveCrossing {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                left_low: *left_low,
                left_high: *left_high,
                right_low: *right_low,
                right_high: *right_high,
            },
            PersistentStrongholdPieceData::ChestCorridor {
                entry_door,
                has_placed_chest,
            } => StrongholdPieceData::ChestCorridor {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                has_placed_chest: *has_placed_chest,
            },
            PersistentStrongholdPieceData::Library {
                entry_door,
                is_tall,
            } => StrongholdPieceData::Library {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                is_tall: *is_tall,
            },
            PersistentStrongholdPieceData::PortalRoom { has_placed_spawner } => {
                StrongholdPieceData::PortalRoom {
                    has_placed_spawner: *has_placed_spawner,
                }
            }
            PersistentStrongholdPieceData::FillerCorridor { steps } => {
                StrongholdPieceData::FillerCorridor { steps: *steps }
            }
        }
    }

    fn mineshaft_kind_to_persistent(kind: &MineshaftPieceKind) -> PersistentMineshaftPieceKind {
        match kind {
            MineshaftPieceKind::Room {
                child_entrance_boxes,
            } => PersistentMineshaftPieceKind::Room {
                child_entrance_boxes: child_entrance_boxes
                    .iter()
                    .map(|&b| PersistentBoundingBox::from_bounding_box(b))
                    .collect(),
            },
            MineshaftPieceKind::Corridor {
                has_rails,
                spider_corridor,
                has_placed_spider,
                num_sections,
            } => PersistentMineshaftPieceKind::Corridor {
                has_rails: *has_rails,
                spider_corridor: *spider_corridor,
                has_placed_spider: *has_placed_spider,
                num_sections: *num_sections,
            },
            MineshaftPieceKind::Crossing {
                direction,
                is_two_floored,
            } => PersistentMineshaftPieceKind::Crossing {
                direction: direction_to_2d(Some(*direction)),
                is_two_floored: *is_two_floored,
            },
            MineshaftPieceKind::Stairs => PersistentMineshaftPieceKind::Stairs,
        }
    }

    fn persistent_to_mineshaft_kind(kind: &PersistentMineshaftPieceKind) -> MineshaftPieceKind {
        match kind {
            PersistentMineshaftPieceKind::Room {
                child_entrance_boxes,
            } => MineshaftPieceKind::Room {
                child_entrance_boxes: child_entrance_boxes
                    .iter()
                    .map(|b| b.to_bounding_box())
                    .collect(),
            },
            PersistentMineshaftPieceKind::Corridor {
                has_rails,
                spider_corridor,
                has_placed_spider,
                num_sections,
            } => MineshaftPieceKind::Corridor {
                has_rails: *has_rails,
                spider_corridor: *spider_corridor,
                has_placed_spider: *has_placed_spider,
                num_sections: *num_sections,
            },
            PersistentMineshaftPieceKind::Crossing {
                direction,
                is_two_floored,
            } => MineshaftPieceKind::Crossing {
                direction: required_direction_from_2d(*direction),
                is_two_floored: *is_two_floored,
            },
            PersistentMineshaftPieceKind::Stairs => MineshaftPieceKind::Stairs,
        }
    }

    fn structure_piece_payload_to_persistent(
        payload: &StructurePiecePayload,
    ) -> PersistentStructurePiecePayload {
        match payload {
            StructurePiecePayload::Jigsaw(data) => {
                PersistentStructurePiecePayload::Jigsaw(Self::jigsaw_piece_data_to_persistent(data))
            }
            StructurePiecePayload::Template(data) => {
                PersistentStructurePiecePayload::Template(PersistentTemplatePieceData {
                    template_id: data.template_id.clone(),
                    template_position: [
                        data.template_position.x,
                        data.template_position.y,
                        data.template_position.z,
                    ],
                    rotation: rotation_to_persistent(data.rotation),
                    mirror: mirror_to_persistent(data.mirror),
                    rotation_pivot: [
                        data.rotation_pivot.x,
                        data.rotation_pivot.y,
                        data.rotation_pivot.z,
                    ],
                    block_ignore: block_ignore_to_persistent(data.block_ignore),
                    late_block_ignore: block_ignore_to_persistent(data.late_block_ignore),
                    processors: Self::template_processors_to_persistent(&data.processors),
                    liquid_settings: liquid_settings_to_persistent(data.liquid_settings),
                    marker_handling: marker_handling_to_persistent(data.marker_handling),
                    placement_adjustment: placement_adjustment_to_persistent(
                        data.placement_adjustment,
                    ),
                    placement_clip: placement_clip_to_persistent(data.placement_clip),
                    post_process: post_process_to_persistent(data.post_process),
                })
            }
            StructurePiecePayload::Procedural(data) => PersistentStructurePiecePayload::Procedural(
                Self::procedural_piece_data_to_persistent(data),
            ),
        }
    }

    fn persistent_to_structure_piece_payload(
        payload: &PersistentStructurePiecePayload,
    ) -> StructurePiecePayload {
        match payload {
            PersistentStructurePiecePayload::Jigsaw(data) => {
                StructurePiecePayload::Jigsaw(Self::persistent_to_jigsaw_piece_data(data))
            }
            PersistentStructurePiecePayload::Template(data) => {
                StructurePiecePayload::Template(TemplatePieceData {
                    template_id: data.template_id.clone(),
                    template_position: IVec3::new(
                        data.template_position[0],
                        data.template_position[1],
                        data.template_position[2],
                    ),
                    rotation: rotation_from_persistent(data.rotation),
                    mirror: mirror_from_persistent(data.mirror),
                    rotation_pivot: IVec3::new(
                        data.rotation_pivot[0],
                        data.rotation_pivot[1],
                        data.rotation_pivot[2],
                    ),
                    block_ignore: block_ignore_from_persistent(data.block_ignore),
                    late_block_ignore: block_ignore_from_persistent(data.late_block_ignore),
                    processors: Self::persistent_to_template_processors(&data.processors),
                    liquid_settings: liquid_settings_from_persistent(data.liquid_settings),
                    marker_handling: marker_handling_from_persistent(data.marker_handling),
                    placement_adjustment: placement_adjustment_from_persistent(
                        &data.placement_adjustment,
                    ),
                    placement_clip: placement_clip_from_persistent(data.placement_clip),
                    post_process: post_process_from_persistent(data.post_process),
                })
            }
            PersistentStructurePiecePayload::Procedural(data) => {
                StructurePiecePayload::Procedural(Self::persistent_to_procedural_piece_data(data))
            }
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

    fn template_processors_to_persistent(
        processors: &TemplateProcessorList,
    ) -> PersistentTemplateProcessorList {
        match processors {
            TemplateProcessorList::Empty => PersistentTemplateProcessorList::Empty,
            TemplateProcessorList::Registry(id) => {
                PersistentTemplateProcessorList::Registry(id.clone())
            }
            TemplateProcessorList::OceanRuin {
                biome_temp,
                integrity,
            } => PersistentTemplateProcessorList::OceanRuin {
                biome_temp: ocean_ruin_biome_temp_to_persistent(*biome_temp),
                integrity: *integrity,
            },
            TemplateProcessorList::RuinedPortal {
                vertical_placement,
                properties,
            } => PersistentTemplateProcessorList::RuinedPortal {
                vertical_placement: ruined_portal_placement_to_persistent(*vertical_placement),
                cold: properties.cold,
                mossiness: properties.mossiness,
                air_pocket: properties.air_pocket,
                overgrown: properties.overgrown,
                vines: properties.vines,
                replace_with_blackstone: properties.replace_with_blackstone,
            },
        }
    }

    fn persistent_to_template_processors(
        processors: &PersistentTemplateProcessorList,
    ) -> TemplateProcessorList {
        match processors {
            PersistentTemplateProcessorList::Empty => TemplateProcessorList::Empty,
            PersistentTemplateProcessorList::Registry(id) => {
                TemplateProcessorList::Registry(id.clone())
            }
            PersistentTemplateProcessorList::OceanRuin {
                biome_temp,
                integrity,
            } => TemplateProcessorList::OceanRuin {
                biome_temp: ocean_ruin_biome_temp_from_persistent(*biome_temp),
                integrity: *integrity,
            },
            PersistentTemplateProcessorList::RuinedPortal {
                vertical_placement,
                cold,
                mossiness,
                air_pocket,
                overgrown,
                vines,
                replace_with_blackstone,
            } => TemplateProcessorList::RuinedPortal {
                vertical_placement: ruined_portal_placement_from_persistent(*vertical_placement),
                properties: RuinedPortalProperties {
                    cold: *cold,
                    mossiness: *mossiness,
                    air_pocket: *air_pocket,
                    overgrown: *overgrown,
                    vines: *vines,
                    replace_with_blackstone: *replace_with_blackstone,
                },
            },
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
                        bounding_box: PersistentBoundingBox::from_bounding_box(piece.bounding_box),
                        gen_depth: piece.gen_depth,
                        orientation: direction_to_2d(piece.orientation),
                        payload: Self::structure_piece_payload_to_persistent(&piece.payload),
                        ground_level_delta: piece.ground_level_delta,
                        projection: projection_to_persistent(piece.projection),
                        junctions: piece
                            .junctions
                            .iter()
                            .map(|junction| PersistentJigsawJunction {
                                source_x: junction.source_pos.x,
                                source_ground_y: junction.source_pos.y,
                                source_z: junction.source_pos.z,
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
                    let packed: Vec<_> = positions
                        .insertion_order_iter()
                        .copied()
                        .map(PackedChunkPos::from)
                        .collect();
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
                        bounding_box: pp.bounding_box.to_bounding_box(),
                        gen_depth: pp.gen_depth,
                        orientation: direction_from_2d(pp.orientation),
                        payload: Self::persistent_to_structure_piece_payload(&pp.payload),
                        ground_level_delta: pp.ground_level_delta,
                        junctions: pp
                            .junctions
                            .iter()
                            .map(|junction| JigsawJunction {
                                source_pos: IVec3::new(
                                    junction.source_x,
                                    junction.source_ground_y,
                                    junction.source_z,
                                ),
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
    use std::slice;

    use super::*;
    use std::sync::{Arc, Once};

    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::entity::{
        DEFAULT_MAX_AIR_SUPPLY, Entity, SharedEntity,
        entities::{EndCrystalEntity, RawEntity},
        init_test_entities, next_entity_id,
    };
    use glam::DVec3;
    use rustc_hash::FxHashMap;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_block_entity_types;
    use steel_registry::vanilla_blocks;
    use steel_registry::vanilla_entities;
    use steel_utils::BoundingBox;
    use steel_utils::types::UpdateFlags;
    use steel_worldgen::structure::StructureReferenceSet;
    use text_components::TextComponent;

    static RUNTIME_REGISTRIES: Once = Once::new();

    fn init_runtime_registries() {
        RUNTIME_REGISTRIES.call_once(|| {
            init_test_entities();
            init_behaviors();
            init_block_entities();
        });
    }

    fn test_structure_piece() -> StructurePiece {
        StructurePiece {
            piece_type: Identifier::new_static("minecraft", "mscorridor"),
            bounding_box: BoundingBox::new(IVec3::new(0, 64, 0), IVec3::new(1, 65, 1)),
            gen_depth: 0,
            orientation: None,
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        }
    }

    fn single_empty_section() -> Sections {
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice())
    }

    fn test_persistent_end_crystal(pos: DVec3) -> PersistentEntity {
        PersistentEntity {
            entity_type: vanilla_entities::END_CRYSTAL.key.clone(),
            uuid: [9; 16],
            pos: [pos.x, pos.y, pos.z],
            motion: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0],
            fall_distance: 0.0,
            remaining_fire_ticks: 0,
            ticks_frozen: 0,
            is_in_powder_snow: false,
            was_in_powder_snow: false,
            has_visual_fire: false,
            on_ground: false,
            no_gravity: false,
            invulnerable: false,
            air_supply: DEFAULT_MAX_AIR_SUPPLY,
            portal_cooldown: 0,
            custom_name_nbt: Vec::new(),
            custom_name_visible: false,
            silent: false,
            glowing: false,
            tags: Vec::new(),
            custom_data_nbt: Vec::new(),
            nbt_data: Vec::new(),
            passengers: Vec::new(),
        }
    }

    #[test]
    fn proto_carving_mask_presence_roundtrips_when_empty() {
        init_test_registry();

        let pos = ChunkPos::new(3, -4);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        proto.set_status(ChunkStatus::Carvers);
        drop(proto.get_or_create_carving_mask());
        let chunk = ChunkAccess::Proto(proto);

        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
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
        let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
            panic!("carvers status should load as proto chunk");
        };

        assert!(loaded_proto.carving_mask.read().is_some());
    }

    #[test]
    fn proto_carving_mask_bits_roundtrip_through_persistent_chunk() {
        init_test_registry();

        let pos = ChunkPos::new(3, -4);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        proto.set_status(ChunkStatus::Carvers);
        {
            let mut mask = proto.get_or_create_carving_mask();
            mask.set(7, 5, 11);
        }
        let chunk = ChunkAccess::Proto(proto);

        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
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
        let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
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
        init_test_registry();

        let pos = ChunkPos::new(-2, 1);
        let marked = BlockPos::new(-17, -63, 31);
        let proto = ProtoChunk::new(single_empty_section(), pos, -64, 16, Weak::new());
        proto.set_status(ChunkStatus::Noise);
        proto.mark_pos_for_postprocessing(marked);
        let packed = ProtoChunk::pack_postprocessing_offset(marked);
        let chunk = ChunkAccess::Proto(proto);

        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
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
        let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
            panic!("noise status should load as proto chunk");
        };

        assert_eq!(loaded_proto.postprocessing.read()[0], vec![packed]);
    }

    #[test]
    fn persistent_entity_load_clamps_position_like_vanilla() {
        init_runtime_registries();

        let persistent =
            test_persistent_end_crystal(DVec3::new(100_000_000.0, -100_000_000.0, -100_000_000.0));
        let Some(entity) = ChunkStorage::persistent_to_entity_at_level(
            &persistent,
            ChunkPos::new(0, 0),
            &Weak::new(),
        ) else {
            panic!("entity should load with clamped position");
        };

        assert_eq!(
            entity.position(),
            DVec3::new(
                ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
                -ENTITY_LOAD_MAX_VERTICAL_POSITION,
                -ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
            )
        );
    }

    #[test]
    fn persistent_entity_load_rejects_non_finite_rotation_like_vanilla() {
        init_runtime_registries();

        let mut persistent = test_persistent_end_crystal(DVec3::new(1.0, 2.0, 3.0));
        persistent.rotation = [f32::NAN, 0.0];

        assert!(
            ChunkStorage::persistent_to_entity_at_level(
                &persistent,
                ChunkPos::new(0, 0),
                &Weak::new(),
            )
            .is_none()
        );
    }

    #[test]
    fn proto_block_entities_roundtrip_and_promote_to_full_chunk() {
        init_runtime_registries();

        let pos = ChunkPos::new(0, 0);
        let block_pos = BlockPos::new(3, 4, 5);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        let barrel = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::BARREL);
        proto.set_block_state(block_pos, barrel, UpdateFlags::UPDATE_NONE);

        assert!(proto.get_block_entity(block_pos).is_some());

        let chunk = ChunkAccess::Proto(proto);
        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
            panic!("dirty proto chunk should prepare for saving");
        };
        assert_eq!(prepared.persistent.block_entities.len(), 1);

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Features,
            0,
            16,
            Weak::new(),
        );
        let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
            panic!("features status should load as proto chunk");
        };
        assert!(loaded_proto.get_block_entity(block_pos).is_some());

        let full = LevelChunk::from_proto(loaded_proto, 0, 16, Weak::new()).chunk;
        assert!(full.get_block_entity(block_pos).is_some());
    }

    #[test]
    fn proto_entities_roundtrip_and_promote_to_full_chunk() {
        init_runtime_registries();

        let pos = ChunkPos::new(0, 0);
        let entity_pos = DVec3::new(5.5, 6.0, 7.5);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        let crystal = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            entity_pos,
            Weak::new(),
        ));
        crystal.set_beam_target(Some(BlockPos::new(0, 64, 0)));
        crystal.set_invulnerable(true);
        crystal.set_fall_distance(3.75);
        crystal.set_no_gravity(true);
        crystal.set_air_supply(120);
        crystal.set_portal_cooldown(9);
        crystal.set_custom_name(Some(TextComponent::plain("End Test")));
        crystal.set_custom_name_visible(true);
        crystal.set_silent(true);
        crystal.set_glowing_tag(true);
        assert!(crystal.add_tag("steel:test".to_owned()));
        let mut custom_data = NbtCompound::new();
        custom_data.insert("marker", "roundtrip");
        crystal.set_custom_data(custom_data);
        proto.add_entity(crystal);

        let chunk = ChunkAccess::Proto(proto);
        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
            panic!("dirty proto chunk should prepare for saving");
        };
        assert_eq!(prepared.persistent.entities.len(), 1);
        assert!((prepared.persistent.entities[0].fall_distance - 3.75).abs() <= f64::EPSILON);
        assert!(prepared.persistent.entities[0].no_gravity);
        assert!(prepared.persistent.entities[0].invulnerable);
        assert_eq!(prepared.persistent.entities[0].air_supply, 120);
        assert_eq!(prepared.persistent.entities[0].portal_cooldown, 9);
        assert!(prepared.persistent.entities[0].custom_name_visible);
        assert!(prepared.persistent.entities[0].silent);
        assert!(prepared.persistent.entities[0].glowing);
        assert_eq!(
            prepared.persistent.entities[0].tags,
            vec!["steel:test".to_owned()]
        );
        assert!(!prepared.persistent.entities[0].custom_name_nbt.is_empty());
        assert!(!prepared.persistent.entities[0].custom_data_nbt.is_empty());

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Features,
            0,
            16,
            Weak::new(),
        );
        assert!(loaded.pending_entities.is_empty());
        let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
            panic!("features status should load as proto chunk");
        };
        assert_eq!(loaded_proto.get_entities().len(), 1);

        let promoted = LevelChunk::from_proto(loaded_proto, 0, 16, Weak::new());
        assert_eq!(promoted.pending_entities.len(), 1);
        assert!(promoted.pending_entities[0].is_no_gravity());
        assert!(promoted.pending_entities[0].is_invulnerable());
        assert_eq!(promoted.pending_entities[0].air_supply(), 120);
        assert_eq!(promoted.pending_entities[0].portal_cooldown(), 9);
        assert_eq!(
            promoted.pending_entities[0].custom_name(),
            Some(TextComponent::plain("End Test"))
        );
        assert!(promoted.pending_entities[0].is_custom_name_visible());
        assert!(promoted.pending_entities[0].is_silent());
        assert!(promoted.pending_entities[0].has_glowing_tag());
        assert_eq!(
            promoted.pending_entities[0].tags(),
            vec!["steel:test".to_owned()]
        );
        assert_eq!(
            promoted.pending_entities[0]
                .custom_data()
                .string("marker")
                .map(ToString::to_string),
            Some("roundtrip".to_owned())
        );
    }

    #[test]
    fn prepared_save_reports_handled_runtime_entity_ids() {
        init_runtime_registries();

        let pos = ChunkPos::new(0, 0);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        let chunk = ChunkAccess::Proto(proto);
        let entity: SharedEntity = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(5.5, 6.0, 7.5),
            Weak::new(),
        ));

        let Some(prepared) =
            ChunkStorage::prepare_chunk_save(&chunk, slice::from_ref(&entity), true)
        else {
            panic!("forced runtime entity save should prepare a chunk save");
        };

        assert_eq!(prepared.handled_runtime_entity_ids, vec![entity.id()]);
        assert_eq!(prepared.persistent.entities.len(), 1);
    }

    #[test]
    fn full_chunk_load_defers_entities_to_world_registration() {
        init_runtime_registries();

        let pos = ChunkPos::new(0, 0);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        let chunk = ChunkAccess::Proto(proto);
        let entity: SharedEntity = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(5.5, 6.0, 7.5),
            Weak::new(),
        ));

        let Some(prepared) =
            ChunkStorage::prepare_chunk_save(&chunk, slice::from_ref(&entity), true)
        else {
            panic!("forced runtime entity save should prepare a chunk save");
        };

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Full,
            0,
            16,
            Weak::new(),
        );

        assert!(matches!(loaded.chunk, ChunkAccess::Full(_)));
        assert_eq!(loaded.status, ChunkStatus::Full);
        assert_eq!(loaded.pending_entities.len(), 1);
        assert_eq!(loaded.pending_entities[0].uuid(), entity.uuid());
    }

    #[test]
    fn runtime_entity_passengers_save_nested_and_load_flattened_for_registration() {
        init_runtime_registries();

        let pos = ChunkPos::new(0, 0);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        let chunk = ChunkAccess::Proto(proto);
        let vehicle: SharedEntity = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(5.5, 6.0, 7.5),
            Weak::new(),
        ));
        let passenger: SharedEntity = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(5.5, 8.0, 7.5),
            Weak::new(),
        ));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);
        let vehicle_uuid = vehicle.uuid();
        let passenger_uuid = passenger.uuid();
        let entities = [Arc::clone(&vehicle), Arc::clone(&passenger)];

        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &entities, true) else {
            panic!("forced runtime entity save should prepare a chunk save");
        };

        assert_eq!(prepared.persistent.entities.len(), 1);
        assert_eq!(
            prepared.persistent.entities[0].uuid,
            *vehicle_uuid.as_bytes()
        );
        assert_eq!(prepared.persistent.entities[0].passengers.len(), 1);
        assert_eq!(
            prepared.persistent.entities[0].passengers[0].uuid,
            *passenger_uuid.as_bytes()
        );

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Full,
            0,
            16,
            Weak::new(),
        );

        assert!(matches!(loaded.chunk, ChunkAccess::Full(_)));
        assert_eq!(loaded.pending_entities.len(), 2);
        let Some(loaded_passenger) = loaded
            .pending_entities
            .iter()
            .find(|entity| entity.uuid() == passenger_uuid)
        else {
            panic!("passenger should load into pending registration list");
        };
        let Some(loaded_vehicle) = loaded_passenger.vehicle() else {
            panic!("passenger should restore its vehicle relationship");
        };
        assert_eq!(loaded_vehicle.uuid(), vehicle_uuid);
        assert!(loaded_vehicle.has_passenger(loaded_passenger.as_ref()));
    }

    #[test]
    fn runtime_entity_passengers_skip_non_serializable_entities_like_vanilla() {
        init_runtime_registries();

        let pos = ChunkPos::new(0, 0);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        let chunk = ChunkAccess::Proto(proto);
        let vehicle: SharedEntity = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(5.5, 6.0, 7.5),
            Weak::new(),
        ));
        let passenger: SharedEntity = Arc::new(RawEntity::new(
            next_entity_id(),
            DVec3::new(5.5, 8.0, 7.5),
            Weak::new(),
            &vanilla_entities::PLAYER,
        ));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);
        let vehicle_uuid = vehicle.uuid();

        let Some(prepared) =
            ChunkStorage::prepare_chunk_save(&chunk, slice::from_ref(&vehicle), true)
        else {
            panic!("forced runtime entity save should prepare a chunk save");
        };

        assert_eq!(prepared.persistent.entities.len(), 1);
        assert_eq!(
            prepared.persistent.entities[0].uuid,
            *vehicle_uuid.as_bytes()
        );
        assert!(prepared.persistent.entities[0].passengers.is_empty());

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Full,
            0,
            16,
            Weak::new(),
        );

        assert!(matches!(loaded.chunk, ChunkAccess::Full(_)));
        assert_eq!(loaded.pending_entities.len(), 1);
        assert_eq!(loaded.pending_entities[0].uuid(), vehicle_uuid);
    }

    #[test]
    fn unimplemented_block_entities_preserve_nbt_through_proto_save_load() {
        init_runtime_registries();

        let pos = ChunkPos::new(0, 0);
        let block_pos = BlockPos::new(4, 4, 6);
        let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
        let spawner = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::SPAWNER);
        proto.set_block_state(block_pos, spawner, UpdateFlags::UPDATE_NONE);

        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:chests/simple_dungeon");
        nbt.insert("LootTableSeed", 42_i64);
        let entity = BLOCK_ENTITIES.create_and_load_owned_or_raw(
            &vanilla_block_entity_types::MOB_SPAWNER,
            proto.level_weak(),
            block_pos,
            spawner,
            nbt,
        );
        proto.add_and_register_block_entity(entity);

        let chunk = ChunkAccess::Proto(proto);
        let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
            panic!("dirty proto chunk should prepare for saving");
        };
        assert_eq!(prepared.persistent.block_entities.len(), 1);

        let loaded = ChunkStorage::persistent_to_chunk(
            &prepared.persistent,
            pos,
            ChunkStatus::Features,
            0,
            16,
            Weak::new(),
        );
        let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
            panic!("features status should load as proto chunk");
        };
        let Some(loaded_entity) = loaded_proto.get_block_entity(block_pos) else {
            panic!("raw block entity should survive chunk load");
        };

        let mut saved = NbtCompound::new();
        let guard = loaded_entity.lock();
        assert_eq!(
            guard.get_type().id(),
            vanilla_block_entity_types::MOB_SPAWNER.id()
        );
        guard.save_additional(&mut saved);
        drop(guard);

        assert_eq!(
            saved.string("LootTable").map(ToString::to_string),
            Some("minecraft:chests/simple_dungeon".to_owned())
        );
        assert_eq!(saved.long("LootTableSeed"), Some(42));
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
            StructureReferenceSet::default(),
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
                PackedChunkPos::from(ChunkPos::new(2, 0)),
                PackedChunkPos::from(ChunkPos::new(1, 0))
            ]
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "single fixture verifies every persisted jigsaw field roundtrips together"
    )]
    fn structure_start_roundtrip_preserves_typed_jigsaw_state() {
        init_test_registry();

        let structure_id = Identifier::new_static("steel", "test_jigsaw_structure");
        let piece_type = Identifier::new_static("minecraft", "jigsaw");
        let template_id = Identifier::new_static("minecraft", "village/plains/houses/test_house");
        let processor_id = Identifier::new_static("minecraft", "street_plains");

        let piece = StructurePiece {
            piece_type: piece_type.clone(),
            bounding_box: BoundingBox::new(IVec3::new(10, 64, 20), IVec3::new(15, 70, 25)),
            gen_depth: 3,
            orientation: Some(Direction::North),
            payload: StructurePiecePayload::Jigsaw(JigsawPieceData {
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
                position: IVec3::new(10, 64, 20),
                rotation: Rotation::Clockwise90,
                liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
            }),
            ground_level_delta: 1,
            junctions: vec![JigsawJunction {
                source_pos: IVec3::new(12, 65, 24),
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
        assert_eq!(loaded_piece.ground_level_delta, 1);
        assert_eq!(loaded_piece.projection, Some(Projection::Rigid));
        assert_eq!(loaded_piece.junctions.len(), 1);
        assert_eq!(
            loaded_piece.junctions[0].dest_projection,
            Projection::TerrainMatching
        );

        let StructurePiecePayload::Jigsaw(jigsaw) = &loaded_piece.payload else {
            panic!("typed jigsaw state should roundtrip");
        };
        assert_eq!(jigsaw.position, IVec3::new(10, 64, 20));
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

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "single roundtrip fixture covers every structure piece payload variant together"
    )]
    fn structure_start_roundtrip_preserves_template_and_procedural_payloads() {
        init_test_registry();

        let structure_id = Identifier::new_static("steel", "test_payload_variants");
        let template_id = Identifier::new_static("minecraft", "shipwreck/with_mast");
        let igloo_template_id = Identifier::new_static("minecraft", "igloo/top");
        let ocean_ruin_template_id = Identifier::new_static("minecraft", "underwater_ruin/warm_1");
        let processor_id = Identifier::new_static("minecraft", "zombie_plains");

        let template_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "shipwreck"),
            bounding_box: BoundingBox::new(IVec3::new(0, 70, 0), IVec3::new(12, 80, 12)),
            gen_depth: 2,
            orientation: Some(Direction::East),
            payload: StructurePiecePayload::Template(TemplatePieceData {
                template_id: template_id.clone(),
                template_position: IVec3::new(1, 70, 2),
                rotation: Rotation::Clockwise180,
                mirror: StructureMirror::FrontBack,
                rotation_pivot: IVec3::new(4, 0, 15),
                block_ignore: StructureBlockIgnore::StructureAndAir,
                late_block_ignore: StructureBlockIgnore::None,
                processors: TemplateProcessorList::Registry(processor_id.clone()),
                liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
                marker_handling: TemplateMarkerHandling::DataMarkers,
                placement_adjustment: TemplatePlacementAdjustment::Shipwreck {
                    is_beached: true,
                    height_adjusted: false,
                },
                placement_clip: TemplatePlacementClip::CenterChunkExpandedToTemplate,
                post_process: TemplatePostProcess::NetherFossil,
            }),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let igloo_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "iglu"),
            bounding_box: BoundingBox::new(IVec3::new(4, 80, 4), IVec3::new(10, 84, 11)),
            gen_depth: 0,
            orientation: Some(Direction::North),
            payload: StructurePiecePayload::Template(TemplatePieceData {
                template_id: igloo_template_id.clone(),
                template_position: IVec3::new(4, 90, 4),
                rotation: Rotation::Clockwise90,
                mirror: StructureMirror::None,
                rotation_pivot: IVec3::new(3, 5, 5),
                block_ignore: StructureBlockIgnore::StructureBlock,
                late_block_ignore: StructureBlockIgnore::None,
                processors: TemplateProcessorList::Empty,
                liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
                marker_handling: TemplateMarkerHandling::Igloo,
                placement_adjustment: TemplatePlacementAdjustment::Igloo {
                    template_offset: (0, 0, 0),
                },
                placement_clip: TemplatePlacementClip::CenterChunk,
                post_process: TemplatePostProcess::IglooTop,
            }),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let ocean_ruin_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "orp"),
            bounding_box: BoundingBox::new(IVec3::new(12, 90, 12), IVec3::new(20, 96, 20)),
            gen_depth: 0,
            orientation: Some(Direction::North),
            payload: StructurePiecePayload::Template(TemplatePieceData {
                template_id: ocean_ruin_template_id.clone(),
                template_position: IVec3::new(12, 90, 12),
                rotation: Rotation::CounterClockwise90,
                mirror: StructureMirror::None,
                rotation_pivot: IVec3::new(0, 0, 0),
                block_ignore: StructureBlockIgnore::None,
                late_block_ignore: StructureBlockIgnore::StructureAndAir,
                processors: TemplateProcessorList::OceanRuin {
                    biome_temp: OceanRuinBiomeTempData::Warm,
                    integrity: 0.8,
                },
                liquid_settings: LiquidSettingsData::ApplyWaterlogging,
                marker_handling: TemplateMarkerHandling::OceanRuin { is_large: false },
                placement_adjustment: TemplatePlacementAdjustment::OceanRuin,
                placement_clip: TemplatePlacementClip::CenterChunk,
                post_process: TemplatePostProcess::None,
            }),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let procedural_piece = StructurePiece::non_jigsaw(
            Identifier::new_static("minecraft", "mscorridor"),
            BoundingBox::new(IVec3::new(20, 40, 20), IVec3::new(30, 50, 30)),
            5,
            Some(Direction::South),
        );
        let buried_treasure_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "btp"),
            bounding_box: BoundingBox::new(IVec3::new(41, 90, 43), IVec3::new(41, 90, 43)),
            gen_depth: 0,
            orientation: None,
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::BuriedTreasure),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let desert_pyramid_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "tedp"),
            bounding_box: BoundingBox::new(IVec3::new(48, 63, 48), IVec3::new(68, 77, 68)),
            gen_depth: 0,
            orientation: Some(Direction::East),
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::DesertPyramid(
                DesertPyramidPieceData {
                    height_position: Some(63),
                    has_placed_chest: [true, false, true, false],
                    potential_suspicious_sand_world_positions: vec![BlockPos::new(51, 64, 54)],
                    random_collapsed_roof_pos: BlockPos::new(50, 64, 50),
                },
            )),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let jungle_temple_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "tejp"),
            bounding_box: BoundingBox::new(IVec3::new(64, 63, 64), IVec3::new(75, 72, 78)),
            gen_depth: 0,
            orientation: Some(Direction::South),
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::JungleTemple(
                JungleTemplePieceData {
                    height_position: Some(64),
                    placed_main_chest: true,
                    placed_hidden_chest: false,
                    placed_trap1: true,
                    placed_trap2: false,
                },
            )),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let mineshaft_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "mscorridor"),
            bounding_box: BoundingBox::new(IVec3::new(32, 45, 32), IVec3::new(34, 47, 46)),
            gen_depth: 4,
            orientation: Some(Direction::North),
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(
                MineshaftPiecePayload {
                    mineshaft_type: MineshaftType::Mesa,
                    kind: MineshaftPieceKind::Corridor {
                        has_rails: true,
                        spider_corridor: false,
                        has_placed_spider: true,
                        num_sections: 3,
                    },
                },
            )),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let fortress_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "nemt"),
            bounding_box: BoundingBox::new(IVec3::new(48, 52, 48), IVec3::new(54, 59, 56)),
            gen_depth: 6,
            orientation: Some(Direction::East),
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::NetherFortress(
                FortressPieceData::MonsterThrone {
                    has_placed_spawner: true,
                },
            )),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let ocean_monument_room = OceanMonumentRoomData {
            index: 12,
            has_opening: [false, true, true, false, true, false],
            has_up_connection: true,
        };
        let ocean_monument_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "omb"),
            bounding_box: BoundingBox::new(IVec3::new(64, 39, 64), IVec3::new(121, 61, 121)),
            gen_depth: 0,
            orientation: Some(Direction::South),
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::OceanMonument(
                OceanMonumentPieceData {
                    child_pieces: vec![
                        OceanMonumentChildPiece {
                            bounding_box: BoundingBox::new(
                                IVec3::new(73, 39, 86),
                                IVec3::new(80, 42, 93),
                            ),
                            kind: OceanMonumentChildPieceKind::SimpleRoom {
                                room: ocean_monument_room,
                                main_design: 2,
                            },
                        },
                        OceanMonumentChildPiece {
                            bounding_box: BoundingBox::new(
                                IVec3::new(65, 40, 65),
                                IVec3::new(87, 47, 85),
                            ),
                            kind: OceanMonumentChildPieceKind::WingRoom { main_design: 1 },
                        },
                        OceanMonumentChildPiece {
                            bounding_box: BoundingBox::new(
                                IVec3::new(86, 52, 86),
                                IVec3::new(99, 56, 99),
                            ),
                            kind: OceanMonumentChildPieceKind::Penthouse,
                        },
                    ],
                },
            )),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let stronghold_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "shrc"),
            bounding_box: BoundingBox::new(IVec3::new(55, 35, 55), IVec3::new(65, 41, 65)),
            gen_depth: 7,
            orientation: Some(Direction::North),
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::Stronghold(
                StrongholdPieceData::RoomCrossing {
                    entry_door: StrongholdSmallDoorType::IronDoor,
                    crossing_type: 2,
                },
            )),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };
        let swamp_hut_piece = StructurePiece {
            piece_type: Identifier::new_static("minecraft", "tesh"),
            bounding_box: BoundingBox::new(IVec3::new(80, 63, 80), IVec3::new(86, 69, 88)),
            gen_depth: 0,
            orientation: Some(Direction::West),
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::SwampHut(
                SwampHutPieceData {
                    height_position: Some(62),
                    spawned_witch: true,
                    spawned_cat: false,
                },
            )),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        };

        let start = StructureStart::new(
            structure_id.clone(),
            ChunkPos::new(8, 9),
            vec![
                template_piece,
                igloo_piece,
                ocean_ruin_piece,
                procedural_piece,
                buried_treasure_piece,
                desert_pyramid_piece,
                jungle_temple_piece,
                mineshaft_piece,
                fortress_piece,
                ocean_monument_piece,
                stronghold_piece,
                swamp_hut_piece,
            ],
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
        assert_eq!(loaded_start.pieces.len(), 12);

        let StructurePiecePayload::Template(template) = &loaded_start.pieces[0].payload else {
            panic!("template payload should roundtrip");
        };
        assert_eq!(template.template_id, template_id);
        assert_eq!(template.template_position, IVec3::new(1, 70, 2));
        assert_eq!(template.rotation, Rotation::Clockwise180);
        assert_eq!(template.mirror, StructureMirror::FrontBack);
        assert_eq!(template.rotation_pivot, IVec3::new(4, 0, 15));
        assert_eq!(template.block_ignore, StructureBlockIgnore::StructureAndAir);
        assert_eq!(template.late_block_ignore, StructureBlockIgnore::None);
        assert_eq!(
            template.liquid_settings,
            LiquidSettingsData::IgnoreWaterlogging
        );
        assert_eq!(
            template.marker_handling,
            TemplateMarkerHandling::DataMarkers
        );
        assert_eq!(
            template.placement_adjustment,
            TemplatePlacementAdjustment::Shipwreck {
                is_beached: true,
                height_adjusted: false,
            }
        );
        assert_eq!(
            template.placement_clip,
            TemplatePlacementClip::CenterChunkExpandedToTemplate
        );
        assert_eq!(template.post_process, TemplatePostProcess::NetherFossil);
        assert_eq!(
            template.processors,
            TemplateProcessorList::Registry(processor_id.clone())
        );

        let StructurePiecePayload::Template(template) = &loaded_start.pieces[1].payload else {
            panic!("igloo template payload should roundtrip");
        };
        assert_eq!(template.template_id, igloo_template_id);
        assert_eq!(template.template_position, IVec3::new(4, 90, 4));
        assert_eq!(template.rotation, Rotation::Clockwise90);
        assert_eq!(template.mirror, StructureMirror::None);
        assert_eq!(template.rotation_pivot, IVec3::new(3, 5, 5));
        assert_eq!(template.block_ignore, StructureBlockIgnore::StructureBlock);
        assert_eq!(template.late_block_ignore, StructureBlockIgnore::None);
        assert_eq!(template.processors, TemplateProcessorList::Empty);
        assert_eq!(template.marker_handling, TemplateMarkerHandling::Igloo);
        assert_eq!(
            template.placement_adjustment,
            TemplatePlacementAdjustment::Igloo {
                template_offset: (0, 0, 0),
            }
        );
        assert_eq!(template.placement_clip, TemplatePlacementClip::CenterChunk);
        assert_eq!(template.post_process, TemplatePostProcess::IglooTop);

        let StructurePiecePayload::Template(template) = &loaded_start.pieces[2].payload else {
            panic!("ocean ruin template payload should roundtrip");
        };
        assert_eq!(template.template_id, ocean_ruin_template_id);
        assert_eq!(template.template_position, IVec3::new(12, 90, 12));
        assert_eq!(template.rotation, Rotation::CounterClockwise90);
        assert_eq!(template.mirror, StructureMirror::None);
        assert_eq!(template.rotation_pivot, IVec3::new(0, 0, 0));
        assert_eq!(template.block_ignore, StructureBlockIgnore::None);
        assert_eq!(
            template.late_block_ignore,
            StructureBlockIgnore::StructureAndAir
        );
        assert_eq!(
            template.processors,
            TemplateProcessorList::OceanRuin {
                biome_temp: OceanRuinBiomeTempData::Warm,
                integrity: 0.8,
            }
        );
        assert_eq!(
            template.marker_handling,
            TemplateMarkerHandling::OceanRuin { is_large: false }
        );
        assert_eq!(
            template.placement_adjustment,
            TemplatePlacementAdjustment::OceanRuin
        );
        assert_eq!(template.placement_clip, TemplatePlacementClip::CenterChunk);
        assert_eq!(template.post_process, TemplatePostProcess::None);

        assert!(matches!(
            loaded_start.pieces[3].payload,
            StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented)
        ));
        assert!(matches!(
            loaded_start.pieces[4].payload,
            StructurePiecePayload::Procedural(ProceduralPieceData::BuriedTreasure)
        ));

        let StructurePiecePayload::Procedural(ProceduralPieceData::DesertPyramid(payload)) =
            &loaded_start.pieces[5].payload
        else {
            panic!("desert pyramid payload should roundtrip");
        };
        assert_eq!(payload.height_position, Some(63));
        assert_eq!(payload.has_placed_chest, [true, false, true, false]);
        assert!(payload.potential_suspicious_sand_world_positions.is_empty());
        assert_eq!(payload.random_collapsed_roof_pos, BlockPos::new(0, 0, 0));

        let StructurePiecePayload::Procedural(ProceduralPieceData::JungleTemple(payload)) =
            &loaded_start.pieces[6].payload
        else {
            panic!("jungle temple payload should roundtrip");
        };
        assert_eq!(payload.height_position, Some(64));
        assert!(payload.placed_main_chest);
        assert!(!payload.placed_hidden_chest);
        assert!(payload.placed_trap1);
        assert!(!payload.placed_trap2);

        let StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(payload)) =
            &loaded_start.pieces[7].payload
        else {
            panic!("mineshaft payload should roundtrip");
        };
        assert_eq!(payload.mineshaft_type, MineshaftType::Mesa);
        let MineshaftPieceKind::Corridor {
            has_rails,
            spider_corridor,
            has_placed_spider,
            num_sections,
        } = &payload.kind
        else {
            panic!("expected mineshaft corridor payload");
        };
        assert!(*has_rails);
        assert!(!*spider_corridor);
        assert!(*has_placed_spider);
        assert_eq!(*num_sections, 3);

        let StructurePiecePayload::Procedural(ProceduralPieceData::NetherFortress(
            fortress_payload,
        )) = &loaded_start.pieces[8].payload
        else {
            panic!("nether fortress payload should roundtrip");
        };
        assert_eq!(
            *fortress_payload,
            FortressPieceData::MonsterThrone {
                has_placed_spawner: true,
            }
        );

        let StructurePiecePayload::Procedural(ProceduralPieceData::OceanMonument(payload)) =
            &loaded_start.pieces[9].payload
        else {
            panic!("ocean monument payload should roundtrip");
        };
        assert_eq!(payload.child_pieces.len(), 3);
        let OceanMonumentChildPieceKind::SimpleRoom { room, main_design } =
            &payload.child_pieces[0].kind
        else {
            panic!("ocean monument simple room child should roundtrip");
        };
        assert_eq!(*room, ocean_monument_room);
        assert_eq!(*main_design, 2);
        assert!(matches!(
            payload.child_pieces[1].kind,
            OceanMonumentChildPieceKind::WingRoom { main_design: 1 }
        ));
        assert!(matches!(
            payload.child_pieces[2].kind,
            OceanMonumentChildPieceKind::Penthouse
        ));

        let StructurePiecePayload::Procedural(ProceduralPieceData::Stronghold(stronghold_payload)) =
            &loaded_start.pieces[10].payload
        else {
            panic!("stronghold payload should roundtrip");
        };
        assert_eq!(
            *stronghold_payload,
            StrongholdPieceData::RoomCrossing {
                entry_door: StrongholdSmallDoorType::IronDoor,
                crossing_type: 2,
            }
        );

        let StructurePiecePayload::Procedural(ProceduralPieceData::SwampHut(payload)) =
            &loaded_start.pieces[11].payload
        else {
            panic!("swamp hut payload should roundtrip");
        };
        assert_eq!(payload.height_position, Some(62));
        assert!(payload.spawned_witch);
        assert!(!payload.spawned_cat);
    }

    #[test]
    fn template_processor_list_roundtrips_ruined_portal_processors() {
        let ocean_ruin_processors = TemplateProcessorList::OceanRuin {
            biome_temp: OceanRuinBiomeTempData::Cold,
            integrity: 0.7,
        };
        let persistent = ChunkStorage::template_processors_to_persistent(&ocean_ruin_processors);
        let loaded = ChunkStorage::persistent_to_template_processors(&persistent);
        assert_eq!(loaded, ocean_ruin_processors);

        let processors = TemplateProcessorList::RuinedPortal {
            vertical_placement: RuinedPortalPlacementData::OnOceanFloor,
            properties: RuinedPortalProperties {
                cold: true,
                mossiness: 0.8,
                air_pocket: false,
                overgrown: true,
                vines: true,
                replace_with_blackstone: false,
            },
        };

        let persistent = ChunkStorage::template_processors_to_persistent(&processors);
        let loaded = ChunkStorage::persistent_to_template_processors(&persistent);

        assert_eq!(loaded, processors);
        assert_eq!(
            placement_clip_from_persistent(placement_clip_to_persistent(
                TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
            )),
            TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
        );
        assert_eq!(
            post_process_from_persistent(post_process_to_persistent(
                TemplatePostProcess::RuinedPortal
            )),
            TemplatePostProcess::RuinedPortal,
        );
        assert_eq!(
            marker_handling_from_persistent(marker_handling_to_persistent(
                TemplateMarkerHandling::EndCity
            )),
            TemplateMarkerHandling::EndCity,
        );
        assert_eq!(
            marker_handling_from_persistent(marker_handling_to_persistent(
                TemplateMarkerHandling::WoodlandMansion
            )),
            TemplateMarkerHandling::WoodlandMansion,
        );
    }
}
