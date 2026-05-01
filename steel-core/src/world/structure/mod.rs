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

use rustc_hash::FxHashMap;

use steel_utils::random::RandomSplitter;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, ChunkPos, Direction, Identifier};
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};

use steel_registry::biome::BiomeRef;
use steel_registry::template_pool::TemplateData;

use crate::world::structure::placement::StructureSelectionEntry;
use crate::worldgen::ChunkBiomeSampler;
use crate::worldgen::generators::vanilla::{
    column_base_height, column_interpolated_density, iterate_noise_column_with_aquifer,
};
use crate::worldgen::noise::aquifer::{AquiferResult, LazyAquifer};

/// A structure start placed in a chunk. Vanilla's `StructureStart` — invalid (empty)
/// starts are not stored.
#[derive(Debug, Clone)]
pub struct StructureStart {
    /// Structure id (e.g., `minecraft:village`).
    pub structure: Identifier,
    /// Origin chunk.
    pub chunk_pos: ChunkPos,
    /// Reference count from neighboring chunks.
    pub references: i32,
    /// Pieces composing this structure.
    pub pieces: Vec<StructurePiece>,
    /// Bounding-box inflation applied at construction. Vanilla inflates by 12
    /// when `terrain_adaptation != NONE`. Stored for serialization parity; the
    /// inflation is already baked into [`bounding_box`](Self::bounding_box).
    pub bb_inflate: i32,
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
        bb_inflate: i32,
    ) -> Self {
        let bounding_box = Self::compute_bounding_box(&pieces, bb_inflate);
        Self {
            structure,
            chunk_pos,
            references: 0,
            pieces,
            bb_inflate,
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
}

/// Vanilla's `StructurePiece`. Type-specific data is stored as an NBT blob
/// (56+ piece types in vanilla).
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
    /// Type-specific NBT (simdnbt binary).
    pub nbt_data: Vec<u8>,
    /// Offset from piece minY to ground level. Used by Beardifier. Default 0 for non-jigsaw.
    pub ground_level_delta: i32,
    /// Junctions for Beardifier terrain adaptation.
    pub junctions: Vec<jigsaw::JigsawJunction>,
}

/// Structure starts keyed by structure id.
pub type StructureStartMap = FxHashMap<Identifier, StructureStart>;

/// Structure references → origin chunk positions.
pub type StructureReferenceMap = FxHashMap<Identifier, Vec<ChunkPos>>;

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
    /// Structure templates (piece definitions + sizes).
    pub templates: &'src FxHashMap<Identifier, TemplateData>,

    /// Biome sampler scoped to this chunk.
    pub biome_sampler: &'ctx mut ChunkBiomeSampler<'src>,
    /// Column cache for height/density queries (grid-initialized on demand).
    pub height_cache: &'ctx mut N::ColumnCache,
    /// Aquifer built on first query; skipped on chunks where no structure needs it.
    pub aquifer: &'ctx mut LazyAquifer<'src, N>,
}

/// Result of a successful `Structure::find_generation_point`.
pub struct GenerationStub {
    /// World-space position the start anchors at.
    pub position: (i32, i32, i32),
    /// Pieces already sized and positioned in world space.
    pub pieces: Vec<StructurePiece>,
}

/// Vanilla's `Structure::findValidGenerationPoint`. Impls own their RNG order,
/// collision checks, and biome check.
pub trait Structure<N: DimensionNoises>: Send + Sync {
    /// Bounding-box inflation for reference intersection. Vanilla uses 12 when
    /// `terrain_adaptation != NONE`.
    fn bb_inflate(&self) -> i32 {
        0
    }

    /// `entry` carries per-set metadata (weight, biomes, jigsaw config).
    /// `rng` is a fresh `LegacyRandom` seeded with `setLargeFeatureSeed`.
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
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
