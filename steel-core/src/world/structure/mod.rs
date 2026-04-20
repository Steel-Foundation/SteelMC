//! Structure start and reference types for chunk-level structure tracking.
//!
//! In vanilla, chunks store two maps:
//! - `structureStarts`: structures originating in this chunk
//! - `structuresReferences`: references to structures from nearby chunks
//!
//! The structure key is `Identifier` until a structure registry is added.

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

use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::random::RandomSplitter;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, ChunkPos, Direction, Identifier};

use steel_registry::biome::BiomeRef;
use steel_registry::template_pool::TemplateData;

use crate::chunk::aquifer::{AquiferResult, LazyAquifer};
use crate::chunk::vanilla_generator::{
    column_base_height, column_interpolated_density, iterate_noise_column_with_aquifer,
};
use crate::world::structure::placement::StructureSelectionEntry;
use crate::worldgen::ChunkBiomeSampler;

/// A structure start placed in a chunk.
///
/// Corresponds to vanilla's `StructureStart`. A start is "valid" if it has
/// at least one piece; invalid starts are not stored (they correspond to
/// vanilla's `INVALID_START` sentinel).
#[derive(Debug, Clone)]
pub struct StructureStart {
    /// The structure type identifier (e.g., `minecraft:village`).
    pub structure: Identifier,
    /// The chunk where this structure originates.
    pub chunk_pos: ChunkPos,
    /// How many neighboring chunks reference this start.
    pub references: i32,
    /// The pieces composing this structure.
    pub pieces: Vec<StructurePiece>,
    /// Bounding box inflation for reference intersection checks.
    /// Vanilla inflates by 12 when `terrain_adaptation != NONE`.
    pub bb_inflate: i32,
    /// Union of all pieces' bounding boxes, cached at creation. `None` when
    /// `pieces` is empty (invalid start emitted by legacy/unknown types).
    /// Reference scans do up to 17×17 neighbor lookups per target chunk, so
    /// an origin chunk containing a 30-piece jigsaw would otherwise rebuild
    /// this union per visiting neighbor.
    pub bounding_box: Option<BoundingBox>,
}

impl StructureStart {
    /// Create a new start, computing the piece-union bounding box up-front.
    #[must_use]
    pub fn new(
        structure: Identifier,
        chunk_pos: ChunkPos,
        pieces: Vec<StructurePiece>,
        bb_inflate: i32,
    ) -> Self {
        let bounding_box = Self::compute_bounding_box(&pieces);
        Self {
            structure,
            chunk_pos,
            references: 0,
            pieces,
            bb_inflate,
            bounding_box,
        }
    }

    /// Union of all pieces' bounding boxes, or `None` if empty.
    #[must_use]
    pub fn compute_bounding_box(pieces: &[StructurePiece]) -> Option<BoundingBox> {
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
        Some(bb)
    }
}

/// A single piece of a structure.
///
/// Corresponds to vanilla's `StructurePiece`. Type-specific data is stored
/// as an NBT blob since there are 56+ piece types in vanilla.
#[derive(Debug, Clone)]
pub struct StructurePiece {
    /// Piece type identifier (e.g., `minecraft:jigsaw`).
    pub piece_type: Identifier,
    /// World-space bounding box of this piece.
    pub bounding_box: BoundingBox,
    /// Generation depth (distance from start piece in the piece tree).
    pub gen_depth: i32,
    /// Horizontal orientation of this piece (`None` for unoriented pieces).
    /// Only horizontal directions (North/South/East/West) are used.
    pub orientation: Option<Direction>,
    /// Type-specific NBT data (simdnbt binary format).
    pub nbt_data: Vec<u8>,
    /// Ground level delta — offset from piece minY to "ground level".
    /// Used by Beardifier for terrain adaptation. Default 0 for non-jigsaw pieces.
    pub ground_level_delta: i32,
    /// Junctions connecting this piece to neighbors.
    /// Used by Beardifier for junction-based terrain adaptation.
    pub junctions: Vec<jigsaw::JigsawJunction>,
}

/// Map of structure starts keyed by structure identifier.
pub type StructureStartMap = FxHashMap<Identifier, StructureStart>;

/// Map of structure references keyed by structure identifier.
/// Values are the chunk positions of origin chunks that contain the structure start.
pub type StructureReferenceMap = FxHashMap<Identifier, Vec<ChunkPos>>;

/// Ternary classification of a block in the base-noise column (no surface rules).
/// Used by structures that walk the column to find valid placement positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnBlock {
    /// Empty space.
    Air,
    /// Aquifer-placed fluid (lava/water).
    Fluid,
    /// Default solid block (e.g. stone, netherrack, end stone).
    Solid,
}

/// Per-chunk context shared by every structure's `findGenerationPoint`.
///
/// Holds the mutable per-chunk state (biome sampler, height cache, aquifer)
/// so that individual structures don't each allocate their own. Methods
/// wrap the common operations — height queries, biome sampling, column
/// classification — so structures don't reach into `VanillaGenerator`'s
/// helpers directly.
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
    /// `chunk_x * 16` — convenience.
    pub chunk_min_x: i32,
    /// `chunk_z * 16` — convenience.
    pub chunk_min_z: i32,
    /// `chunk_min_x + 8`.
    pub center_block_x: i32,
    /// `chunk_min_z + 8`.
    pub center_block_z: i32,
    /// Sea level for this dimension.
    pub sea_level: i32,
    /// Shared memoisation slot for the chunk-center surface Y. First call to
    /// [`surface_y`](GenerationContext::surface_y) fills it; subsequent calls
    /// (including ones reached via a freshly rebuilt context for the next
    /// structure) reuse the cached value.
    pub(crate) surface_y_cache: &'ctx mut Option<i32>,
    /// Whether `height_cache`'s 5×5 quart grid has been populated. First
    /// access that benefits from the grid flips this and calls
    /// `init_grid`. Shared across per-structure contexts for the same chunk.
    pub(crate) height_cache_grid_ready: &'ctx mut bool,

    /// Dimension noise router (immutable — shared across all chunks).
    pub noises: &'src N,
    /// Positional splitter used to seed per-chunk RNG.
    pub splitter: &'src RandomSplitter,
    /// Structure templates (piece definitions + sizes).
    pub templates: &'src FxHashMap<Identifier, TemplateData>,

    /// Biome sampler scoped to this chunk.
    pub biome_sampler: &'ctx mut ChunkBiomeSampler<'src>,
    /// Column cache used by height/density queries (grid-initialized on construction).
    pub height_cache: &'ctx mut N::ColumnCache,
    /// Deferred aquifer. Built on first query; skipped entirely on chunks
    /// where no selected structure reads the aquifer.
    pub aquifer: &'ctx mut LazyAquifer<'src, N>,
}

/// Result of a successful `Structure::find_generation_point` call.
pub struct GenerationStub {
    /// World-space position the start anchors at. Used for the biome check
    /// (if the structure impl didn't do it itself) and for downstream code.
    pub position: (i32, i32, i32),
    /// The pieces, already sized and positioned in world space.
    pub pieces: Vec<StructurePiece>,
}

/// Common interface for every structure type. One impl per structure kind;
/// each is registered in the per-dimension `VanillaGenerator::structures`
/// registry.
///
/// Mirrors vanilla's `Structure::findValidGenerationPoint` — impls are
/// responsible for their own RNG-consumption order, collision checks, and
/// biome check. Returning `None` = no start placed for this chunk.
pub trait Structure<N: DimensionNoises>: Send + Sync {
    /// Bounding-box inflation for reference intersection. Vanilla inflates
    /// by 12 when `terrain_adaptation != NONE`.
    fn bb_inflate(&self) -> i32 {
        0
    }

    /// Runs the full `findValidGenerationPoint` flow. `entry` carries the
    /// per-set metadata (weight, allowed biomes, jigsaw config if any).
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
    /// `getBaseHeight(WORLD_SURFACE_WG)` — first free-height Y above terrain,
    /// aquifer-aware. Starts scan from `preliminary_surface_level + 16` using
    /// the cell-based iterator.
    ///
    /// `ocean_floor = false` → opaque = Solid or Fluid (`WORLD_SURFACE_WG`).
    /// `ocean_floor = true` → opaque = Solid only (`OCEAN_FLOOR_WG`).
    ///
    /// **Caveat:** in dimensions where `preliminary_surface_level` is a
    /// constant (e.g. End = 0.0), the cap will miss real terrain — use
    /// [`base_height_full`](Self::base_height_full) for those cases.
    pub fn base_height(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32 {
        self.ensure_height_cache_grid();
        let aq = self.aquifer.ensure(self.height_cache);
        column_base_height::<N>(
            self.height_cache,
            self.noises,
            aq,
            x,
            z,
            ocean_floor,
        )
    }

    /// Full-column variant of [`base_height`](Self::base_height) — scans from
    /// the chunk top without relying on `preliminary_surface_level`. Matches
    /// vanilla's `iterateNoiseColumn` exactly. Use for dimensions with an
    /// unreliable `preliminary_surface_level` (End).
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

    /// Biome at a block position. Quantizes to quart coords before sampling.
    pub fn biome_at(&mut self, block_x: i32, block_y: i32, block_z: i32) -> BiomeRef {
        self.biome_sampler
            .sample(block_x >> 2, block_y >> 2, block_z >> 2)
    }

    /// Classify a single block in the base-noise column (no surface rules).
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

    /// Surface Y at chunk center (`base_height(center, center, false) - 1`).
    /// Memoised — the first call pays for a column probe (and triggers aquifer
    /// construction), subsequent calls are free. The cache is shared across
    /// contexts for the same chunk via a borrowed slot.
    pub fn surface_y(&mut self) -> i32 {
        if let Some(y) = self.surface_y_cache.as_ref().copied() {
            return y;
        }
        let y = self.base_height(self.center_block_x, self.center_block_z, false) - 1;
        *self.surface_y_cache = Some(y);
        y
    }

    /// Populate `height_cache`'s 5×5 quart grid if not already done. Called
    /// by the cache-using query methods so chunks that don't reach them skip
    /// the ~25-position density-graph evaluation entirely.
    fn ensure_height_cache_grid(&mut self) {
        if *self.height_cache_grid_ready {
            return;
        }
        self.height_cache
            .init_grid(self.chunk_min_x, self.chunk_min_z, self.noises);
        *self.height_cache_grid_ready = true;
    }
}
