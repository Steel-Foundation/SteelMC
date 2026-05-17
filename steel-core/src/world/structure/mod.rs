//! Structure start/reference tracking.
//!
//! Vanilla keeps two per-chunk maps: `structureStarts` (originating here) and
//! `structuresReferences` (pointing at nearby origin chunks). The structure key
//! is `Identifier` until a structure registry is added.

pub mod end_city;
pub mod fortress;
pub mod igloo;
pub mod jigsaw;
pub mod mansion;
pub mod mineshaft;
pub mod nether_fossil;
pub mod ocean_monument;
pub mod ocean_ruin;
pub mod placement;
pub mod ruined_portal;
pub mod shipwreck;
pub mod single_piece;
pub mod stronghold;

use std::{cell::RefCell, slice};

use rustc_hash::FxHashMap;

use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{Random, RandomSplitter};
use steel_utils::{BlockPos, BoundingBox, ChunkPos, Direction, Identifier, Rotation};
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};

use steel_registry::biome::BiomeRef;
use steel_registry::structure::{
    LiquidSettingsData, RuinedPortalPlacementData, StructureData, TerrainAdjustment,
};
use steel_registry::template_pool::{Projection, TemplateData, TemplatePoolData};

use crate::worldgen::ChunkBiomeSampler;
use crate::worldgen::generators::vanilla::{
    column_base_height, column_interpolated_density, iterate_noise_column_with_aquifer,
};
use crate::worldgen::noise::aquifer::{Aquifer, AquiferResult, LazyAquifer};

const VANILLA_HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// Matches vanilla's `Direction.Plane.HORIZONTAL.getRandomDirection`.
pub(crate) fn random_horizontal_direction(rng: &mut LegacyRandom) -> Direction {
    VANILLA_HORIZONTAL_DIRECTIONS[rng.next_i32_bounded(4) as usize]
}

/// A structure start placed in a chunk. Vanilla's `StructureStart` — invalid (empty)
/// starts are not stored.
#[derive(Debug, Clone)]
pub struct StructureStart {
    /// Structure id (e.g., `minecraft:village`).
    pub structure: Identifier,
    /// Origin chunk.
    pub chunk_pos: ChunkPos,
    /// Vanilla's map/locate reference counter. This is distinct from
    /// [`StructureReferenceMap`]; generating per-chunk structure references does
    /// not increment this counter.
    pub references: i32,
    /// Pieces composing this structure.
    pub pieces: Vec<StructurePiece>,
    /// Bounding-box inflation applied at construction. Vanilla inflates by 12
    /// when `terrain_adaptation != NONE`. Stored for serialization parity; the
    /// inflation is already baked into [`bounding_box`](Self::bounding_box).
    pub bb_inflate: i32,
    /// Terrain adaptation mode from the structure registry. Used by Beardifier.
    pub terrain_adjustment: TerrainAdjustment,
    /// Cached bounding box matching vanilla's `StructureStart.getBoundingBox()`:
    /// the union of piece bounding boxes, then `inflatedBy(bb_inflate)`.
    /// `None` iff `pieces` is empty.
    pub bounding_box: Option<BoundingBox>,
}

impl StructureStart {
    /// Creates a start, computing the inflated piece-union bounding box up-front.
    #[must_use]
    pub fn new(
        structure: Identifier,
        chunk_pos: ChunkPos,
        pieces: Vec<StructurePiece>,
        terrain_adjustment: TerrainAdjustment,
    ) -> Self {
        let bb_inflate = terrain_adjustment.bb_inflate();
        let bounding_box = Self::compute_bounding_box(&pieces, bb_inflate);
        Self {
            structure,
            chunk_pos,
            references: 0,
            pieces,
            bb_inflate,
            terrain_adjustment,
            bounding_box,
        }
    }

    /// Union of all pieces' bounding boxes, inflated by `bb_inflate` on every
    /// axis. Returns `None` if `pieces` is empty. Mirrors vanilla's
    /// `StructureStart.getBoundingBox()` (= `adjustBoundingBox(union)`).
    #[must_use]
    pub fn compute_bounding_box(pieces: &[StructurePiece], bb_inflate: i32) -> Option<BoundingBox> {
        let (first, rest) = pieces.split_first()?;
        let mut bb = first.bounding_box;
        for piece in rest {
            bb = BoundingBox::new(
                bb.min_x.min(piece.bounding_box.min_x),
                bb.min_y.min(piece.bounding_box.min_y),
                bb.min_z.min(piece.bounding_box.min_z),
                bb.max_x.max(piece.bounding_box.max_x),
                bb.max_y.max(piece.bounding_box.max_y),
                bb.max_z.max(piece.bounding_box.max_z),
            );
        }
        Some(bb.inflated_by(bb_inflate, bb_inflate, bb_inflate))
    }

    /// Vanilla `StructureStart.placeInChunk` reference position: the first
    /// piece center X/Z and first piece minimum Y.
    #[must_use]
    pub fn placement_reference_pos(&self) -> Option<BlockPos> {
        let first_piece = self.pieces.first()?;
        let center = first_piece.bounding_box.get_center();
        Some(BlockPos::new(
            center.x(),
            first_piece.bounding_box.min_y,
            center.z(),
        ))
    }
}

/// Vanilla's `StructurePiece` runtime state.
#[derive(Debug, Clone)]
pub struct StructurePiece {
    /// Piece type id (e.g., `minecraft:jigsaw`).
    pub piece_type: Identifier,
    /// World-space bounding box.
    pub bounding_box: BoundingBox,
    /// Distance from the start piece in the piece tree.
    pub gen_depth: i32,
    /// Horizontal orientation; `None` for unoriented pieces.
    pub orientation: Option<Direction>,
    /// Type-specific data used by the structure-piece placement stage.
    pub payload: StructurePiecePayload,
    /// Offset from piece minY to ground level. Used by Beardifier. Default 0 for non-jigsaw.
    pub ground_level_delta: i32,
    /// Junctions for Beardifier terrain adaptation.
    pub junctions: Vec<jigsaw::JigsawJunction>,
    /// Jigsaw projection. `None` for non-jigsaw pieces.
    ///
    /// Beardifier treats `Some(Rigid)` and `None` as terrain-adapting, but skips
    /// `Some(TerrainMatching)` from the rigid set (still collecting junctions).
    /// Mirrors vanilla's `piece instanceof PoolElementStructurePiece` + `Projection.RIGID` check.
    pub projection: Option<Projection>,
}

impl StructurePiece {
    /// Creates a non-jigsaw piece with vanilla's default non-pool metadata.
    #[must_use]
    pub const fn non_jigsaw(
        piece_type: Identifier,
        bounding_box: BoundingBox,
        gen_depth: i32,
        orientation: Option<Direction>,
    ) -> Self {
        Self {
            piece_type,
            bounding_box,
            gen_depth,
            orientation,
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        }
    }
}

/// Type-specific structure-piece placement payload.
///
/// This is Steel's boundary between structure-start generation and feature-stage
/// block placement. Common vanilla fields stay on [`StructurePiece`]; placement
/// implementations dispatch on this enum instead of inferring behavior from a
/// bounding box or legacy NBT blob.
#[derive(Debug, Clone)]
pub enum StructurePiecePayload {
    /// Pool piece produced by jigsaw assembly.
    Jigsaw(jigsaw::JigsawPieceData),
    /// Template-backed vanilla piece outside the jigsaw system.
    Template(TemplatePieceData),
    /// Code-generated piece family whose blocks are emitted procedurally.
    Procedural(ProceduralPieceData),
}

/// Template-backed non-jigsaw placement data.
#[derive(Debug, Clone)]
pub struct TemplatePieceData {
    /// Structure template identifier.
    pub template_id: Identifier,
    /// World-space template origin before rotation/mirror transforms.
    pub template_position: (i32, i32, i32),
    /// Template rotation.
    pub rotation: Rotation,
    /// Template mirror mode.
    pub mirror: StructureMirror,
    /// Rotation pivot in template-local block coordinates.
    pub rotation_pivot: (i32, i32, i32),
    /// Block-ignore processor applied before the registry processor list.
    pub block_ignore: StructureBlockIgnore,
    /// Block-ignore processor applied after the registry processor list.
    pub late_block_ignore: StructureBlockIgnore,
    /// Processor list applied during placement.
    pub processors: TemplateProcessorList,
    /// Liquid handling mode used by vanilla template placement.
    pub liquid_settings: LiquidSettingsData,
    /// How structure-template data markers are handled for this family.
    pub marker_handling: TemplateMarkerHandling,
    /// Family-specific position adjustment before template block placement.
    pub placement_adjustment: TemplatePlacementAdjustment,
    /// Bounding box passed to vanilla template placement.
    pub placement_clip: TemplatePlacementClip,
    /// Family-specific work done after the template blocks are placed.
    pub post_process: TemplatePostProcess,
}

/// Processors for template-backed non-jigsaw pieces.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateProcessorList {
    /// No processors.
    Empty,
    /// Registry-backed vanilla processor list.
    Registry(Identifier),
    /// Vanilla's hardcoded ruined-portal processor sequence.
    RuinedPortal {
        /// Vertical placement controls lava replacement.
        vertical_placement: RuinedPortalPlacementData,
        /// Ruined portal setup properties.
        properties: RuinedPortalProperties,
    },
}

/// Vanilla ruined-portal piece properties used by processors and postprocess.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuinedPortalProperties {
    /// Whether cold lava/netherrack behavior is active.
    pub cold: bool,
    /// Vanilla block-age processor mossiness.
    pub mossiness: f32,
    /// Whether structure air is preserved.
    pub air_pocket: bool,
    /// Whether netherrack can grow jungle leaves.
    pub overgrown: bool,
    /// Whether vines can be added to sturdy sides.
    pub vines: bool,
    /// Whether stone ruin blocks are replaced with blackstone variants.
    pub replace_with_blackstone: bool,
}

/// Vanilla `Mirror` modes used by template placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureMirror {
    /// No mirror transform.
    None,
    /// Mirror across the template front/back axis.
    FrontBack,
    /// Mirror across the template left/right axis.
    LeftRight,
}

/// Hardcoded vanilla block-ignore processors used by template placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureBlockIgnore {
    /// Do not ignore any block states.
    None,
    /// Ignore structure blocks.
    StructureBlock,
    /// Ignore structure blocks and air.
    StructureAndAir,
}

/// Marker handling requested by a template-backed structure piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateMarkerHandling {
    /// Ignore data markers.
    Ignore,
    /// Dispatch data markers to the structure-family placement code.
    DataMarkers,
    /// Shipwreck map, supply, and treasure chest markers.
    Shipwreck,
    /// Igloo basement chest marker.
    Igloo,
}

/// Family-specific template position adjustment before block placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplatePlacementAdjustment {
    /// Place at the persisted template position.
    None,
    /// Shipwreck height adjustment, persisted after the first placement call.
    Shipwreck {
        /// Whether this is the beached shipwreck variant.
        is_beached: bool,
        /// Vanilla `height_adjusted` flag.
        height_adjusted: bool,
    },
    /// Igloo per-placement height adjustment.
    Igloo {
        /// Vanilla template offset for this igloo piece.
        template_offset: (i32, i32, i32),
    },
}

/// Vanilla bounding box adjustment before calling `StructureTemplate.placeInWorld`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplatePlacementClip {
    /// Use the center chunk's writable box unchanged.
    CenterChunk,
    /// Expand the center chunk writable box to include this piece's transformed template box.
    CenterChunkExpandedToTemplate,
}

/// Family-specific post-template processing for template-backed pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplatePostProcess {
    /// No family-specific postprocess.
    None,
    /// Nether fossil dried-ghast placement.
    NetherFossil,
    /// Igloo top-piece trapdoor snow-block fixup.
    IglooTop,
}

/// Family-specific state for code-generated structure pieces.
#[derive(Debug, Clone)]
pub enum ProceduralPieceData {
    /// Procedural family whose placement implementation has not been enabled yet.
    Unimplemented,
    /// Mineshaft room/corridor/crossing/stairs payload.
    Mineshaft(mineshaft::MineshaftPiecePayload),
}

/// Structure starts keyed by structure id.
pub type StructureStartMap = FxHashMap<Identifier, StructureStart>;

/// Structure references → origin chunk positions.
///
/// Vanilla stores these as a `LongSet`, so duplicates are ignored by construction.
pub type StructureReferenceMap = FxHashMap<Identifier, StructureReferenceSet>;

/// Insertion-ordered set of structure-start chunk positions.
///
/// Vanilla reference storage is set-like, but feature-stage structure placement
/// consumes these positions while seeding per-structure RNG. Steel keeps the
/// scan insertion order explicit instead of relying on hash-set iteration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructureReferenceSet {
    positions: Vec<ChunkPos>,
}

impl StructureReferenceSet {
    /// Inserts a chunk position if it was not already present.
    pub fn insert(&mut self, pos: ChunkPos) -> bool {
        if self.positions.contains(&pos) {
            return false;
        }
        self.positions.push(pos);
        true
    }

    /// Extends this set with insertion-order duplicate removal.
    pub fn extend(&mut self, positions: impl IntoIterator<Item = ChunkPos>) {
        for pos in positions {
            self.insert(pos);
        }
    }

    /// Returns an iterator over positions in insertion order.
    pub fn iter(&self) -> slice::Iter<'_, ChunkPos> {
        self.positions.iter()
    }

    /// Returns `true` when no positions are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

impl FromIterator<ChunkPos> for StructureReferenceSet {
    fn from_iter<T: IntoIterator<Item = ChunkPos>>(iter: T) -> Self {
        let mut set = Self::default();
        set.extend(iter);
        set
    }
}

impl<'a> IntoIterator for &'a StructureReferenceSet {
    type IntoIter = slice::Iter<'a, ChunkPos>;
    type Item = &'a ChunkPos;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for StructureReferenceSet {
    type IntoIter = std::vec::IntoIter<ChunkPos>;
    type Item = ChunkPos;

    fn into_iter(self) -> Self::IntoIter {
        self.positions.into_iter()
    }
}

/// Block classification in the base-noise column (no surface rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnBlock {
    /// Empty.
    Air,
    /// Aquifer-placed fluid (lava/water).
    Fluid,
    /// Default solid block (stone, netherrack, end stone).
    Solid,
}

/// Per-chunk context shared by every structure's `findGenerationPoint`.
///
/// Holds mutable per-chunk state (biome sampler, height cache, aquifer) so structures
/// don't each allocate their own. Wraps `VanillaGenerator`'s helpers.
pub struct GenerationContext<'ctx, 'src, N: DimensionNoises>
where
    'src: 'ctx,
{
    /// World seed.
    pub seed: i64,
    /// Chunk being populated.
    pub chunk_x: i32,
    /// Chunk being populated.
    pub chunk_z: i32,
    /// `chunk_x * 16`.
    pub chunk_min_x: i32,
    /// `chunk_z * 16`.
    pub chunk_min_z: i32,
    /// `chunk_min_x + 8`.
    pub center_block_x: i32,
    /// `chunk_min_z + 8`.
    pub center_block_z: i32,
    /// Sea level for this dimension.
    pub sea_level: i32,
    /// Shared memoisation slot for the chunk-center surface Y.
    pub(crate) surface_y_cache: &'ctx mut Option<i32>,
    /// Whether `height_cache`'s 5×5 quart grid has been populated. Shared across
    /// per-structure contexts in the same chunk.
    pub(crate) height_cache_grid_ready: &'ctx mut bool,

    /// Dimension noise router.
    pub noises: &'src N,
    /// Positional splitter for per-chunk RNG.
    pub splitter: &'src RandomSplitter,
    /// Template pool registry for jigsaw assembly.
    pub template_pools: &'src FxHashMap<Identifier, TemplatePoolData>,
    /// Structure templates (piece definitions + sizes).
    pub templates: &'src FxHashMap<Identifier, TemplateData>,

    /// Biome sampler scoped to this chunk.
    pub biome_sampler: &'ctx mut ChunkBiomeSampler<'src>,
    /// Column cache for height/density queries (grid-initialized on demand).
    pub height_cache: &'ctx mut N::ColumnCache,
    /// Aquifer built on first query; skipped on chunks where no structure needs it.
    pub aquifer: &'ctx mut LazyAquifer<'src, N>,
    pub(crate) terrain_height_cache: RefCell<FxHashMap<(i32, i32, bool), i32>>,
    pub(crate) terrain_opaque_cache: RefCell<FxHashMap<(i32, i32, i32, bool), bool>>,
    pub(crate) terrain_probes: RefCell<FxHashMap<(i32, i32), TerrainProbe<N>>>,
}

pub(crate) struct TerrainProbe<N: DimensionNoises> {
    cache: N::ColumnCache,
    aquifer: Aquifer<N>,
}

impl<N: DimensionNoises> TerrainProbe<N> {
    fn new(chunk_min_x: i32, chunk_min_z: i32, splitter: &RandomSplitter, noises: &N) -> Self {
        let mut cache = N::ColumnCache::default();
        cache.init_grid(chunk_min_x, chunk_min_z, noises);
        let aquifer = Aquifer::<N>::new(
            chunk_min_x,
            chunk_min_z,
            N::Settings::MIN_Y,
            N::Settings::HEIGHT,
            splitter,
            noises,
            cache.clone(),
        );
        Self { cache, aquifer }
    }
}

/// Result of a successful `Structure::find_generation_point`.
pub struct GenerationStub {
    /// World-space position the start anchors at.
    pub position: (i32, i32, i32),
    /// Pieces already sized and positioned in world space.
    pub pieces: Vec<StructurePiece>,
}

/// Terrain, biome, and template queries exposed to structure algorithms.
///
/// Vanilla calls these through `ChunkGenerator`/`WorldGenLevel`; keeping the
/// interface here lets structure algorithms stay independent of a concrete
/// chunk generator while preserving their vanilla query order.
pub trait StructureGenerationContext {
    /// World seed.
    fn seed(&self) -> i64;
    /// Chunk X being populated.
    fn chunk_x(&self) -> i32;
    /// Chunk Z being populated.
    fn chunk_z(&self) -> i32;
    /// Minimum block X of the chunk.
    fn chunk_min_x(&self) -> i32;
    /// Minimum block Z of the chunk.
    fn chunk_min_z(&self) -> i32;
    /// Center block X of the chunk.
    fn center_block_x(&self) -> i32;
    /// Center block Z of the chunk.
    fn center_block_z(&self) -> i32;
    /// Sea level for this generator/dimension.
    fn sea_level(&self) -> i32;
    /// Minimum build Y.
    fn min_y(&self) -> i32;
    /// Total build height.
    fn height(&self) -> i32;
    /// One-past-maximum build Y.
    fn max_y(&self) -> i32 {
        self.min_y() + self.height()
    }
    /// Template pool registry for jigsaw assembly.
    fn template_pools(&self) -> &FxHashMap<Identifier, TemplatePoolData>;
    /// Structure templates (piece definitions + sizes).
    fn templates(&self) -> &FxHashMap<Identifier, TemplateData>;
    /// Base height at a column.
    fn base_height(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32;
    /// Full-column base height scan.
    fn base_height_full(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32;
    /// Biome at a block position.
    fn biome_at(&mut self, block_x: i32, block_y: i32, block_z: i32) -> BiomeRef;
    /// Classify a block in the generator's base terrain.
    fn column_state(&mut self, x: i32, y: i32, z: i32) -> ColumnBlock;
    /// Chunk-center surface Y, memoised by the concrete context.
    fn surface_y(&mut self) -> i32;
    /// Surface height for off-chunk terrain queries used by piece placement.
    fn terrain_surface_height(&self, x: i32, z: i32, ocean_floor: bool) -> i32;
    /// Opaque terrain test for off-chunk terrain queries used by piece placement.
    fn terrain_is_opaque(&self, x: i32, y: i32, z: i32, ocean_floor: bool) -> bool;
}

/// Vanilla's `Structure::findValidGenerationPoint`. Impls own their RNG order,
/// collision checks, and biome check.
pub trait Structure: Send + Sync {
    /// `structure` carries registry data; per-set metadata stays in placement.
    /// `rng` is a fresh `LegacyRandom` seeded with `setLargeFeatureSeed`.
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub>;
}

impl<'ctx, 'src, N: DimensionNoises> GenerationContext<'ctx, 'src, N>
where
    'src: 'ctx,
{
    /// `getBaseHeight(WORLD_SURFACE_WG)` — aquifer-aware, scans from
    /// `preliminary_surface_level + 16`.
    ///
    /// `ocean_floor=false` → opaque is Solid+Fluid; `true` → opaque is Solid only.
    ///
    /// In dimensions with a constant `preliminary_surface_level` (End), use
    /// [`base_height_full`](Self::base_height_full) instead.
    pub fn base_height(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32 {
        self.ensure_height_cache_grid();
        let aq = self.aquifer.ensure(self.height_cache);
        column_base_height::<N>(self.height_cache, self.noises, aq, x, z, ocean_floor)
    }

    /// Full-column scan from chunk top. Matches vanilla's `iterateNoiseColumn`.
    pub fn base_height_full(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32 {
        self.ensure_height_cache_grid();
        let aq = self.aquifer.ensure(self.height_cache);
        iterate_noise_column_with_aquifer::<N>(
            self.height_cache,
            self.noises,
            aq,
            x,
            z,
            ocean_floor,
        )
    }

    /// Biome at a block position (quantized to quart).
    pub fn biome_at(&mut self, block_x: i32, block_y: i32, block_z: i32) -> BiomeRef {
        self.biome_sampler
            .sample(block_x >> 2, block_y >> 2, block_z >> 2)
    }

    /// Classify a single block in the base-noise column.
    pub fn column_state(&mut self, x: i32, y: i32, z: i32) -> ColumnBlock {
        self.ensure_height_cache_grid();
        let cw = N::Settings::CELL_WIDTH;
        let ch = N::Settings::CELL_HEIGHT;
        let density =
            column_interpolated_density::<N>(self.height_cache, self.noises, x, y, z, cw, ch);
        let aq = self.aquifer.ensure(self.height_cache);
        match aq.compute_substance(self.noises, x, y, z, density) {
            AquiferResult::Solid => ColumnBlock::Solid,
            AquiferResult::Fluid(_) => ColumnBlock::Fluid,
            AquiferResult::Air => ColumnBlock::Air,
        }
    }

    /// Surface Y at chunk center, memoised across per-structure contexts.
    pub fn surface_y(&mut self) -> i32 {
        if let Some(y) = *self.surface_y_cache {
            return y;
        }
        let y = self.base_height(self.center_block_x, self.center_block_z, false) - 1;
        *self.surface_y_cache = Some(y);
        y
    }

    fn ensure_height_cache_grid(&mut self) {
        if *self.height_cache_grid_ready {
            return;
        }
        self.height_cache
            .init_grid(self.chunk_min_x, self.chunk_min_z, self.noises);
        *self.height_cache_grid_ready = true;
    }
}

impl<N: DimensionNoises> StructureGenerationContext for GenerationContext<'_, '_, N> {
    fn seed(&self) -> i64 {
        self.seed
    }

    fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    fn chunk_min_x(&self) -> i32 {
        self.chunk_min_x
    }

    fn chunk_min_z(&self) -> i32 {
        self.chunk_min_z
    }

    fn center_block_x(&self) -> i32 {
        self.center_block_x
    }

    fn center_block_z(&self) -> i32 {
        self.center_block_z
    }

    fn sea_level(&self) -> i32 {
        self.sea_level
    }

    fn min_y(&self) -> i32 {
        N::Settings::MIN_Y
    }

    fn height(&self) -> i32 {
        N::Settings::HEIGHT
    }

    fn template_pools(&self) -> &FxHashMap<Identifier, TemplatePoolData> {
        self.template_pools
    }

    fn templates(&self) -> &FxHashMap<Identifier, TemplateData> {
        self.templates
    }

    fn base_height(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32 {
        GenerationContext::base_height(self, x, z, ocean_floor)
    }

    fn base_height_full(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32 {
        GenerationContext::base_height_full(self, x, z, ocean_floor)
    }

    fn biome_at(&mut self, block_x: i32, block_y: i32, block_z: i32) -> BiomeRef {
        GenerationContext::biome_at(self, block_x, block_y, block_z)
    }

    fn column_state(&mut self, x: i32, y: i32, z: i32) -> ColumnBlock {
        GenerationContext::column_state(self, x, y, z)
    }

    fn surface_y(&mut self) -> i32 {
        GenerationContext::surface_y(self)
    }

    fn terrain_surface_height(&self, x: i32, z: i32, ocean_floor: bool) -> i32 {
        if let Some(height) = self
            .terrain_height_cache
            .borrow()
            .get(&(x, z, ocean_floor))
            .copied()
        {
            return height;
        }

        let cell_w = N::Settings::CELL_WIDTH;
        let cell_x = x.div_euclid(cell_w) * cell_w;
        let cell_z = z.div_euclid(cell_w) * cell_w;
        let aq_chunk_x = (cell_x >> 4) * 16;
        let aq_chunk_z = (cell_z >> 4) * 16;
        let height = {
            let mut probes = self.terrain_probes.borrow_mut();
            let probe = probes.entry((aq_chunk_x, aq_chunk_z)).or_insert_with(|| {
                TerrainProbe::<N>::new(aq_chunk_x, aq_chunk_z, self.splitter, self.noises)
            });
            iterate_noise_column_with_aquifer::<N>(
                &mut probe.cache,
                self.noises,
                &mut probe.aquifer,
                x,
                z,
                ocean_floor,
            )
        };
        self.terrain_height_cache
            .borrow_mut()
            .insert((x, z, ocean_floor), height);
        height
    }

    fn terrain_is_opaque(&self, x: i32, y: i32, z: i32, ocean_floor: bool) -> bool {
        if let Some(opaque) = self
            .terrain_opaque_cache
            .borrow()
            .get(&(x, y, z, ocean_floor))
            .copied()
        {
            return opaque;
        }

        let cell_w = N::Settings::CELL_WIDTH;
        let cell_h = N::Settings::CELL_HEIGHT;
        let cell_x = x.div_euclid(cell_w) * cell_w;
        let cell_z = z.div_euclid(cell_w) * cell_w;
        let aq_chunk_x = (cell_x >> 4) * 16;
        let aq_chunk_z = (cell_z >> 4) * 16;
        let opaque = {
            let mut probes = self.terrain_probes.borrow_mut();
            let probe = probes.entry((aq_chunk_x, aq_chunk_z)).or_insert_with(|| {
                TerrainProbe::<N>::new(aq_chunk_x, aq_chunk_z, self.splitter, self.noises)
            });
            let density = column_interpolated_density::<N>(
                &mut probe.cache,
                self.noises,
                x,
                y,
                z,
                cell_w,
                cell_h,
            );
            match probe
                .aquifer
                .compute_substance(self.noises, x, y, z, density)
            {
                AquiferResult::Solid => true,
                AquiferResult::Fluid(_) => !ocean_floor,
                AquiferResult::Air => false,
            }
        };
        self.terrain_opaque_cache
            .borrow_mut()
            .insert((x, y, z, ocean_floor), opaque);
        opaque
    }
}
