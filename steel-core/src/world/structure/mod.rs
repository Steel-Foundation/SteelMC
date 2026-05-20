//! Structure start/reference tracking.
//!
//! Vanilla keeps two per-chunk maps: `structureStarts` (originating here) and
//! `structuresReferences` (pointing at nearby origin chunks). The structure key
//! is `Identifier` until a structure registry is added.

pub mod desert_pyramid;
pub mod end_city;
pub mod fortress;
pub mod igloo;
pub mod jigsaw;
pub mod jungle_temple;
pub mod mansion;
pub mod mineshaft;
pub mod nether_fossil;
pub mod ocean_monument;
pub mod ocean_ruin;
mod piece;
pub mod placement;
pub mod ruined_portal;
pub mod shipwreck;
pub mod single_piece;
pub mod stronghold;
pub mod swamp_hut;

use std::{cell::RefCell, slice, vec};

use rustc_hash::FxHashMap;

use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{Random, RandomSplitter};
use steel_utils::{BlockPos, BoundingBox, ChunkPos, Direction, Identifier};
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};

use steel_registry::biome::BiomeRef;
use steel_registry::structure::{StructureData, TerrainAdjustment};
use steel_registry::template_pool::{TemplateData, TemplatePoolData};

use crate::worldgen::ChunkBiomeSampler;
use crate::worldgen::generators::vanilla::{
    column_base_height, column_interpolated_density, find_solid_block_below_air,
    iterate_noise_column_with_aquifer,
};
use crate::worldgen::noise::aquifer::{Aquifer, AquiferResult, LazyAquifer};

pub use piece::{
    ProceduralPieceData, RuinedPortalProperties, StructureBlockIgnore, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

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

/// Vanilla's `StructurePiece.makeBoundingBox`: north/south keep width/depth,
/// east/west swap them.
pub(crate) const fn make_oriented_piece_bounding_box(
    chunk_min_x: i32,
    y: i32,
    chunk_min_z: i32,
    orientation: Direction,
    width: i32,
    height: i32,
    depth: i32,
) -> BoundingBox {
    let z_axis = matches!(orientation, Direction::North | Direction::South);
    let (box_width, box_depth) = if z_axis {
        (width, depth)
    } else {
        (depth, width)
    };
    BoundingBox::new(
        chunk_min_x,
        y,
        chunk_min_z,
        chunk_min_x + box_width - 1,
        y + height - 1,
        chunk_min_z + box_depth - 1,
    )
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

/// Structure starts keyed by structure id.
pub type StructureStartMap = FxHashMap<Identifier, StructureStart>;

/// Structure references → origin chunk positions.
///
/// Vanilla stores these as a fastutil `LongOpenHashSet`, so duplicates are
/// ignored and feature-stage iteration follows that table order.
pub type StructureReferenceMap = FxHashMap<Identifier, StructureReferenceSet>;

/// Set of structure-start chunk positions with vanilla iteration order.
///
/// Reference generation discovers sources in a stable scan order, but vanilla
/// stores the packed chunk longs in fastutil's `LongOpenHashSet`. Feature-stage
/// placement consumes the set through that table iteration order, so Steel keeps
/// the insertion order for persistence and exposes the vanilla iteration order
/// for worldgen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructureReferenceSet {
    insertion_order: Vec<ChunkPos>,
    iteration_order: Vec<ChunkPos>,
}

impl StructureReferenceSet {
    /// Inserts a chunk position if it was not already present.
    pub fn insert(&mut self, pos: ChunkPos) -> bool {
        if self.insertion_order.contains(&pos) {
            return false;
        }
        self.insertion_order.push(pos);
        self.rebuild_iteration_order();
        true
    }

    /// Extends this set with insertion-order duplicate removal.
    pub fn extend(&mut self, positions: impl IntoIterator<Item = ChunkPos>) {
        for pos in positions {
            self.insert(pos);
        }
    }

    /// Returns an iterator over positions in vanilla `LongOpenHashSet` order.
    pub fn iter(&self) -> slice::Iter<'_, ChunkPos> {
        self.iteration_order.iter()
    }

    /// Returns an iterator over positions in discovery order.
    pub fn insertion_order_iter(&self) -> slice::Iter<'_, ChunkPos> {
        self.insertion_order.iter()
    }

    /// Returns `true` when no positions are stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.insertion_order.is_empty()
    }

    fn rebuild_iteration_order(&mut self) {
        self.iteration_order = Self::vanilla_long_open_hash_set_order(&self.insertion_order);
    }

    fn vanilla_long_open_hash_set_order(insertion_order: &[ChunkPos]) -> Vec<ChunkPos> {
        let Some(table_size) = Self::vanilla_long_open_hash_set_table_size(insertion_order.len())
        else {
            return Vec::new();
        };
        let mask = (table_size - 1) as u64;
        let mut table = vec![None; table_size];
        let mut zero_key = None;

        for &pos in insertion_order {
            let packed = Self::pack_chunk_pos(pos);
            if packed == 0 {
                zero_key = Some(pos);
                continue;
            }

            let mut slot = (Self::fastutil_mix(packed) & mask) as usize;
            loop {
                if table[slot].is_none() {
                    table[slot] = Some(pos);
                    break;
                }
                slot = (slot + 1) & (table_size - 1);
            }
        }

        let mut ordered = Vec::with_capacity(insertion_order.len());
        if let Some(pos) = zero_key {
            ordered.push(pos);
        }
        for slot in (0..table_size).rev() {
            if let Some(pos) = table[slot] {
                ordered.push(pos);
            }
        }
        ordered
    }

    fn vanilla_long_open_hash_set_table_size(len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }

        let mut table_size = Self::fastutil_array_size(16);
        let mut max_fill = Self::fastutil_max_fill(table_size);
        let mut size = 0;
        for _ in 0..len {
            let old_size = size;
            size += 1;
            if old_size >= max_fill {
                table_size = Self::fastutil_array_size(size + 1);
                max_fill = Self::fastutil_max_fill(table_size);
            }
        }
        Some(table_size)
    }

    fn fastutil_array_size(expected: usize) -> usize {
        let needed = ((expected as f64) / 0.75).ceil() as usize;
        needed.max(2).next_power_of_two()
    }

    const fn fastutil_max_fill(table_size: usize) -> usize {
        let fill = table_size - table_size / 4;
        if fill < table_size {
            fill
        } else {
            table_size - 1
        }
    }

    const fn pack_chunk_pos(pos: ChunkPos) -> u64 {
        (pos.0.x as u32 as u64) | ((pos.0.y as u32 as u64) << 32)
    }

    const fn fastutil_mix(value: u64) -> u64 {
        let mixed = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mixed = mixed ^ (mixed >> 32);
        mixed ^ (mixed >> 16)
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
    type IntoIter = vec::IntoIter<ChunkPos>;
    type Item = ChunkPos;

    fn into_iter(self) -> Self::IntoIter {
        self.iteration_order.into_iter()
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
    /// Highest solid base-terrain block directly below air in `[min_solid_y, start_y)`.
    fn solid_block_below_air(
        &mut self,
        x: i32,
        z: i32,
        start_y: i32,
        min_solid_y: i32,
    ) -> Option<i32> {
        if start_y <= min_solid_y {
            return None;
        }

        let mut above = self.column_state(x, start_y, z);
        for y in (min_solid_y..start_y).rev() {
            let current = self.column_state(x, y, z);
            if above == ColumnBlock::Air && current == ColumnBlock::Solid {
                return Some(y);
            }
            above = current;
        }
        None
    }
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

    /// Highest solid base-terrain block directly below air in `[min_solid_y, start_y)`.
    pub fn solid_block_below_air(
        &mut self,
        x: i32,
        z: i32,
        start_y: i32,
        min_solid_y: i32,
    ) -> Option<i32> {
        self.ensure_height_cache_grid();
        let aq = self.aquifer.ensure(self.height_cache);
        find_solid_block_below_air::<N>(
            self.height_cache,
            self.noises,
            aq,
            x,
            z,
            start_y,
            min_solid_y,
        )
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

    fn solid_block_below_air(
        &mut self,
        x: i32,
        z: i32,
        start_y: i32,
        min_solid_y: i32,
    ) -> Option<i32> {
        GenerationContext::solid_block_below_air(self, x, z, start_y, min_solid_y)
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
