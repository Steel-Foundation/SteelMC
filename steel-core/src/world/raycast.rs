use super::*;

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

impl ClipHitResult {
    /// Returns whether this clip missed all selected block and fluid shapes.
    #[must_use]
    pub const fn is_miss(self) -> bool {
        self.miss
    }
}

impl World {
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
            && local.x <= aabb.max_x()
            && local.y >= aabb.min_y()
            && local.y <= aabb.max_y()
            && local.z >= aabb.min_z()
            && local.z <= aabb.max_z()
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

    /// Ray-AABB intersection returning the entry t-parameter and the hit face.
    ///
    /// Returns `Some((tmin, direction))` where `tmin` is the ray parameter at entry
    /// and `direction` is the face normal pointing away from the hit surface.
    /// Returns `None` if the AABB is missed or entirely behind the ray origin.
    ///
    /// Used internally by [`ray_outline_check`] to pick the *closest* hit across
    /// a multi-box voxel shape, matching vanilla's `VoxelShape.clip()` behavior.
    pub(super) fn intersects_aabb_with_t(
        start: DVec3,
        end: DVec3,
        min: DVec3,
        max: DVec3,
    ) -> Option<(f64, Direction)> {
        let dir = end - start;

        let mut tmin = f64::NEG_INFINITY;
        let mut tmax = f64::INFINITY;
        let mut hit_dir = None;

        macro_rules! slab {
            ($start:expr, $dir:expr, $min:expr, $max:expr, $neg:expr, $pos:expr) => {{
                if $dir.abs() < 1e-8 {
                    if $start < $min || $start > $max {
                        return None;
                    }
                } else {
                    let inv = 1.0 / $dir;
                    let mut t1 = ($min - $start) * inv;
                    let mut t2 = ($max - $start) * inv;

                    let dir_hit = if t1 > t2 {
                        std::mem::swap(&mut t1, &mut t2);
                        $pos
                    } else {
                        $neg
                    };

                    if t1 > tmin {
                        tmin = t1;
                        hit_dir = Some(dir_hit);
                    }

                    tmax = tmax.min(t2);
                    if tmin > tmax {
                        return None;
                    }
                }
            }};
        }

        slab!(
            start.x,
            dir.x,
            min.x,
            max.x,
            Direction::West,
            Direction::East
        );
        slab!(start.y, dir.y, min.y, max.y, Direction::Down, Direction::Up);
        slab!(
            start.z,
            dir.z,
            min.z,
            max.z,
            Direction::North,
            Direction::South
        );

        if tmax < 0.0 {
            None
        } else {
            hit_dir.map(|d| (tmin, d))
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
