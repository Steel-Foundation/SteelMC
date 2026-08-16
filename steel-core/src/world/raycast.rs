use super::*;
use crate::chunk::gameplay_chunk_lookup_cache::LocalFullChunkHolderCache;
use crate::physics::block_has_extensible_collision_behavior;
use steel_registry::blocks::BlockRef;

/// Controls how a block position is treated during a raytrace traversal.
///
/// Returned by the predicate closure passed to [`World::raytrace`].
#[derive(Debug)]
pub enum RaytraceAction {
    /// Skip this block and continue traversal (transparent block).
    Pass,
    /// Test the block's voxel shape for a precise ray intersection.
    CheckShape,
    /// Immediately treat this block as a hit without shape testing.
    ImmediateHit,
}

/// Block shape channel used by vanilla-style world clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBlockShape {
    /// `ClipContext.Block.COLLIDER`
    Collider,
    /// `ClipContext.Block.OUTLINE`
    Outline,
    /// `ClipContext.Block.VISUAL`
    Visual,
    /// `ClipContext.Block.FALLDAMAGE_RESETTING`
    FallDamageResetting {
        /// Whether the clip context entity is a player.
        entity_is_player: bool,
    },
}

/// Fluid shape filter used by vanilla-style world clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFluid {
    /// `ClipContext.Fluid.NONE`
    None,
    /// `ClipContext.Fluid.SOURCE_ONLY`
    SourceOnly,
    /// `ClipContext.Fluid.ANY`
    Any,
    /// `ClipContext.Fluid.WATER`
    Water,
}

/// Result of a vanilla-style world clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipHitResult {
    /// Exact hit location in world coordinates.
    pub location: DVec3,
    /// Hit face, or the miss direction for misses.
    pub direction: Direction,
    /// Block position containing the hit or miss endpoint.
    pub block_pos: BlockPos,
    /// Whether this result is a miss.
    pub miss: bool,
    /// Whether the ray started inside the hit shape.
    pub inside: bool,
    /// Whether this hit was synthesized by the world border.
    pub world_border_hit: bool,
}

// Radius-four TNT exposure repeatedly visits a few hundred nearby positions. A 256-entry direct
// cache retains almost all of that working set without heap allocation or cross-explosion lifetime.
const EXPLOSION_EXPOSURE_CACHE_BITS: u32 = 8;
const EXPLOSION_EXPOSURE_CACHE_SIZE: usize = 1 << EXPLOSION_EXPOSURE_CACHE_BITS;
const EXPLOSION_EXPOSURE_CACHE_MASK: usize = EXPLOSION_EXPOSURE_CACHE_SIZE - 1;
const EXPLOSION_EXPOSURE_HASH_PHI: u64 = 0x9e37_79b9_7f4a_7c15;

// A normal radius-four explosion queries a 19x19x19 block region. Two bits per position fit in
// these inline words, avoiding one heap allocation per explosion while unusual radii simply use
// the compatibility path.
const EXPLOSION_EXPOSURE_CLEAR_GRID_WORDS: usize = 224;
const EXPLOSION_EXPOSURE_CLEAR_GRID_POSITIONS: usize =
    EXPLOSION_EXPOSURE_CLEAR_GRID_WORDS * (u64::BITS as usize / 2);

#[derive(Clone, Copy)]
struct ExplosionExposureClearGridBounds {
    min: BlockPos,
    size_x: usize,
    size_y: usize,
    size_z: usize,
}

struct ExplosionExposureClearGrid {
    bounds: Option<ExplosionExposureClearGridBounds>,
    // Packed two-bit values: 0 is unresolved, 1 is a stable static empty shape, and 2 must use
    // the exact collision path. Each lookup therefore reads one word instead of two bitsets.
    states: [u64; EXPLOSION_EXPOSURE_CLEAR_GRID_WORDS],
}

impl ExplosionExposureClearGrid {
    const fn new() -> Self {
        Self {
            bounds: None,
            states: [0; EXPLOSION_EXPOSURE_CLEAR_GRID_WORDS],
        }
    }

    fn configure(&mut self, min: BlockPos, max: BlockPos) {
        let Some(size_x) = inclusive_axis_size(min.x(), max.x()) else {
            self.bounds = None;
            return;
        };
        let Some(size_y) = inclusive_axis_size(min.y(), max.y()) else {
            self.bounds = None;
            return;
        };
        let Some(size_z) = inclusive_axis_size(min.z(), max.z()) else {
            self.bounds = None;
            return;
        };
        let Some(volume) = size_x
            .checked_mul(size_y)
            .and_then(|plane| plane.checked_mul(size_z))
        else {
            self.bounds = None;
            return;
        };
        if volume > EXPLOSION_EXPOSURE_CLEAR_GRID_POSITIONS {
            self.bounds = None;
            return;
        }

        self.bounds = Some(ExplosionExposureClearGridBounds {
            min,
            size_x,
            size_y,
            size_z,
        });
        self.states.fill(0);
    }

    fn clear(&mut self) {
        self.states.fill(0);
    }

    fn index(&self, pos: BlockPos) -> Option<usize> {
        let bounds = self.bounds?;
        // Wrapping subtraction maps every below-min coordinate to a large unsigned value. Since
        // the configured grid is tiny, the same range checks safely reject both underflow and
        // coordinates above the maximum without three checked-subtraction branches.
        let x = pos.x().wrapping_sub(bounds.min.x()) as u32 as usize;
        let y = pos.y().wrapping_sub(bounds.min.y()) as u32 as usize;
        let z = pos.z().wrapping_sub(bounds.min.z()) as u32 as usize;
        if x >= bounds.size_x || y >= bounds.size_y || z >= bounds.size_z {
            return None;
        }
        Some((y * bounds.size_z + z) * bounds.size_x + x)
    }

    const fn cached(&self, index: usize) -> Option<bool> {
        let word = index / (u64::BITS as usize / 2);
        let shift = (index % (u64::BITS as usize / 2)) * 2;
        match (self.states[word] >> shift) & 0b11 {
            0 => None,
            1 => Some(true),
            _ => Some(false),
        }
    }

    fn record(&mut self, index: usize, clear: bool) {
        let word = index / (u64::BITS as usize / 2);
        let shift = (index % (u64::BITS as usize / 2)) * 2;
        let mask = 0b11_u64 << shift;
        let value = u64::from(if clear { 1_u8 } else { 2_u8 }) << shift;
        self.states[word] = (self.states[word] & !mask) | value;
    }
}

fn inclusive_axis_size(min: i32, max: i32) -> Option<usize> {
    let size = usize::try_from(max.checked_sub(min)?.checked_add(1)?).ok()?;
    (size != 0).then_some(size)
}

#[derive(Clone, Copy)]
struct ExplosionExposureCacheEntry {
    pos: BlockPos,
    collision: OffsetVoxelShape,
    generation: u32,
}

impl ExplosionExposureCacheEntry {
    const EMPTY: Self = Self {
        pos: BlockPos::new(0, 0, 0),
        collision: OffsetVoxelShape::without_offset(VoxelShape::EMPTY),
        generation: 0,
    };
}

/// Bounded cache for vanilla explosion exposure rays.
///
/// Missing chunks are never cached, allowing asynchronous Full publication to become visible
/// while the synchronous gameplay callback is running. Dynamic and non-Vanilla collision shapes
/// are resolved on every visit so live block entities and plugin callbacks retain their normal
/// query semantics. Reuse across entities is only valid when the caller can prove that the
/// intervening entity callbacks cannot mutate blocks; callers must clear the cache before invoking
/// any other entity implementation.
pub(crate) struct ExplosionExposureRaycast<'world> {
    world: &'world World,
    collision_context: BlockCollisionContext,
    full_chunks: LocalFullChunkHolderCache,
    entries: [ExplosionExposureCacheEntry; EXPLOSION_EXPOSURE_CACHE_SIZE],
    clear_grid: ExplosionExposureClearGrid,
    generation: u32,
    #[cfg(test)]
    stats: ExplosionExposureRaycastStats,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExplosionExposureRaycastStats {
    pub(crate) block_visits: usize,
    pub(crate) cache_hits: usize,
    pub(crate) state_lookups: usize,
    pub(crate) collision_lookups: usize,
    pub(crate) clear_grid_hits: usize,
    pub(crate) clear_grid_resolutions: usize,
}

impl<'world> ExplosionExposureRaycast<'world> {
    pub(crate) const fn new(
        world: &'world World,
        collision_context: BlockCollisionContext,
    ) -> Self {
        Self {
            world,
            collision_context,
            full_chunks: LocalFullChunkHolderCache::new(),
            entries: [ExplosionExposureCacheEntry::EMPTY; EXPLOSION_EXPOSURE_CACHE_SIZE],
            clear_grid: ExplosionExposureClearGrid::new(),
            generation: 1,
            #[cfg(test)]
            stats: ExplosionExposureRaycastStats {
                block_visits: 0,
                cache_hits: 0,
                state_lookups: 0,
                collision_lookups: 0,
                clear_grid_hits: 0,
                clear_grid_resolutions: 0,
            },
        }
    }

    /// Configures a bounded, explosion-local cache of statically clear positions.
    ///
    /// Bounds that exceed the inline capacity disable the optimization and retain the exact
    /// compatibility path.
    pub(crate) fn configure_clear_grid(&mut self, min: BlockPos, max: BlockPos) {
        self.clear_grid.configure(min, max);
    }

    /// Drops every retained static collision result.
    pub(crate) fn clear(&mut self) {
        self.clear_grid.clear();
        if self.generation == u32::MAX {
            self.entries.fill(ExplosionExposureCacheEntry::EMPTY);
            self.generation = 1;
        } else {
            self.generation += 1;
        }
    }

    /// Selects the entity collision context used by subsequent exposure rays.
    pub(crate) const fn set_collision_context(&mut self, collision_context: BlockCollisionContext) {
        self.collision_context = collision_context;
    }

    /// Keeps cached shapes only across Steel-owned Vanilla collision behavior calls.
    ///
    /// Plugin behavior may mutate another block while answering a shape query. Clearing after its
    /// callback makes the next Vanilla DDA visit read the same live state Vanilla would observe.
    fn retain_cache_after_collision_query(&mut self, block: BlockRef) -> bool {
        let retain = !block_has_extensible_collision_behavior(block);
        if !retain {
            self.clear();
        }
        retain
    }

    /// Returns whether a collider-only, fluid-free exposure ray misses every block.
    pub(crate) fn is_path_clear(&mut self, from: DVec3, to: DVec3) -> bool {
        is_collision_path_clear(from, to, |pos| self.block_intersects_ray(pos, from, to))
    }

    fn block_intersects_ray(&mut self, pos: BlockPos, from: DVec3, to: DVec3) -> bool {
        #[cfg(test)]
        {
            self.stats.block_visits += 1;
        }

        let mut unresolved_grid_index = None;
        if let Some(index) = self.clear_grid.index(pos) {
            match self.clear_grid.cached(index) {
                Some(true) => {
                    #[cfg(test)]
                    {
                        self.stats.clear_grid_hits += 1;
                    }
                    return false;
                }
                Some(false) => {}
                None => unresolved_grid_index = Some(index),
            }
        }

        self.block_intersects_ray_exact(pos, from, to, unresolved_grid_index)
    }

    fn block_intersects_ray_exact(
        &mut self,
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
        unresolved_grid_index: Option<usize>,
    ) -> bool {
        let cache_index = explosion_exposure_cache_index(pos);
        let entry = self.entries[cache_index];
        if entry.generation == self.generation && entry.pos == pos {
            #[cfg(test)]
            {
                self.stats.cache_hits += 1;
            }
            if let Some(index) = unresolved_grid_index {
                self.clear_grid.record(index, entry.collision.is_empty());
                #[cfg(test)]
                {
                    self.stats.clear_grid_resolutions += 1;
                }
            }
            return Self::static_collision_intersects(entry.collision, pos, from, to);
        }

        #[cfg(test)]
        {
            self.stats.state_lookups += 1;
            self.stats.collision_lookups += 1;
        }
        let (state, cacheable) = self
            .world
            .explosion_exposure_block_state(pos, &mut self.full_chunks);
        let block = state.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        if block.config.dynamic_shape {
            let boxes =
                behavior.get_collision_boxes(state, self.world, pos, self.collision_context);
            self.retain_cache_after_collision_query(block);
            return boxes
                .into_iter()
                .any(|aabb| World::clip_local_aabb(pos, from, to, aabb).is_some());
        }

        let shape = behavior.get_collision_shape(state, self.world, pos, self.collision_context);
        let offset = if shape.is_empty() {
            DVec3::ZERO
        } else {
            behavior.get_collision_shape_offset(state, self.world, pos, self.collision_context)
        };
        let collision = OffsetVoxelShape::new(shape, offset);
        let retain_cache = self.retain_cache_after_collision_query(block);
        let intersects = Self::static_collision_intersects(collision, pos, from, to);
        if cacheable && retain_cache {
            let collision_is_empty = collision.is_empty();
            let mut recorded_in_grid = false;
            if let Some(index) = unresolved_grid_index {
                self.clear_grid.record(index, collision_is_empty);
                recorded_in_grid = true;
                #[cfg(test)]
                {
                    self.stats.clear_grid_resolutions += 1;
                }
            }
            // Dense clear entries no longer need a large direct-cache slot. Retaining only
            // potentially colliding shapes reduces destructive conflicts on the exact path.
            if !recorded_in_grid || !collision_is_empty {
                self.entries[cache_index] = ExplosionExposureCacheEntry {
                    pos,
                    collision,
                    generation: self.generation,
                };
            }
        }
        intersects
    }

    fn static_collision_intersects(
        collision: OffsetVoxelShape,
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
    ) -> bool {
        collision
            .iter()
            .any(|aabb| World::clip_local_aabb(pos, from, to, aabb).is_some())
    }

    #[cfg(test)]
    pub(crate) const fn stats(&self) -> ExplosionExposureRaycastStats {
        self.stats
    }
}

#[inline]
fn explosion_exposure_cache_index(pos: BlockPos) -> usize {
    let tag = PackedBlockPos::from(pos).as_raw() as u64;
    let mut mixed = tag.wrapping_mul(EXPLOSION_EXPOSURE_HASH_PHI);
    mixed ^= mixed >> 32;
    mixed ^= mixed >> 16;
    (mixed as usize) & EXPLOSION_EXPOSURE_CACHE_MASK
}

#[inline]
fn is_collision_path_clear(
    start_pos: DVec3,
    end_pos: DVec3,
    mut blocks_ray: impl FnMut(BlockPos) -> bool,
) -> bool {
    if start_pos == end_pos {
        return true;
    }

    let adjust = -1.0e-7f64;
    let to = end_pos.lerp(start_pos, adjust);
    let from = start_pos.lerp(end_pos, adjust);
    let mut block = BlockPos::from(from);
    if blocks_ray(block) {
        return false;
    }

    let difference = to - from;
    let step = glam::IVec3::new(
        minecraft_sign(difference.x),
        minecraft_sign(difference.y),
        minecraft_sign(difference.z),
    );
    let delta = DVec3::new(
        if step.x == 0 {
            f64::MAX
        } else {
            f64::from(step.x) / difference.x
        },
        if step.y == 0 {
            f64::MAX
        } else {
            f64::from(step.y) / difference.y
        },
        if step.z == 0 {
            f64::MAX
        } else {
            f64::from(step.z) / difference.z
        },
    );
    let mut next = DVec3::new(
        delta.x
            * if step.x > 0 {
                1.0 - (from.x - from.x.floor())
            } else {
                from.x - from.x.floor()
            },
        delta.y
            * if step.y > 0 {
                1.0 - (from.y - from.y.floor())
            } else {
                from.y - from.y.floor()
            },
        delta.z
            * if step.z > 0 {
                1.0 - (from.z - from.z.floor())
            } else {
                from.z - from.z.floor()
            },
    );

    while next.x <= 1.0 || next.y <= 1.0 || next.z <= 1.0 {
        if next.x < next.y && next.x < next.z {
            block.0.x += step.x;
            next.x += delta.x;
        } else if next.y < next.z {
            block.0.y += step.y;
            next.y += delta.y;
        } else {
            block.0.z += step.z;
            next.z += delta.z;
        }
        if blocks_ray(block) {
            return false;
        }
    }

    true
}

#[inline]
const fn minecraft_sign(number: f64) -> i32 {
    if number == 0.0 {
        0
    } else if number > 0.0 {
        1
    } else {
        -1
    }
}

impl ClipHitResult {
    /// Returns whether this clip missed all selected block and fluid shapes.
    #[must_use]
    pub const fn is_miss(self) -> bool {
        self.miss
    }
}

impl World {
    /// Reads exposure state while distinguishing stable Full data from an air
    /// fallback caused by a chunk that may still publish asynchronously.
    fn explosion_exposure_block_state(
        &self,
        pos: BlockPos,
        full_chunks: &mut LocalFullChunkHolderCache,
    ) -> (BlockStateId, bool) {
        if !self.is_in_valid_bounds(pos) {
            return (
                REGISTRY.blocks.get_base_state_id(&vanilla_blocks::VOID_AIR),
                true,
            );
        }

        full_chunks.block_state(&self.chunk_map, pos).map_or_else(
            || {
                (
                    REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR),
                    false,
                )
            },
            |state| (state, true),
        )
    }

    /// Checks if a ray intersects with a block's selection box.
    pub fn ray_outline_check(
        &self,
        block_pos: BlockPos,
        from: DVec3,
        to: DVec3,
    ) -> (bool, Option<Direction>) {
        let state = self.get_block_state(block_pos);
        let shape = state.get_outline_shape_at(block_pos);

        match Self::clip_shape(block_pos, from, to, shape) {
            Some(hit) => (true, Some(hit.direction)),
            None => (false, None),
        }
    }

    /// Performs a vanilla-style block/fluid clip in the world.
    #[must_use]
    pub fn clip(
        &self,
        start_pos: DVec3,
        end_pos: DVec3,
        block_shape: ClipBlockShape,
        fluid: ClipFluid,
    ) -> ClipHitResult {
        if start_pos == end_pos {
            return Self::clip_miss(start_pos, end_pos);
        }

        let adjust = -1.0e-7f64;
        let to = end_pos.lerp(start_pos, adjust);
        let from = start_pos.lerp(end_pos, adjust);

        let mut block = BlockPos::new(
            from.x.floor() as i32,
            from.y.floor() as i32,
            from.z.floor() as i32,
        );

        if let Some(hit) = self.clip_block_and_fluid(block, start_pos, end_pos, block_shape, fluid)
        {
            return hit;
        }

        let difference = to - from;

        let step = difference.signum().as_ivec3();

        let delta = DVec3::new(
            if step.x == 0 {
                f64::MAX
            } else {
                (f64::from(step.x)) / difference.x
            },
            if step.y == 0 {
                f64::MAX
            } else {
                (f64::from(step.y)) / difference.y
            },
            if step.z == 0 {
                f64::MAX
            } else {
                (f64::from(step.z)) / difference.z
            },
        );

        let mut next = DVec3::new(
            delta.x
                * (if step.x > 0 {
                    1.0 - (from.x - from.x.floor())
                } else {
                    from.x - from.x.floor()
                }),
            delta.y
                * (if step.y > 0 {
                    1.0 - (from.y - from.y.floor())
                } else {
                    from.y - from.y.floor()
                }),
            delta.z
                * (if step.z > 0 {
                    1.0 - (from.z - from.z.floor())
                } else {
                    from.z - from.z.floor()
                }),
        );

        while next.x <= 1.0 || next.y <= 1.0 || next.z <= 1.0 {
            if next.x < next.y && next.x < next.z {
                block.0.x += step.x;
                next.x += delta.x;
            } else if next.y < next.z {
                block.0.y += step.y;
                next.y += delta.y;
            } else {
                block.0.z += step.z;
                next.z += delta.z;
            }

            if let Some(hit) =
                self.clip_block_and_fluid(block, start_pos, end_pos, block_shape, fluid)
            {
                return hit;
            }
        }

        Self::clip_miss(start_pos, end_pos)
    }

    /// Returns whether a collider-only, fluid-free clip misses every block.
    #[cfg(test)]
    pub(crate) fn is_block_collision_path_clear(
        &self,
        start_pos: DVec3,
        end_pos: DVec3,
        collision_context: BlockCollisionContext,
    ) -> bool {
        is_collision_path_clear(start_pos, end_pos, |pos| {
            let state = self.get_block_state(pos);
            let boxes = BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .get_collision_boxes(state, self, pos, collision_context);
            boxes
                .into_iter()
                .any(|aabb| Self::clip_local_aabb(pos, start_pos, end_pos, aabb).is_some())
        })
    }

    /// Performs vanilla `CollisionGetter.clipIncludingBorder`.
    #[must_use]
    pub fn clip_including_border(
        &self,
        start_pos: DVec3,
        end_pos: DVec3,
        block_shape: ClipBlockShape,
        fluid: ClipFluid,
    ) -> ClipHitResult {
        let hit = self.clip(start_pos, end_pos, block_shape, fluid);
        let border = self.world_border_snapshot();
        if border.is_within_bounds_with_margin(start_pos.x, start_pos.z, 0.0)
            && !border.is_within_bounds_with_margin(hit.location.x, hit.location.z, 0.0)
        {
            let delta = hit.location - start_pos;
            let location = border.clamp_vec3_to_bound(hit.location);
            return ClipHitResult {
                location,
                direction: Self::approximate_nearest_direction(delta),
                block_pos: BlockPos::from(location),
                miss: false,
                inside: false,
                world_border_hit: true,
            };
        }
        hit
    }

    pub(super) fn clip_block_and_fluid(
        &self,
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
        block_shape: ClipBlockShape,
        fluid: ClipFluid,
    ) -> Option<ClipHitResult> {
        let state = self.get_block_state(pos);
        let block_result = Self::clip_shape(
            pos,
            from,
            to,
            self.clip_block_shape(state, pos, block_shape),
        )
        .map(|hit| Self::clip_with_interaction_override(pos, from, to, state, hit));
        let fluid_result = self.clip_fluid_shape(pos, from, to, state, fluid);

        match (block_result, fluid_result) {
            (Some(block_hit), Some(fluid_hit)) => {
                let block_distance = from.distance_squared(block_hit.location);
                let fluid_distance = from.distance_squared(fluid_hit.location);
                if block_distance <= fluid_distance {
                    Some(block_hit)
                } else {
                    Some(fluid_hit)
                }
            }
            (Some(hit), None) | (None, Some(hit)) => Some(hit),
            (None, None) => None,
        }
    }

    pub(super) fn clip_with_interaction_override(
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
        state: BlockStateId,
        block_hit: ClipHitResult,
    ) -> ClipHitResult {
        let Some(override_hit) =
            Self::clip_shape(pos, from, to, state.get_interaction_shape_at(pos))
        else {
            return block_hit;
        };

        if from.distance_squared(override_hit.location) < from.distance_squared(block_hit.location)
        {
            ClipHitResult {
                direction: override_hit.direction,
                ..block_hit
            }
        } else {
            block_hit
        }
    }

    pub(super) fn clip_block_shape(
        &self,
        state: BlockStateId,
        pos: BlockPos,
        shape: ClipBlockShape,
    ) -> OffsetVoxelShape {
        match shape {
            ClipBlockShape::Collider => state.get_collision_shape_at(pos),
            ClipBlockShape::Outline => state.get_outline_shape_at(pos),
            ClipBlockShape::Visual => state.get_visual_shape_at(pos),
            ClipBlockShape::FallDamageResetting { entity_is_player } => {
                OffsetVoxelShape::without_offset(
                    self.fall_damage_resetting_shape(state, entity_is_player),
                )
            }
        }
    }

    pub(super) fn fall_damage_resetting_shape(
        &self,
        state: BlockStateId,
        entity_is_player: bool,
    ) -> VoxelShape {
        let block = state.get_block();
        if block.has_tag(&BlockTag::FALL_DAMAGE_RESETTING) {
            return VoxelShape::FULL_BLOCK;
        }

        if !entity_is_player {
            return VoxelShape::EMPTY;
        }

        if block == &vanilla_blocks::END_GATEWAY || block == &vanilla_blocks::END_PORTAL {
            return VoxelShape::FULL_BLOCK;
        }

        if block == &vanilla_blocks::NETHER_PORTAL
            && self.get_game_rule(&PLAYERS_NETHER_PORTAL_DEFAULT_DELAY) == 0
        {
            return VoxelShape::FULL_BLOCK;
        }

        VoxelShape::EMPTY
    }

    pub(super) fn clip_fluid_shape(
        &self,
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
        state: BlockStateId,
        fluid: ClipFluid,
    ) -> Option<ClipHitResult> {
        let fluid_state = state.get_fluid_state();
        let can_pick = match fluid {
            ClipFluid::None => false,
            ClipFluid::SourceOnly => fluid_state.is_source(),
            ClipFluid::Any => !fluid_state.is_empty(),
            ClipFluid::Water => fluid_state.is_water(),
        };
        if !can_pick {
            return None;
        }

        let height = self.fluid_clip_height(pos, fluid_state);
        Self::clip_local_aabb(
            pos,
            from,
            to,
            BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, height, 1.0),
        )
    }

    pub(super) fn fluid_clip_height(&self, pos: BlockPos, fluid_state: FluidState) -> f64 {
        let above_fluid = self.get_block_state(pos.above()).get_fluid_state();
        Self::fluid_clip_height_from_above(fluid_state, above_fluid)
    }

    pub(super) fn fluid_clip_height_from_above(
        fluid_state: FluidState,
        above_fluid: FluidState,
    ) -> f64 {
        if FLUID_BEHAVIORS
            .get_behavior(fluid_state.fluid_id)
            .is_same(above_fluid.fluid_id)
        {
            1.0
        } else {
            f64::from(fluid_state.own_height())
        }
    }

    pub(super) fn clip_shape(
        block_pos: BlockPos,
        from: DVec3,
        to: DVec3,
        shape: OffsetVoxelShape,
    ) -> Option<ClipHitResult> {
        if shape.is_empty() {
            return None;
        }

        if (to - from).length_squared() < 1.0e-7 {
            return None;
        }

        let block_vec = DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        );
        let inside_test_point = from + (to - from) * 0.001;
        if Self::shape_contains_world_point(shape, block_vec, inside_test_point) {
            return Some(ClipHitResult {
                location: inside_test_point,
                direction: Self::approximate_nearest_direction(to - from).opposite(),
                block_pos,
                miss: false,
                inside: true,
                world_border_hit: false,
            });
        }

        let mut closest: Option<(f64, Direction)> = None;

        for shape in shape.iter() {
            let world_min = DVec3::new(shape.min_x(), shape.min_y(), shape.min_z()) + block_vec;
            let world_max = DVec3::new(shape.max_x(), shape.max_y(), shape.max_z()) + block_vec;

            if let Some(hit) = Self::intersects_aabb_with_t(from, to, world_min, world_max)
                && hit.0 > 0.0
                && hit.0 < 1.0
                && closest.is_none_or(|(best_t, _)| hit.0 < best_t)
            {
                closest = Some(hit);
            }
        }

        closest.map(|(t, direction)| ClipHitResult {
            location: from + (to - from) * t,
            direction,
            block_pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        })
    }

    pub(super) fn clip_local_aabb(
        block_pos: BlockPos,
        from: DVec3,
        to: DVec3,
        aabb: BlockLocalAabb,
    ) -> Option<ClipHitResult> {
        if aabb.is_empty() {
            return None;
        }

        if (to - from).length_squared() < 1.0e-7 {
            return None;
        }

        let block_vec = DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        );
        let inside_test_point = from + (to - from) * 0.001;
        if Self::local_aabb_contains_world_point(aabb, block_vec, inside_test_point) {
            return Some(ClipHitResult {
                location: inside_test_point,
                direction: Self::approximate_nearest_direction(to - from).opposite(),
                block_pos,
                miss: false,
                inside: true,
                world_border_hit: false,
            });
        }

        let world_min = DVec3::new(aabb.min_x(), aabb.min_y(), aabb.min_z()) + block_vec;
        let world_max = DVec3::new(aabb.max_x(), aabb.max_y(), aabb.max_z()) + block_vec;
        Self::intersects_aabb_with_t(from, to, world_min, world_max).and_then(|(t, direction)| {
            if t > 0.0 && t < 1.0 {
                Some(ClipHitResult {
                    location: from + (to - from) * t,
                    direction,
                    block_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                })
            } else {
                None
            }
        })
    }

    pub(super) fn shape_contains_world_point(
        shape: OffsetVoxelShape,
        block_vec: DVec3,
        point: DVec3,
    ) -> bool {
        shape
            .iter()
            .any(|aabb| Self::local_aabb_contains_world_point(aabb, block_vec, point))
    }

    pub(super) fn local_aabb_contains_world_point(
        aabb: BlockLocalAabb,
        block_vec: DVec3,
        point: DVec3,
    ) -> bool {
        let local = point - block_vec;
        !aabb.is_empty()
            && local.x >= aabb.min_x()
            && local.x < aabb.max_x()
            && local.y >= aabb.min_y()
            && local.y < aabb.max_y()
            && local.z >= aabb.min_z()
            && local.z < aabb.max_z()
    }

    pub(super) fn clip_miss(from: DVec3, to: DVec3) -> ClipHitResult {
        ClipHitResult {
            location: to,
            direction: Self::approximate_nearest_direction(from - to),
            block_pos: BlockPos::from(to),
            miss: true,
            inside: false,
            world_border_hit: false,
        }
    }

    pub(super) fn approximate_nearest_direction(vector: DVec3) -> Direction {
        let mut result = Direction::North;
        let mut highest_dot = 0.0;
        for direction in [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            let dot = vector.dot(direction.offset_vec().as_dvec3());
            if dot > highest_dot {
                highest_dot = dot;
                result = direction;
            }
        }
        result
    }

    /// Mirrors Minecraft 26.2 `AABB.getDirection` for one block-local shape box.
    pub(super) fn intersects_aabb_with_t(
        start: DVec3,
        end: DVec3,
        min: DVec3,
        max: DVec3,
    ) -> Option<(f64, Direction)> {
        let delta = end - start;
        let mut scale = 1.0;
        let mut direction = None;

        if delta.x > 1.0e-7 {
            Self::clip_aabb_face(
                &mut scale,
                &mut direction,
                delta,
                start,
                min.x,
                [min.y, max.y, min.z, max.z],
                Direction::West,
            );
        } else if delta.x < -1.0e-7 {
            Self::clip_aabb_face(
                &mut scale,
                &mut direction,
                delta,
                start,
                max.x,
                [min.y, max.y, min.z, max.z],
                Direction::East,
            );
        }

        let y_delta = DVec3::new(delta.y, delta.z, delta.x);
        let y_start = DVec3::new(start.y, start.z, start.x);
        if delta.y > 1.0e-7 {
            Self::clip_aabb_face(
                &mut scale,
                &mut direction,
                y_delta,
                y_start,
                min.y,
                [min.z, max.z, min.x, max.x],
                Direction::Down,
            );
        } else if delta.y < -1.0e-7 {
            Self::clip_aabb_face(
                &mut scale,
                &mut direction,
                y_delta,
                y_start,
                max.y,
                [min.z, max.z, min.x, max.x],
                Direction::Up,
            );
        }

        let z_delta = DVec3::new(delta.z, delta.x, delta.y);
        let z_start = DVec3::new(start.z, start.x, start.y);
        if delta.z > 1.0e-7 {
            Self::clip_aabb_face(
                &mut scale,
                &mut direction,
                z_delta,
                z_start,
                min.z,
                [min.x, max.x, min.y, max.y],
                Direction::North,
            );
        } else if delta.z < -1.0e-7 {
            Self::clip_aabb_face(
                &mut scale,
                &mut direction,
                z_delta,
                z_start,
                max.z,
                [min.x, max.x, min.y, max.y],
                Direction::South,
            );
        }

        direction.map(|direction| (scale, direction))
    }

    #[inline]
    fn clip_aabb_face(
        scale: &mut f64,
        direction: &mut Option<Direction>,
        delta: DVec3,
        start: DVec3,
        face: f64,
        side_bounds: [f64; 4],
        new_direction: Direction,
    ) {
        let candidate_scale = (face - start.x) / delta.x;
        let side_b = start.y + candidate_scale * delta.y;
        let side_c = start.z + candidate_scale * delta.z;
        if 0.0 < candidate_scale
            && candidate_scale < *scale
            && side_bounds[0] - 1.0e-7 < side_b
            && side_b < side_bounds[1] + 1.0e-7
            && side_bounds[2] - 1.0e-7 < side_c
            && side_c < side_bounds[3] + 1.0e-7
        {
            *scale = candidate_scale;
            *direction = Some(new_direction);
        }
    }

    /// Performs a raytrace in the world.
    ///
    /// Adapted from Pumpkin project.
    pub fn raytrace<F>(
        &self,
        start_pos: DVec3,
        end_pos: DVec3,
        hit_check: F,
    ) -> (Option<BlockPos>, Option<Direction>)
    where
        F: Fn(BlockPos, &Self) -> RaytraceAction,
    {
        if start_pos == end_pos {
            return (None, None);
        }

        let adjust = -1.0e-7f64;
        let to = end_pos.lerp(start_pos, adjust);
        let from = start_pos.lerp(end_pos, adjust);

        let mut block = BlockPos::new(
            from.x.floor() as i32,
            from.y.floor() as i32,
            from.z.floor() as i32,
        );

        match hit_check(block, self) {
            RaytraceAction::ImmediateHit => return (Some(block), None),
            RaytraceAction::CheckShape => {
                let (hit, face) = self.ray_outline_check(block, start_pos, end_pos);
                if hit {
                    return (Some(block), face);
                }
            }
            RaytraceAction::Pass => {}
        }

        let difference = to - from;

        let step = difference.signum().as_ivec3();

        let delta = DVec3::new(
            if step.x == 0 {
                f64::MAX
            } else {
                (f64::from(step.x)) / difference.x
            },
            if step.y == 0 {
                f64::MAX
            } else {
                (f64::from(step.y)) / difference.y
            },
            if step.z == 0 {
                f64::MAX
            } else {
                (f64::from(step.z)) / difference.z
            },
        );

        let mut next = DVec3::new(
            delta.x
                * (if step.x > 0 {
                    1.0 - (from.x - from.x.floor())
                } else {
                    from.x - from.x.floor()
                }),
            delta.y
                * (if step.y > 0 {
                    1.0 - (from.y - from.y.floor())
                } else {
                    from.y - from.y.floor()
                }),
            delta.z
                * (if step.z > 0 {
                    1.0 - (from.z - from.z.floor())
                } else {
                    from.z - from.z.floor()
                }),
        );

        while next.x <= 1.0 || next.y <= 1.0 || next.z <= 1.0 {
            // Vanilla parity: traverseBlocks tie-breaking — Z wins on any tie.
            // X wins only when strictly less than both Y and Z.
            // Y wins only when strictly less than both X and Z.
            // Everything else (including all ties) goes to Z.
            let block_direction = if next.x < next.y && next.x < next.z {
                block.0.x += step.x;
                next.x += delta.x;
                if step.x > 0 {
                    Direction::West
                } else {
                    Direction::East
                }
            } else if next.y < next.x && next.y < next.z {
                block.0.y += step.y;
                next.y += delta.y;
                if step.y > 0 {
                    Direction::Down
                } else {
                    Direction::Up
                }
            } else {
                block.0.z += step.z;
                next.z += delta.z;
                if step.z > 0 {
                    Direction::North
                } else {
                    Direction::South
                }
            };

            match hit_check(block, self) {
                RaytraceAction::ImmediateHit => {
                    return (Some(block), Some(block_direction));
                }
                RaytraceAction::CheckShape => {
                    let (hit, face) = self.ray_outline_check(block, start_pos, end_pos);
                    if hit {
                        return (Some(block), face);
                    }
                }
                RaytraceAction::Pass => {}
            }
        }

        (None, None)
    }
}

#[cfg(test)]
mod voxel_shape_clip_tests {
    use super::*;

    fn axis_point(axis: usize, primary: f64, side_b: f64, side_c: f64) -> DVec3 {
        match axis {
            0 => DVec3::new(primary, side_b, side_c),
            1 => DVec3::new(side_c, primary, side_b),
            2 => DVec3::new(side_b, side_c, primary),
            _ => unreachable!("the fixture only defines the three coordinate axes"),
        }
    }

    fn axis_bounds(axis: usize, primary_min: f64, primary_max: f64) -> (DVec3, DVec3) {
        (
            axis_point(axis, primary_min, 0.0, 0.0),
            axis_point(axis, primary_max, 1.0, 1.0),
        )
    }

    fn primary_component(vector: DVec3, axis: usize) -> f64 {
        match axis {
            0 => vector.x,
            1 => vector.y,
            2 => vector.z,
            _ => unreachable!("the fixture only defines the three coordinate axes"),
        }
    }

    #[test]
    fn voxel_shape_inside_probe_is_lower_inclusive_and_upper_exclusive() {
        let full_block = BlockLocalAabb::FULL_BLOCK;
        let block_origin = DVec3::ZERO;

        assert!(World::local_aabb_contains_world_point(
            full_block,
            block_origin,
            DVec3::new(0.0, 0.5, 0.5),
        ));
        assert!(!World::local_aabb_contains_world_point(
            full_block,
            block_origin,
            DVec3::new(1.0, 0.5, 0.5),
        ));
        assert!(!World::local_aabb_contains_world_point(
            full_block,
            block_origin,
            DVec3::new(0.5, 1.0, 0.5),
        ));
        assert!(!World::local_aabb_contains_world_point(
            full_block,
            block_origin,
            DVec3::new(0.5, 0.5, 1.0),
        ));
    }

    #[test]
    fn voxel_shape_clip_matches_java_full_cube_at_exact_opposite_boundaries() {
        let shape = OffsetVoxelShape::without_offset(VoxelShape::FULL_BLOCK);

        // Minecraft 26.2's VoxelShape.clip probes 0.001 along the segment. For a
        // CubeVoxelShape the resulting x=1 index is outside, while x=0 is inside.
        assert!(
            World::clip_shape(
                BlockPos::ZERO,
                DVec3::new(0.0, 0.5, 0.5),
                DVec3::new(1000.0, 0.5, 0.5),
                shape,
            )
            .is_none()
        );

        let Some(hit) = World::clip_shape(
            BlockPos::ZERO,
            DVec3::new(1.0, 0.5, 0.5),
            DVec3::new(-999.0, 0.5, 0.5),
            shape,
        ) else {
            panic!("the lower-bound probe should start inside the full cube");
        };
        assert!(hit.inside);
        assert_eq!(hit.location, DVec3::new(0.0, 0.5, 0.5));
        assert_eq!(hit.direction, Direction::East);
    }

    #[test]
    fn aabb_clip_matches_java_faces_in_every_axis_and_direction() {
        let expected_scale = 1.0_f64 / 3.0;
        for (axis, positive_direction, negative_direction) in [
            (0, Direction::West, Direction::East),
            (1, Direction::Down, Direction::Up),
            (2, Direction::North, Direction::South),
        ] {
            let (min, max) = axis_bounds(axis, 0.0, 1.0);
            let Some((positive_scale, positive_hit)) = World::intersects_aabb_with_t(
                axis_point(axis, -1.0, 0.5, 0.5),
                axis_point(axis, 2.0, 0.5, 0.5),
                min,
                max,
            ) else {
                panic!("positive Java clip fixture missed axis {axis}");
            };
            assert_eq!(positive_hit, positive_direction);
            assert_eq!(positive_scale.to_bits(), expected_scale.to_bits());

            let Some((negative_scale, negative_hit)) = World::intersects_aabb_with_t(
                axis_point(axis, 2.0, 0.5, 0.5),
                axis_point(axis, -1.0, 0.5, 0.5),
                min,
                max,
            ) else {
                panic!("negative Java clip fixture missed axis {axis}");
            };
            assert_eq!(negative_hit, negative_direction);
            assert_eq!(negative_scale.to_bits(), expected_scale.to_bits());
        }
    }

    #[test]
    fn aabb_clip_matches_java_strict_side_epsilon_in_every_axis() {
        for (axis, positive_direction) in [
            (0, Direction::West),
            (1, Direction::Down),
            (2, Direction::North),
        ] {
            let (min, max) = axis_bounds(axis, 0.0, 1.0);
            let Some((_, direction)) = World::intersects_aabb_with_t(
                axis_point(axis, -1.0, 1.0 + 0.5e-7, -0.5e-7),
                axis_point(axis, 2.0, 1.0 + 0.5e-7, -0.5e-7),
                min,
                max,
            ) else {
                panic!("Java's side epsilon should accept axis {axis}");
            };
            assert_eq!(direction, positive_direction);

            assert!(
                World::intersects_aabb_with_t(
                    axis_point(axis, -1.0, 1.0 + 1.0e-7, 0.5),
                    axis_point(axis, 2.0, 1.0 + 1.0e-7, 0.5),
                    min,
                    max,
                )
                .is_none(),
                "Java's upper side epsilon is strict on axis {axis}",
            );
            assert!(
                World::intersects_aabb_with_t(
                    axis_point(axis, -1.0, 0.5, -1.0e-7),
                    axis_point(axis, 2.0, 0.5, -1.0e-7),
                    min,
                    max,
                )
                .is_none(),
                "Java's lower side epsilon is strict on axis {axis}",
            );
        }
    }

    #[test]
    fn aabb_clip_matches_java_direction_threshold_in_every_axis() {
        let threshold = 1.0e-7_f64;
        let above_threshold = f64::from_bits(threshold.to_bits() + 1);
        for (axis, positive_direction, negative_direction) in [
            (0, Direction::West, Direction::East),
            (1, Direction::Down, Direction::Up),
            (2, Direction::North, Direction::South),
        ] {
            let exact_positive_from = axis_point(axis, -threshold / 2.0, 0.5, 0.5);
            let exact_positive_to = axis_point(axis, threshold / 2.0, 0.5, 0.5);
            assert_eq!(
                primary_component(exact_positive_to - exact_positive_from, axis).to_bits(),
                threshold.to_bits(),
            );
            let (positive_min, positive_max) = axis_bounds(axis, 0.0, 1.0);
            assert!(
                World::intersects_aabb_with_t(
                    exact_positive_from,
                    exact_positive_to,
                    positive_min,
                    positive_max,
                )
                .is_none(),
                "Java does not test a positive face at exactly +1e-7 on axis {axis}",
            );

            let above_positive_from = axis_point(axis, -above_threshold / 2.0, 0.5, 0.5);
            let above_positive_to = axis_point(axis, above_threshold / 2.0, 0.5, 0.5);
            let Some((_, direction)) = World::intersects_aabb_with_t(
                above_positive_from,
                above_positive_to,
                positive_min,
                positive_max,
            ) else {
                panic!("Java tests a positive face above +1e-7 on axis {axis}");
            };
            assert_eq!(direction, positive_direction);

            let exact_negative_from = axis_point(axis, threshold / 2.0, 0.5, 0.5);
            let exact_negative_to = axis_point(axis, -threshold / 2.0, 0.5, 0.5);
            assert_eq!(
                primary_component(exact_negative_to - exact_negative_from, axis).to_bits(),
                (-threshold).to_bits(),
            );
            let (negative_min, negative_max) = axis_bounds(axis, -1.0, 0.0);
            assert!(
                World::intersects_aabb_with_t(
                    exact_negative_from,
                    exact_negative_to,
                    negative_min,
                    negative_max,
                )
                .is_none(),
                "Java does not test a negative face at exactly -1e-7 on axis {axis}",
            );

            let above_negative_from = axis_point(axis, above_threshold / 2.0, 0.5, 0.5);
            let above_negative_to = axis_point(axis, -above_threshold / 2.0, 0.5, 0.5);
            let Some((_, direction)) = World::intersects_aabb_with_t(
                above_negative_from,
                above_negative_to,
                negative_min,
                negative_max,
            ) else {
                panic!("Java tests a negative face below -1e-7 on axis {axis}");
            };
            assert_eq!(direction, negative_direction);
        }
    }

    #[test]
    fn aabb_clip_preserves_java_axis_order_for_equal_corner_hits() {
        let Some((_, direction)) = World::intersects_aabb_with_t(
            DVec3::new(-1.0, -1.0, 0.5),
            DVec3::new(2.0, 2.0, 0.5),
            DVec3::ZERO,
            DVec3::ONE,
        ) else {
            panic!("the corner fixture should hit the full cube");
        };
        assert_eq!(direction, Direction::West);
    }
}

#[cfg(test)]
mod explosion_exposure_cache_tests {
    use super::*;
    use crate::test_support::fresh_test_world;
    use steel_registry::{
        blocks::{Block, behavior::BlockConfig},
        vanilla_blocks,
    };

    #[test]
    fn collision_path_clear_preserves_zero_axis_callback_order() {
        let from = DVec3::new(0.0, 0.25, 0.25);
        let to = DVec3::new(0.0, 2.25, 0.25);
        let start = BlockPos::new(0, 0, 0);
        let expected = [start, start, BlockPos::new(0, 1, 0), BlockPos::new(0, 2, 0)];
        let mut visited = Vec::new();

        assert!(is_collision_path_clear(from, to, |pos| {
            visited.push(pos);
            false
        }));
        assert_eq!(visited, expected);

        let mut visited = Vec::new();
        let mut start_visits = 0;
        assert!(!is_collision_path_clear(from, to, |pos| {
            visited.push(pos);
            if pos != start {
                return false;
            }
            start_visits += 1;
            start_visits == 2
        }));
        assert_eq!(visited, [start, start]);
    }

    #[test]
    fn clear_grid_covers_normal_tnt_query_without_heap_storage() {
        let mut grid = ExplosionExposureClearGrid::new();
        grid.configure(BlockPos::new(-9, 55, -9), BlockPos::new(9, 73, 9));
        assert!(grid.bounds.is_some());
        assert!(grid.index(BlockPos::new(-9, 55, -9)).is_some());
        assert!(grid.index(BlockPos::new(9, 73, 9)).is_some());
        assert!(grid.index(BlockPos::new(10, 73, 9)).is_none());
        let Some(index) = grid.index(BlockPos::new(9, 73, 9)) else {
            panic!("the configured upper corner should be represented in the grid");
        };
        grid.record(index, true);
        assert_eq!(grid.cached(index), Some(true));
        grid.clear();
        assert_eq!(grid.cached(index), None);
        grid.record(index, false);
        assert_eq!(grid.cached(index), Some(false));

        grid.configure(BlockPos::new(-10, 54, -10), BlockPos::new(10, 74, 10));
        assert!(grid.bounds.is_none());

        grid.configure(BlockPos::new(1, 1, 1), BlockPos::new(0, 0, 0));
        assert!(grid.bounds.is_none());
        grid.configure(BlockPos::new(i32::MIN, 0, 0), BlockPos::new(i32::MAX, 0, 0));
        assert!(grid.bounds.is_none());
    }

    #[test]
    fn extensible_collision_query_invalidates_every_exposure_cache() {
        static PLUGIN_BLOCK: Block = Block::new(
            Identifier::new_static("exposure_test", "mutating_shape"),
            BlockConfig::new(),
            &[],
        );

        let world = fresh_test_world("extensible_exposure_cache_boundary");
        let mut raycast =
            ExplosionExposureRaycast::new(world.as_ref(), BlockCollisionContext::empty());
        let pos = BlockPos::new(0, 64, 0);
        raycast.configure_clear_grid(pos, pos);
        let Some(grid_index) = raycast.clear_grid.index(pos) else {
            panic!("the configured position should be represented in the clear grid");
        };
        raycast.clear_grid.record(grid_index, true);
        let direct_index = explosion_exposure_cache_index(pos);
        raycast.entries[direct_index] = ExplosionExposureCacheEntry {
            pos,
            collision: OffsetVoxelShape::without_offset(VoxelShape::EMPTY),
            generation: raycast.generation,
        };

        let initial_generation = raycast.generation;
        assert!(raycast.retain_cache_after_collision_query(&vanilla_blocks::STONE));
        assert_eq!(raycast.generation, initial_generation);
        assert_eq!(raycast.clear_grid.cached(grid_index), Some(true));
        assert!(!raycast.retain_cache_after_collision_query(&PLUGIN_BLOCK));
        assert_ne!(raycast.generation, initial_generation);
        assert_eq!(raycast.clear_grid.cached(grid_index), None);
        assert_ne!(raycast.entries[direct_index].generation, raycast.generation);
    }

    #[test]
    fn cache_index_uses_the_full_fastutil_long_mix() {
        let fixtures = [
            (BlockPos::new(0, 64, 0), 94),
            (BlockPos::new(1, 64, 0), 61),
            (BlockPos::new(0, 64, 1), 14),
            (BlockPos::new(50, 200, 53), 208),
            (BlockPos::new(50, 200, 68), 48),
            (BlockPos::new(-1, 64, -1), 180),
        ];

        for (pos, expected) in fixtures {
            assert_eq!(explosion_exposure_cache_index(pos), expected);
        }

        // These positions collide under the truncated multiply-high variant.
        assert_ne!(
            explosion_exposure_cache_index(BlockPos::new(50, 200, 53)),
            explosion_exposure_cache_index(BlockPos::new(50, 200, 68)),
        );
    }
}
