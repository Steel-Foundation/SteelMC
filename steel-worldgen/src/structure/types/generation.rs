use crate::biomes::ChunkBiomeSampler;
use crate::density::traits::{ColumnCache, NoiseSettings};
use crate::noise::AquiferResult;
use crate::noise::LazyAquifer;
use crate::structure::StructurePiece;
use crate::structure::types::ColumnBlock;
use crate::utils::column_base_height;
use crate::utils::column_interpolated_density;
use crate::utils::find_solid_block_below_air;
use crate::utils::iterate_noise_column_with_aquifer;
use crate::{density::DimensionNoises, noise::Aquifer};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use steel_registry::biome::BiomeRef;
use steel_registry::template_pool::{TemplateData, TemplatePoolData};
use steel_utils::Identifier;
use steel_utils::random::RandomSplitter;

/// Per-chunk context shared by every structure's `findGenerationPoint`.
///
/// Holds mutable per-chunk state (biome sampler, height cache, aquifer) so structures
/// don't each allocate their own. Wraps `VanillaGenerator`'s helpers.
pub struct GenerationContext<'ctx, 'src, N: DimensionNoises>
where
    'src: 'ctx,
{
    /// World seed.
    seed: i64,
    /// Chunk being populated.
    chunk_x: i32,
    /// Chunk being populated.
    chunk_z: i32,
    /// `chunk_x * 16`.
    chunk_min_x: i32,
    /// `chunk_z * 16`.
    chunk_min_z: i32,
    /// `chunk_min_x + 8`.
    center_block_x: i32,
    /// `chunk_min_z + 8`.
    center_block_z: i32,
    /// Sea level for this dimension.
    sea_level: i32,
    /// Shared memoisation slot for the chunk-center surface Y.
    surface_y_cache: &'ctx mut Option<i32>,
    /// Whether `height_cache`'s 5×5 quart grid has been populated. Shared across
    /// per-structure contexts in the same chunk.
    height_cache_grid_ready: &'ctx mut bool,

    /// Dimension noise router.
    noises: &'src N,
    /// Positional splitter for per-chunk RNG.
    splitter: &'src RandomSplitter,
    /// Template pool registry for jigsaw assembly.
    template_pools: &'src FxHashMap<Identifier, TemplatePoolData>,
    /// Template data registry for jigsaw assembly.
    templates: &'src FxHashMap<Identifier, TemplateData>,

    /// Biome sampler scoped to this chunk.
    biome_sampler: &'ctx mut ChunkBiomeSampler<'src>,
    /// Column cache for height/density queries (grid-initialized on demand).
    height_cache: &'ctx mut N::ColumnCache,
    /// Aquifer built on first query; skipped on chunks where no structure needs it.
    aquifer: &'ctx mut LazyAquifer<'src, N>,
    /// Cache for terrain height checks.
    terrain_height_cache: RefCell<FxHashMap<(i32, i32, bool), i32>>,
    /// Cache for terrain opacity checks.
    terrain_opaque_cache: RefCell<FxHashMap<(i32, i32, i32, bool), bool>>,
    /// Probes for off-chunk height/opaque checks.
    terrain_probes: RefCell<FxHashMap<(i32, i32), TerrainProbe<N>>>,
}

/// An off-chunk height and opacity probe.
pub struct TerrainProbe<N: DimensionNoises> {
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

impl<'ctx, 'src, N: DimensionNoises> GenerationContext<'ctx, 'src, N>
where
    'src: 'ctx,
{
    /// Creates a per-chunk structure generation context.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "borrows all mutable per-chunk generation state without owning it"
    )]
    pub fn new(
        seed: i64,
        chunk_x: i32,
        chunk_z: i32,
        sea_level: i32,
        noises: &'src N,
        splitter: &'src RandomSplitter,
        template_pools: &'src FxHashMap<Identifier, TemplatePoolData>,
        templates: &'src FxHashMap<Identifier, TemplateData>,
        biome_sampler: &'ctx mut ChunkBiomeSampler<'src>,
        height_cache: &'ctx mut N::ColumnCache,
        aquifer: &'ctx mut LazyAquifer<'src, N>,
        surface_y_cache: &'ctx mut Option<i32>,
        height_cache_grid_ready: &'ctx mut bool,
    ) -> Self {
        let chunk_min_x = chunk_x * 16;
        let chunk_min_z = chunk_z * 16;
        Self {
            seed,
            chunk_x,
            chunk_z,
            chunk_min_x,
            chunk_min_z,
            center_block_x: chunk_min_x + 8,
            center_block_z: chunk_min_z + 8,
            sea_level,
            surface_y_cache,
            height_cache_grid_ready,
            noises,
            splitter,
            template_pools,
            templates,
            biome_sampler,
            height_cache,
            aquifer,
            terrain_height_cache: RefCell::default(),
            terrain_opaque_cache: RefCell::default(),
            terrain_probes: RefCell::default(),
        }
    }

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
