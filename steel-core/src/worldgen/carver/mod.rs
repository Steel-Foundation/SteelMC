//! World-carving: runtime types for running configured carvers during the
//! `CARVERS` chunk stage.
//!
//! Mirrors vanilla's `net.minecraft.world.level.levelgen.carver` package. The
//! [`CarvingContext`] bundles the dimension-level state; a [`CarveRun`]
//! bundles the per-chunk references that every carver method threads
//! through.

use std::{cell::Cell, sync::LazyLock};

use glam::IVec3;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use steel_math::lerp2;
use steel_math::trig;
use steel_registry::REGISTRY;
use steel_registry::biome::BiomeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::ChunkPos;
use steel_utils::{BlockPos, BlockStateId, Identifier, types::UpdateFlags};
use steel_worldgen::density::DimensionNoises;
use steel_worldgen::surface::{SurfaceConditionNoiseCache, SurfaceRuleContext};

use crate::behavior::BlockStateBehaviorExt as _;
use crate::chunk::{
    chunk_access::ChunkAccess,
    heightmap::{Heightmap, HeightmapType},
};
use crate::worldgen::carving_mask::CarvingMask;
use crate::worldgen::surface::SurfaceSystem;
use steel_worldgen::noise::{Aquifer, AquiferResult};

pub mod canyon;
pub mod cave;

/// The four preliminary-surface-level samples at a chunk's block corners, in
/// world Y. Indexed by local `(x, z)` corner as `(0,0)`, `(16,0)`, `(0,16)`,
/// `(16,16)`.
#[derive(Debug, Clone, Copy)]
pub struct PreliminarySurfaceCorners {
    /// Corner at `(chunk_min_x, chunk_min_z)`.
    pub nw: i32,
    /// Corner at `(chunk_min_x + 16, chunk_min_z)`.
    pub ne: i32,
    /// Corner at `(chunk_min_x, chunk_min_z + 16)`.
    pub sw: i32,
    /// Corner at `(chunk_min_x + 16, chunk_min_z + 16)`.
    pub se: i32,
}

/// A source chunk's position and carver-list biome — the unit of work in the
/// 17×17 `apply_carvers` loop. Each entry feeds one or more carver
/// invocations from the biome's `carvers` list.
#[derive(Debug, Clone, Copy)]
pub struct SourceChunk {
    /// Chunk position of the carver's origin.
    pub pos: ChunkPos,
    /// Biome providing the source chunk's carver list.
    pub biome: BiomeRef,
}

/// Runtime context for a single `apply_carvers` invocation on one chunk.
///
/// Mirrors vanilla's `CarvingContext`. Owns the freshly-built [`Aquifer`] for
/// this chunk; the aquifer is regenerated per carver invocation rather than
/// cached on the [`ProtoChunk`] — see the TODO on `ProtoChunk::carving_mask`
/// for discussion.
pub struct CarvingContext<'a, N: DimensionNoises> {
    /// Dimension minimum Y (inclusive).
    pub min_y: i32,
    /// Dimension vertical extent in blocks (`max_y = min_y + gen_depth - 1`).
    pub gen_depth: i32,
    /// Surface system (biome-specific surface noise + clay bands).
    pub surface_system: &'a SurfaceSystem,
    /// Owned aquifer for this chunk. Built fresh from the dimension's noises
    /// at the start of `apply_carvers`.
    pub aquifer: Aquifer<N>,
    /// Default solid block for this dimension (stone / netherrack /
    /// `end_stone`).
    pub default_block_id: BlockStateId,
    /// Preliminary surface levels at the 4 corners of this chunk, used for
    /// bilinear interpolation of `min_surface_level` during top-material
    /// lookup.
    pub psl_corners: PreliminarySurfaceCorners,
    /// Chunk NW block X — anchors `psl_corners`.
    pub chunk_min_x: i32,
    /// Chunk NW block Z — anchors `psl_corners`.
    pub chunk_min_z: i32,
}

impl<N: DimensionNoises> CarvingContext<'_, N> {
    /// Bilinear interpolation of the 4 preliminary-surface-level corners at
    /// the given in-chunk position. Matches vanilla's
    /// `SurfaceRules.Context.updateXZ` path.
    #[must_use]
    pub fn min_surface_level(&self, block_x: i32, block_z: i32) -> i32 {
        let local_x = (block_x - self.chunk_min_x).clamp(0, 16);
        let local_z = (block_z - self.chunk_min_z).clamp(0, 16);
        // Vanilla: (float)(blockX & 15) / 16.0F — float intermediate is exact for 0-15
        let t_x = f64::from(local_x as u8) / 16.0;
        let t_z = f64::from(local_z as u8) / 16.0;
        let c = self.psl_corners;
        let interp = lerp2(
            t_x,
            t_z,
            f64::from(c.nw),
            f64::from(c.ne),
            f64::from(c.sw),
            f64::from(c.se),
        );
        interp.floor() as i32
    }

    /// Runs surface rules at a single position to pick the "top material"
    /// block (grass / podzol / mycelium / sand / ...). Called by the carver
    /// when it uncovers dirt beneath a grass block so the exposed surface gets
    /// rewritten to the biome-appropriate surface block.
    ///
    /// Mirrors vanilla's `SurfaceSystem.topMaterial` (the `@Deprecated`
    /// carver-specific variant). Vanilla hardcodes
    /// `stone_depth_above = stone_depth_below = 1` here, and the water height
    /// depends on whether the carved block was replaced with a fluid.
    pub fn top_material(
        &self,
        biome_id: u16,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        steep: bool,
        under_fluid: bool,
    ) -> Option<BlockStateId> {
        // Surface noise inputs (same helpers build_surface uses per column).
        let surface_depth = self.surface_system.get_surface_depth(block_x, block_z);
        let surface_secondary = self.surface_system.get_surface_secondary(block_x, block_z);
        let min_surface_level = self.min_surface_level(block_x, block_z) + surface_depth - 8;

        let water_height = if under_fluid { block_y + 1 } else { i32::MIN };
        let condition_noise_values: SmallVec<[Cell<f64>; 8]> = N::surface_noise_ids()
            .iter()
            .map(|_| Cell::new(0.0))
            .collect();
        let condition_noise_initialized: SmallVec<[Cell<bool>; 8]> = N::surface_noise_ids()
            .iter()
            .map(|_| Cell::new(false))
            .collect();
        let condition_noise_cache =
            SurfaceConditionNoiseCache::new(&condition_noise_values, &condition_noise_initialized);

        let mut ctx = SurfaceRuleContext::new(
            block_x,
            block_z,
            surface_depth,
            surface_secondary,
            min_surface_level,
            steep,
            block_y,
            1,
            1,
            water_height,
            Some(biome_id),
            None,
            self.surface_system,
            &condition_noise_cache,
            N::surface_rule_block_states(),
        );

        N::try_apply_surface_rule(&mut ctx)
    }
}

/// Vanilla's `WorldCarver.canReplaceBlock`: a carver may only replace blocks
/// in its config's `replaceable` tag.
#[must_use]
pub fn can_replace_block(state: BlockStateId, tag: &Identifier) -> bool {
    if state.is_air() {
        return false;
    }
    let Some(block) = REGISTRY.blocks.by_state_id(state) else {
        return false;
    };
    block.has_tag(tag)
}

/// Per-state membership cache for a carver's replaceable block tag.
///
/// Vanilla tests a block-state predicate for every candidate block. Steel's
/// registry stores tags by block key, so resolving that predicate once into a
/// state-id table avoids repeated tag/hash lookups in the carve loop while
/// preserving the configured tag as the source of truth.
#[derive(Debug)]
pub struct CarverReplaceableStates {
    states: Box<[bool]>,
}

impl CarverReplaceableStates {
    fn build(tag: &Identifier) -> Self {
        let states = REGISTRY
            .blocks
            .state_to_block_lookup
            .iter()
            .map(|&block| block.has_tag(tag))
            .collect();
        Self { states }
    }

    /// Returns whether `state` belongs to this cached replaceable set.
    #[inline]
    #[must_use]
    pub fn contains(&self, state: BlockStateId) -> bool {
        self.states.get(state.0 as usize).copied().unwrap_or(false)
    }
}

static CARVER_REPLACEABLE_STATES: LazyLock<FxHashMap<Identifier, CarverReplaceableStates>> =
    LazyLock::new(|| {
        let mut states_by_tag = FxHashMap::default();
        for (_, carver) in REGISTRY.configured_carvers.iter() {
            let tag = &carver.base().replaceable_tag;
            if !states_by_tag.contains_key(tag) {
                states_by_tag.insert(tag.clone(), CarverReplaceableStates::build(tag));
            }
        }
        states_by_tag
    });

/// Returns the cached replaceable-state set for a configured carver tag.
#[must_use]
pub fn cached_replaceable_states(tag: &Identifier) -> Option<&'static CarverReplaceableStates> {
    CARVER_REPLACEABLE_STATES.get(tag)
}

/// Which carver family dictates the per-block decision inside
/// [`CarveRun::carve_ellipsoid`]. Overworld carvers (cave + canyon) use the
/// aquifer to pick air / water / lava; the nether variant hardcodes lava
/// below `min_gen_y + 31` and cave-air elsewhere, with no aquifer lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarverStyle {
    /// Overworld / end: aquifer-driven fluid/air.
    Overworld,
    /// Nether: lava below `min_gen_y + 31` else `CAVE_AIR`; no aquifer check.
    Nether,
}

/// Well-known block state IDs a carver needs. Cached once per `apply_carvers`
/// call so the carver loop doesn't hit the registry in its hot path.
#[derive(Debug, Clone, Copy)]
pub struct CarverBlockIds {
    /// `minecraft:air`.
    pub air: BlockStateId,
    /// `minecraft:cave_air` (used by the nether carver).
    pub cave_air: BlockStateId,
    /// `minecraft:lava` (fluid block state).
    pub lava: BlockStateId,
    /// `minecraft:grass_block` default state.
    pub grass_block: BlockStateId,
    /// `minecraft:mycelium` default state.
    pub mycelium: BlockStateId,
    /// `minecraft:dirt` default state.
    pub dirt: BlockStateId,
}

impl CarverBlockIds {
    /// Looks up the well-known block state IDs once from the registry.
    #[must_use]
    pub fn load() -> Self {
        static IDS: LazyLock<CarverBlockIds> = LazyLock::new(CarverBlockIds::load_uncached);
        *IDS
    }

    fn load_uncached() -> Self {
        use steel_registry::{REGISTRY, vanilla_blocks};
        Self {
            air: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR),
            cave_air: REGISTRY
                .blocks
                .get_default_state_id(&vanilla_blocks::CAVE_AIR),
            lava: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::LAVA),
            grass_block: REGISTRY
                .blocks
                .get_default_state_id(&vanilla_blocks::GRASS_BLOCK),
            mycelium: REGISTRY
                .blocks
                .get_default_state_id(&vanilla_blocks::MYCELIUM),
            dirt: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::DIRT),
        }
    }

    /// Returns whether the given state is one of the air variants this
    /// carver uses (i.e. not a fluid). Used by the top-material flow to
    /// decide `under_fluid`.
    #[must_use]
    pub const fn is_air_like(&self, state: BlockStateId) -> bool {
        // SAFETY: BlockStateId is a `#[repr(transparent)]` wrapper around u16.
        // Hand-written equality keeps this function `const`.
        state.0 == self.air.0 || state.0 == self.cave_air.0
    }
}

/// Predicate called inside the carver's Y scan to decide whether a block is
/// outside the carved shape for a given ellipsoid (cave floor cutoff, canyon
/// width-by-height, etc). Matches vanilla's `WorldCarver.CarveSkipChecker`.
pub trait CarveSkipChecker {
    /// `xd`, `yd`, `zd` are the ellipsoid-normalized offsets from the carver
    /// origin to this block's center (see `CarveRun::carve_ellipsoid`);
    /// `world_y` is the absolute Y coordinate of the current block.
    fn should_skip(&mut self, xd: f64, yd: f64, zd: f64, world_y: i32) -> bool;
}

impl<F: FnMut(f64, f64, f64, i32) -> bool> CarveSkipChecker for F {
    fn should_skip(&mut self, xd: f64, yd: f64, zd: f64, world_y: i32) -> bool {
        self(xd, yd, zd, world_y)
    }
}

/// Per-carver parameters: the replaceable-tag, resolved lava level, and
/// which carver style to dispatch. Block IDs live on [`CarveRun`] because
/// they're shared across all carvers in a chunk.
pub struct CarveParams<'a> {
    /// Tag of blocks the carver is allowed to replace.
    pub replaceable_tag: &'a Identifier,
    /// Cached state-id membership for `replaceable_tag` when available.
    pub replaceable_states: Option<&'static CarverReplaceableStates>,
    /// Resolved lava level (world Y). At or below this, carved blocks become
    /// lava instead of air/water/etc.
    pub lava_level_y: i32,
    /// Which carver family this is (overworld vs nether).
    pub style: CarverStyle,
}

/// Vanilla cave/canyon tunnel radius calculation.
#[inline]
#[must_use]
pub(super) fn horizontal_tunnel_radius(progress_arg: f32, thickness: f32) -> f64 {
    let radius_offset = trig::sin(f64::from(progress_arg)) * thickness;
    1.5 + f64::from(radius_offset)
}

/// Decision returned by the per-block carve-state computation.
enum CarveState {
    /// Place this block.
    Place(BlockStateId),
    /// Aquifer barrier / "don't carve" — skip block.
    Skip,
}

/// The references every carver method needs. Bundled so `carve_ellipsoid`,
/// `carve_block`, `create_tunnel`, `create_room`, `carve_cave`,
/// `carve_canyon`, and `do_carve` can all be `&mut self` methods instead of
/// repeating the same 7–8 arguments.
pub struct CarveRun<'a, 'b, N, F>
where
    N: DimensionNoises,
    F: FnMut(BlockPos) -> u16,
{
    /// Dimension-level context (aquifer, surface system, bounds, psl).
    pub ctx: &'a mut CarvingContext<'b, N>,
    /// Noise generators for this dimension.
    pub noises: &'a N,
    /// Chunk being carved into.
    pub chunk: &'a ChunkAccess,
    /// Chunk NW block X (cached; `ctx.chunk_min_x` mirrors this).
    pub chunk_min_x: i32,
    /// Chunk NW block Z (cached; `ctx.chunk_min_z` mirrors this).
    pub chunk_min_z: i32,
    /// Biome lookup (vanilla `BiomeManager.getBiome`-style, fuzzed).
    pub biome_getter: &'a mut F,
    /// Carving mask for the chunk (lazily created on the proto chunk).
    pub mask: &'a mut CarvingMask,
    /// Block IDs cached once per carver session.
    pub ids: CarverBlockIds,
}

impl<N, F> CarveRun<'_, '_, N, F>
where
    N: DimensionNoises,
    F: FnMut(BlockPos) -> u16,
{
    /// Carve every block inside the given ellipsoid that falls in this chunk.
    /// Mirrors vanilla's `WorldCarver.carveEllipsoid`.
    ///
    /// Returns `true` if at least one block was carved.
    #[expect(
        clippy::similar_names,
        reason = "min_x_idx / min_z_idx / max_x_idx / max_z_idx mirror vanilla"
    )]
    #[expect(
        clippy::too_many_arguments,
        reason = "params + x/y/z/horizontal_radius/vertical_radius + skip_checker mirrors vanilla"
    )]
    pub fn carve_ellipsoid<S: CarveSkipChecker>(
        &mut self,
        params: &CarveParams<'_>,
        x: f64,
        y: f64,
        z: f64,
        horizontal_radius: f64,
        vertical_radius: f64,
        mut skip_checker: S,
    ) -> bool {
        let middle_x = f64::from(self.chunk_min_x) + 8.0;
        let middle_z = f64::from(self.chunk_min_z) + 8.0;
        let max_delta = 16.0 + horizontal_radius * 2.0;
        if (x - middle_x).abs() > max_delta || (z - middle_z).abs() > max_delta {
            return false;
        }

        let min_x_idx = ((x - horizontal_radius).floor() as i32 - self.chunk_min_x - 1).max(0);
        let max_x_idx = ((x + horizontal_radius).floor() as i32 - self.chunk_min_x).min(15);
        let min_y = ((y - vertical_radius).floor() as i32 - 1).max(self.ctx.min_y + 1);
        // Vanilla: `chunk.isUpgrading() ? 0 : 7`. No chunk upgrade path yet,
        // so always 7 — matches extractor config.
        let protected_blocks_on_top = 7;
        let max_y = ((y + vertical_radius).floor() as i32 + 1)
            .min(self.ctx.min_y + self.ctx.gen_depth - 1 - protected_blocks_on_top);
        let min_z_idx = ((z - horizontal_radius).floor() as i32 - self.chunk_min_z - 1).max(0);
        let max_z_idx = ((z + horizontal_radius).floor() as i32 - self.chunk_min_z).min(15);

        let mut carved = false;

        for x_idx in min_x_idx..=max_x_idx {
            let world_x = self.chunk_min_x + x_idx;
            let xd = (f64::from(world_x) + 0.5 - x) / horizontal_radius;

            for z_idx in min_z_idx..=max_z_idx {
                let world_z = self.chunk_min_z + z_idx;
                let zd = (f64::from(world_z) + 0.5 - z) / horizontal_radius;
                if xd * xd + zd * zd >= 1.0 {
                    continue;
                }

                let mut has_grass = false;

                // Scan top-down; range is exclusive of min_y (matches vanilla's
                // `worldY > minY`).
                for world_y in (min_y + 1..=max_y).rev() {
                    let yd = (f64::from(world_y) - 0.5 - y) / vertical_radius;
                    if skip_checker.should_skip(xd, yd, zd, world_y) {
                        continue;
                    }
                    if !self.mask.set_if_unset(x_idx, world_y, z_idx) {
                        continue;
                    }
                    if self.carve_block(params, world_x, world_y, world_z, &mut has_grass) {
                        carved = true;
                    }
                }
            }
        }

        carved
    }

    /// Per-block carve decision + placement. Mirrors vanilla's
    /// `WorldCarver.carveBlock` (and the `NetherWorldCarver` override).
    fn carve_block(
        &mut self,
        params: &CarveParams<'_>,
        world_x: i32,
        world_y: i32,
        world_z: i32,
        has_grass: &mut bool,
    ) -> bool {
        let pos = BlockPos::new(world_x, world_y, world_z);
        let existing = self.chunk.get_block_state(pos);

        // Track grass/mycelium for the top-material rewrite later.
        if existing == self.ids.grass_block || existing == self.ids.mycelium {
            *has_grass = true;
        }

        if !Self::can_replace(params, existing) {
            return false;
        }

        let state = match self.get_carve_state(params, world_x, world_y, world_z) {
            CarveState::Place(id) => id,
            CarveState::Skip => return false,
        };

        self.chunk.set_block_state(pos, state, UpdateFlags::empty());
        if params.style == CarverStyle::Overworld
            && self.ctx.aquifer.should_schedule_fluid_update()
            && state.has_fluid()
        {
            self.chunk.mark_pos_for_postprocessing(pos);
        }

        // Top-material rewrite: only when we just turned a grass/mycelium
        // block into something carved, and the block directly below is plain
        // dirt. Nether carver skips this entirely (its override of carveBlock
        // doesn't run this branch).
        if params.style == CarverStyle::Overworld && *has_grass {
            let below_pos = BlockPos::new(world_x, world_y - 1, world_z);
            if self.chunk.get_block_state(below_pos) == self.ids.dirt {
                let under_fluid = !self.ids.is_air_like(state);
                let steep = self.steep_material_condition(world_x, world_z);
                let biome_id =
                    (self.biome_getter)(BlockPos(IVec3::new(world_x, world_y - 1, world_z)));
                if let Some(top) = self.ctx.top_material(
                    biome_id,
                    world_x,
                    world_y - 1,
                    world_z,
                    steep,
                    under_fluid,
                ) {
                    self.chunk
                        .set_block_state(below_pos, top, UpdateFlags::empty());
                    if top.has_fluid() {
                        self.chunk.mark_pos_for_postprocessing(below_pos);
                    }
                }
            }
        }

        true
    }

    #[inline]
    fn can_replace(params: &CarveParams<'_>, state: BlockStateId) -> bool {
        if state.is_air() {
            return false;
        }
        if let Some(states) = params.replaceable_states {
            return states.contains(state);
        }
        can_replace_block(state, params.replaceable_tag)
    }

    fn steep_material_condition(&self, world_x: i32, world_z: i32) -> bool {
        let heightmaps = self.chunk.proto_heightmaps();
        if let Some(worldgen_surface) = heightmaps.get(HeightmapType::WorldSurfaceWg) {
            return steep_material_condition(worldgen_surface, world_x, world_z);
        }
        drop(heightmaps);

        self.chunk
            .prime_heightmaps(&[HeightmapType::WorldSurfaceWg]);
        let heightmaps = self.chunk.proto_heightmaps();
        if let Some(worldgen_surface) = heightmaps.get(HeightmapType::WorldSurfaceWg) {
            return steep_material_condition(worldgen_surface, world_x, world_z);
        }

        log::error!("WorldSurfaceWg heightmap missing during carver top-material lookup");
        false
    }

    /// Vanilla's `WorldCarver.getCarveState` + the nether override dispatch.
    fn get_carve_state(&mut self, params: &CarveParams<'_>, x: i32, y: i32, z: i32) -> CarveState {
        match params.style {
            CarverStyle::Overworld => {
                if y <= params.lava_level_y {
                    return CarveState::Place(self.ids.lava);
                }
                match self
                    .ctx
                    .aquifer
                    .compute_substance(self.noises, x, y, z, 0.0)
                {
                    AquiferResult::Solid => CarveState::Skip,
                    AquiferResult::Fluid(id) => CarveState::Place(id),
                    AquiferResult::Air => CarveState::Place(self.ids.air),
                }
            }
            CarverStyle::Nether => {
                if y <= self.ctx.min_y + 31 {
                    CarveState::Place(self.ids.lava)
                } else {
                    CarveState::Place(self.ids.cave_air)
                }
            }
        }
    }
}

/// Vanilla's `SurfaceRules.steep()` condition. It is asymmetric: only
/// south-vs-north and west-vs-east deltas are checked.
#[must_use]
fn steep_material_condition(worldgen_surface: &Heightmap, block_x: i32, block_z: i32) -> bool {
    let local_x = (block_x & 15) as usize;
    let local_z = (block_z & 15) as usize;

    let z_north = local_z.saturating_sub(1);
    let z_south = (local_z + 1).min(15);
    let h_north = worldgen_surface.get_highest_taken(local_x, z_north);
    let h_south = worldgen_surface.get_highest_taken(local_x, z_south);
    if h_south >= h_north + 4 {
        return true;
    }

    let x_west = local_x.saturating_sub(1);
    let x_east = (local_x + 1).min(15);
    let h_west = worldgen_surface.get_highest_taken(x_west, local_z);
    let h_east = worldgen_surface.get_highest_taken(x_east, local_z);
    h_west >= h_east + 4
}

/// Vanilla's `WorldCarver.canReach` — prunes carver steps that can't touch
/// any block in the given chunk (used by cave/canyon tunnel loops before
/// carving an ellipsoid).
#[must_use]
pub fn can_reach(
    chunk_min_x: i32,
    chunk_min_z: i32,
    x: f64,
    z: f64,
    current_step: i32,
    total_steps: i32,
    thickness: f32,
) -> bool {
    let x_mid = f64::from(chunk_min_x) + 8.0;
    let z_mid = f64::from(chunk_min_z) + 8.0;
    let xd = x - x_mid;
    let zd = z - z_mid;
    let remaining = f64::from(total_steps - current_step);
    let rr = f64::from(thickness + 2.0_f32 + 16.0_f32);
    xd * xd + zd * zd - remaining * remaining <= rr * rr
}

#[cfg(test)]
mod tests {
    use crate::chunk::heightmap::{Heightmap, HeightmapType};

    use super::steep_material_condition;

    fn flat_world_surface(highest_taken: i32) -> Heightmap {
        let mut heightmap = Heightmap::new(HeightmapType::WorldSurfaceWg, 0, 384);
        for x in 0..16 {
            for z in 0..16 {
                heightmap.set_height(x, z, highest_taken + 1);
            }
        }
        heightmap
    }

    #[test]
    fn steep_material_condition_matches_vanilla_asymmetry() {
        let mut heightmap = flat_world_surface(63);
        heightmap.set_height(5, 4, 61);
        heightmap.set_height(5, 6, 65);
        assert!(steep_material_condition(&heightmap, 5, 5));

        let mut heightmap = flat_world_surface(63);
        heightmap.set_height(5, 4, 65);
        heightmap.set_height(5, 6, 61);
        assert!(!steep_material_condition(&heightmap, 5, 5));

        let mut heightmap = flat_world_surface(63);
        heightmap.set_height(4, 5, 65);
        heightmap.set_height(6, 5, 61);
        assert!(steep_material_condition(&heightmap, 5, 5));

        let mut heightmap = flat_world_surface(63);
        heightmap.set_height(4, 5, 61);
        heightmap.set_height(6, 5, 65);
        assert!(!steep_material_condition(&heightmap, 5, 5));
    }
}
