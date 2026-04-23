//! World-carving: runtime types for running configured carvers during the
//! `CARVERS` chunk stage.
//!
//! Mirrors vanilla's `net.minecraft.world.level.levelgen.carver` package. The
//! [`CarvingContext`] bundles everything a carver needs: dimension bounds, the
//! chunk's aquifer, surface system reference, and preliminary-surface-level
//! corners for top-material lookup.

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::{REGISTRY, TaggedRegistryExt};
use steel_utils::density::DimensionNoises;
use steel_utils::math::noise_math::lerp2;
use steel_utils::surface::SurfaceRuleContext;
use steel_utils::{BlockPos, BlockStateId, Identifier, types::UpdateFlags};

use crate::chunk::aquifer::{Aquifer, AquiferResult};
use crate::chunk::carving_mask::CarvingMask;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::surface_system::SurfaceSystem;

pub mod canyon;
pub mod cave;

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
    /// Sea level — used for default fluid placement.
    pub sea_level: i32,
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
    /// lookup. Values are in world Y. Corners in order: `(0,0)`, `(16,0)`,
    /// `(0,16)`, `(16,16)` relative to the chunk's NW block corner.
    pub psl_corners: (i32, i32, i32, i32),
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
        let (p00, p10, p01, p11) = self.psl_corners;
        let interp = lerp2(
            t_x,
            t_z,
            f64::from(p00),
            f64::from(p10),
            f64::from(p01),
            f64::from(p11),
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
        under_fluid: bool,
    ) -> Option<BlockStateId> {
        // Surface noise inputs (same helpers build_surface uses per column).
        let surface_depth = self.surface_system.get_surface_depth(block_x, block_z);
        let surface_secondary = self.surface_system.get_surface_secondary(block_x, block_z);
        let min_surface_level = self.min_surface_level(block_x, block_z) + surface_depth - 8;

        let water_height = if under_fluid { block_y + 1 } else { i32::MIN };
        let cold_enough_to_snow = self
            .surface_system
            .cold_enough_to_snow(biome_id, block_x, block_y, block_z);

        let ctx = SurfaceRuleContext {
            block_x,
            block_z,
            surface_depth,
            surface_secondary,
            min_surface_level,
            // Steep is only consulted by a narrow branch of vanilla's surface
            // rules (mountains); a single-point carver lookup has no column
            // scan so vanilla effectively defaults it to false.
            steep: false,
            block_y,
            stone_depth_above: 1,
            stone_depth_below: 1,
            water_height,
            biome_id,
            cold_enough_to_snow,
            system: self.surface_system,
        };

        N::try_apply_surface_rule(&ctx)
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
    REGISTRY.blocks.is_in_tag(block, tag)
}

/// Which carver family dictates the per-block decision inside
/// [`carve_ellipsoid`]. Overworld carvers (cave + canyon) use the aquifer to
/// pick air / water / lava; the nether variant hardcodes lava below
/// `min_gen_y + 31` and cave-air elsewhere, with no aquifer lookups.
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
}

/// Predicate called inside the carver's Y scan to decide whether a block is
/// outside the carved shape for a given ellipsoid (cave floor cutoff, canyon
/// width-by-height, etc). Matches vanilla's `WorldCarver.CarveSkipChecker`.
pub trait CarveSkipChecker {
    /// `xd`, `yd`, `zd` are the ellipsoid-normalised offsets from the carver
    /// origin to this block's centre (see `carve_ellipsoid`); `world_y` is the
    /// absolute Y coordinate of the current block.
    fn should_skip(&mut self, xd: f64, yd: f64, zd: f64, world_y: i32) -> bool;
}

impl<F: FnMut(f64, f64, f64, i32) -> bool> CarveSkipChecker for F {
    fn should_skip(&mut self, xd: f64, yd: f64, zd: f64, world_y: i32) -> bool {
        self(xd, yd, zd, world_y)
    }
}

/// Aggregate of references a carver needs while iterating block positions.
/// Keeps `carve_ellipsoid`'s parameter list manageable.
pub struct CarveParams<'a> {
    /// Tag of blocks the carver is allowed to replace.
    pub replaceable_tag: &'a Identifier,
    /// Resolved lava level (world Y). At or below this, carved blocks become
    /// lava instead of air/water/etc.
    pub lava_level_y: i32,
    /// Which carver family this is (overworld vs nether).
    pub style: CarverStyle,
    /// Cached block state IDs to avoid per-block registry lookups.
    pub ids: CarverBlockIds,
}

/// Decision returned by [`get_carve_state`].
enum CarveState {
    /// Place this block.
    Place(BlockStateId),
    /// Aquifer barrier / "don't carve" — skip block.
    Skip,
}

/// Vanilla's `WorldCarver.getCarveState` — decides what block to place when
/// carving position `(x, y, z)` in an overworld carver.
fn get_carve_state_overworld<N: DimensionNoises>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    params: &CarveParams<'_>,
    x: i32,
    y: i32,
    z: i32,
) -> CarveState {
    if y <= params.lava_level_y {
        return CarveState::Place(params.ids.lava);
    }
    match ctx.aquifer.compute_substance(noises, x, y, z, 0.0) {
        AquiferResult::Solid => CarveState::Skip,
        AquiferResult::Fluid(id) => CarveState::Place(id),
        AquiferResult::Air => CarveState::Place(params.ids.air),
    }
}

/// Vanilla's nether-carver `carveBlock` substance pick: lava below
/// `min_gen_y + 31`, else `CAVE_AIR`. No aquifer lookups.
const fn get_carve_state_nether<N: DimensionNoises>(
    ctx: &CarvingContext<'_, N>,
    params: &CarveParams<'_>,
    y: i32,
) -> CarveState {
    if y <= ctx.min_y + 31 {
        CarveState::Place(params.ids.lava)
    } else {
        CarveState::Place(params.ids.cave_air)
    }
}

/// Returns whether the resulting block has a fluid state (i.e. is water or
/// lava). Used by the top-material flow to decide `under_fluid`.
fn block_has_fluid(state: BlockStateId, ids: &CarverBlockIds) -> bool {
    // Carver only ever places air / cave_air / lava / aquifer-picked water.
    // The aquifer result paths feed either `ids.air` or a fluid block state.
    // Anything that isn't an air variant counts as fluid.
    state != ids.air && state != ids.cave_air
}

/// Carve every block inside the given ellipsoid that falls in this chunk.
/// Mirrors vanilla's `WorldCarver.carveEllipsoid`.
///
/// Returns `true` if at least one block was carved.
#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla's carveEllipsoid signature; collapsing into a \
              struct would hurt locality in the hot inner loop"
)]
#[expect(
    clippy::similar_names,
    reason = "min_x_idx / min_z_idx / max_x_idx / max_z_idx mirror vanilla"
)]
pub fn carve_ellipsoid<N, F, S>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    chunk: &ChunkAccess,
    chunk_min_x: i32,
    chunk_min_z: i32,
    mut biome_getter: F,
    mask: &mut CarvingMask,
    params: &CarveParams<'_>,
    x: f64,
    y: f64,
    z: f64,
    horizontal_radius: f64,
    vertical_radius: f64,
    mut skip_checker: S,
) -> bool
where
    N: DimensionNoises,
    F: FnMut(i32, i32, i32) -> u16,
    S: CarveSkipChecker,
{
    let middle_x = f64::from(chunk_min_x) + 8.0;
    let middle_z = f64::from(chunk_min_z) + 8.0;
    let max_delta = 16.0 + horizontal_radius * 2.0;
    if (x - middle_x).abs() > max_delta || (z - middle_z).abs() > max_delta {
        return false;
    }

    let min_x_idx = ((x - horizontal_radius).floor() as i32 - chunk_min_x - 1).max(0);
    let max_x_idx = ((x + horizontal_radius).floor() as i32 - chunk_min_x).min(15);
    let min_y = ((y - vertical_radius).floor() as i32 - 1).max(ctx.min_y + 1);
    // Vanilla: `chunk.isUpgrading() ? 0 : 7`. No chunk upgrade path yet, so
    // always 7 — matches extractor config.
    let protected_blocks_on_top = 7;
    let max_y = ((y + vertical_radius).floor() as i32 + 1)
        .min(ctx.min_y + ctx.gen_depth - 1 - protected_blocks_on_top);
    let min_z_idx = ((z - horizontal_radius).floor() as i32 - chunk_min_z - 1).max(0);
    let max_z_idx = ((z + horizontal_radius).floor() as i32 - chunk_min_z).min(15);

    let mut carved = false;

    for x_idx in min_x_idx..=max_x_idx {
        let world_x = chunk_min_x + x_idx;
        let xd = (f64::from(world_x) + 0.5 - x) / horizontal_radius;

        for z_idx in min_z_idx..=max_z_idx {
            let world_z = chunk_min_z + z_idx;
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
                if mask.get(x_idx, world_y, z_idx) {
                    continue;
                }
                mask.set(x_idx, world_y, z_idx);
                if carve_block(
                    ctx,
                    noises,
                    chunk,
                    &mut biome_getter,
                    params,
                    world_x,
                    world_y,
                    world_z,
                    &mut has_grass,
                ) {
                    carved = true;
                }
            }
        }
    }

    carved
}

/// Per-block carve decision + placement. Mirrors vanilla's
/// `WorldCarver.carveBlock` (and the `NetherWorldCarver` override).
#[expect(
    clippy::too_many_arguments,
    reason = "faithful port of WorldCarver.carveBlock"
)]
fn carve_block<N, F>(
    ctx: &mut CarvingContext<'_, N>,
    noises: &N,
    chunk: &ChunkAccess,
    biome_getter: &mut F,
    params: &CarveParams<'_>,
    world_x: i32,
    world_y: i32,
    world_z: i32,
    has_grass: &mut bool,
) -> bool
where
    N: DimensionNoises,
    F: FnMut(i32, i32, i32) -> u16,
{
    let pos = BlockPos::new(world_x, world_y, world_z);
    let existing = chunk.get_block_state(pos);

    // Track grass/mycelium for the top-material rewrite later.
    if existing == params.ids.grass_block || existing == params.ids.mycelium {
        *has_grass = true;
    }

    if !can_replace_block(existing, params.replaceable_tag) {
        return false;
    }

    let state = match params.style {
        CarverStyle::Overworld => {
            match get_carve_state_overworld(ctx, noises, params, world_x, world_y, world_z) {
                CarveState::Place(id) => id,
                CarveState::Skip => return false,
            }
        }
        CarverStyle::Nether => match get_carve_state_nether(ctx, params, world_y) {
            CarveState::Place(id) => id,
            CarveState::Skip => return false,
        },
    };

    chunk.set_block_state(pos, state, UpdateFlags::empty());

    // TODO: call markPosForPostprocessing when aquifer.shouldScheduleFluidUpdate
    // and the placed block has a non-empty fluid state. Needs a
    // postprocessing queue on ProtoChunk.

    // Top-material rewrite: only when we just turned a grass/mycelium block
    // into something carved, and the block directly below is plain dirt.
    // Nether carver skips this entirely (its override of carveBlock doesn't
    // run this branch).
    if params.style == CarverStyle::Overworld && *has_grass {
        let below_pos = BlockPos::new(world_x, world_y - 1, world_z);
        if chunk.get_block_state(below_pos) == params.ids.dirt {
            let under_fluid = block_has_fluid(state, &params.ids);
            let biome_id = biome_getter(world_x, world_y - 1, world_z);
            if let Some(top) =
                ctx.top_material(biome_id, world_x, world_y - 1, world_z, under_fluid)
            {
                chunk.set_block_state(below_pos, top, UpdateFlags::empty());
                // TODO: markPosForPostprocessing when `top` has fluid state.
            }
        }
    }

    true
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
    let rr = f64::from(thickness) + 2.0 + 16.0;
    xd * xd + zd * zd - remaining * remaining <= rr * rr
}
