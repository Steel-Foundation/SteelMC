//! World collision queries for physics simulation.

use std::{ops::ControlFlow, sync::Arc};

use glam::DVec3;
use smallvec::SmallVec;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_blocks, vanilla_entities,
};
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId, WorldAabb};

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockCollisionBoxes, BlockCollisionContext, blocks::PowderSnowBlock,
};
use crate::entity::{Entity, EntityCollisionCandidates};
use crate::physics::COLLISION_EPSILON;
use crate::physics::shapes::join_is_not_empty;
use crate::world::{BlockRegionBounds, World};

const BLOCK_COLLISION_EPSILON: f64 = 1.0e-7;
const ENTITY_COLLISION_EPSILON: f64 = 1.0e-7;
// Bound the temporary snapshot to at most one full chunk section.
const MAX_PREFETCHED_COLLISION_BLOCKS: usize = 4096;

/// Trait for querying collision shapes from the world.
///
/// This abstraction allows testing physics without a full world instance.
pub trait CollisionWorld {
    /// Gets the block state at the given position.
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId;

    /// Queries all block collision shapes that intersect with the given AABB.
    ///
    /// Returns a list of world-space AABBs representing solid block collisions.
    fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb>;

    /// Returns whether any block collision shape intersects with the given AABB.
    fn has_block_collision(&self, aabb: &WorldAabb) -> bool {
        !self.get_block_collisions(aabb).is_empty()
    }

    /// Queries all block collision shapes with a vanilla collision context.
    fn get_block_collisions_with_context(
        &self,
        aabb: &WorldAabb,
        context: BlockCollisionContext,
    ) -> Vec<WorldAabb> {
        let _ = context;
        self.get_block_collisions(aabb)
    }

    /// Returns whether any block collision shape intersects with the given AABB and context.
    fn has_block_collision_with_context(
        &self,
        aabb: &WorldAabb,
        context: BlockCollisionContext,
    ) -> bool {
        !self
            .get_block_collisions_with_context(aabb, context)
            .is_empty()
    }

    /// Queries all entity collision shapes intersecting the given AABB.
    ///
    /// Path-navigation regions and test worlds use the default empty entity
    /// collision list. Live entity movement supplies these through
    /// [`WorldCollisionProvider`].
    fn get_entity_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        let _ = aabb;
        Vec::new()
    }

    /// Returns whether any entity collision shape intersects with the given AABB.
    fn has_entity_collision(&self, aabb: &WorldAabb) -> bool {
        !self.get_entity_collisions(aabb).is_empty()
    }

    /// Queries world-border collision shapes intersecting the given AABB.
    fn get_world_border_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        let _ = aabb;
        Vec::new()
    }

    /// Returns whether any world-border collision shape intersects with the given AABB.
    fn has_world_border_collision(&self, aabb: &WorldAabb) -> bool {
        !self.get_world_border_collisions(aabb).is_empty()
    }

    /// Queries entity, world-border, then block collisions with a vanilla context.
    fn get_collisions_with_context(
        &self,
        aabb: &WorldAabb,
        context: BlockCollisionContext,
    ) -> Vec<WorldAabb> {
        let mut collisions = self.get_entity_collisions(aabb);
        collisions.extend(self.get_world_border_collisions(aabb));
        collisions.extend(self.get_block_collisions_with_context(aabb, context));
        collisions
    }

    /// Returns whether any entity, world-border, or block collision shape intersects the AABB.
    fn has_collision_with_context(&self, aabb: &WorldAabb, context: BlockCollisionContext) -> bool {
        self.has_entity_collision(aabb)
            || self.has_world_border_collision(aabb)
            || self.has_block_collision_with_context(aabb, context)
    }

    /// Gets collision shapes for vanilla pre-move checks.
    ///
    /// # Arguments
    /// * `aabb` - The entity's bounding box after intended movement
    /// * `old_bottom_center` - The entity's bottom-center position before movement
    /// * `descending` - Whether the source entity is descending.
    ///
    /// # Returns
    /// Collision shapes intersecting the target box.
    ///
    /// Vanilla includes entity collisions and uses the old bottom-center Y as
    /// block collision context.
    fn get_pre_move_collisions(
        &self,
        aabb: &WorldAabb,
        old_bottom_center: DVec3,
        descending: bool,
    ) -> Vec<WorldAabb> {
        let mut collisions = self.get_entity_collisions(aabb);
        collisions.extend(self.get_block_collisions_with_context(
            aabb,
            BlockCollisionContext::pre_move(old_bottom_center.y, descending),
        ));
        collisions
    }
}

/// Implements `CollisionWorld` for the Steel World struct.
pub struct WorldCollisionProvider<'a> {
    world: &'a Arc<World>,
    source: Option<&'a dyn Entity>,
    include_entity_collisions: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BlockCollisionSearchBounds {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
}

impl BlockCollisionSearchBounds {
    fn from_aabb(aabb: &WorldAabb) -> Self {
        Self {
            min_x: (aabb.min_x() - BLOCK_COLLISION_EPSILON).floor() as i32 - 1,
            min_y: (aabb.min_y() - BLOCK_COLLISION_EPSILON).floor() as i32 - 1,
            min_z: (aabb.min_z() - BLOCK_COLLISION_EPSILON).floor() as i32 - 1,
            max_x: (aabb.max_x() + BLOCK_COLLISION_EPSILON).floor() as i32 + 1,
            max_y: (aabb.max_y() + BLOCK_COLLISION_EPSILON).floor() as i32 + 1,
            max_z: (aabb.max_z() + BLOCK_COLLISION_EPSILON).floor() as i32 + 1,
        }
    }

    fn cursor_type(self, x: i32, y: i32, z: i32) -> CollisionCursorType {
        let boundary_axis_count = u8::from(x == self.min_x || x == self.max_x)
            + u8::from(y == self.min_y || y == self.max_y)
            + u8::from(z == self.min_z || z == self.max_z);

        match boundary_axis_count {
            0 => CollisionCursorType::Inside,
            1 => CollisionCursorType::Face,
            2 => CollisionCursorType::Edge,
            _ => CollisionCursorType::Corner,
        }
    }

    const fn region_bounds(self) -> BlockRegionBounds {
        BlockRegionBounds::from_corners(
            BlockPos::new(self.min_x, self.min_y, self.min_z),
            BlockPos::new(self.max_x, self.max_y, self.max_z),
        )
    }

    fn block_count(self) -> Option<usize> {
        let width = usize::try_from(i64::from(self.max_x) - i64::from(self.min_x) + 1).ok()?;
        let height = usize::try_from(i64::from(self.max_y) - i64::from(self.min_y) + 1).ok()?;
        let depth = usize::try_from(i64::from(self.max_z) - i64::from(self.min_z) + 1).ok()?;
        width.checked_mul(height)?.checked_mul(depth)
    }

    fn try_for_each_candidate<R>(
        self,
        mut visit: impl FnMut(BlockPos, CollisionCursorType) -> ControlFlow<R>,
    ) -> ControlFlow<R> {
        // Vanilla's Cursor3D advances X first, then Y, then Z. Collision behavior can be
        // extensible, so retain that callback order even though the final shape set is usually
        // insensitive to traversal order.
        for z in self.min_z..=self.max_z {
            for y in self.min_y..=self.max_y {
                for x in self.min_x..=self.max_x {
                    let cursor_type = self.cursor_type(x, y, z);
                    if cursor_type == CollisionCursorType::Corner {
                        continue;
                    }
                    visit(BlockPos::new(x, y, z), cursor_type)?;
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn try_for_each_inside_candidate<R>(
        self,
        mut visit: impl FnMut(BlockPos, CollisionCursorType) -> ControlFlow<R>,
    ) -> ControlFlow<R> {
        let Some(min_x) = self.min_x.checked_add(1) else {
            return ControlFlow::Continue(());
        };
        let Some(min_y) = self.min_y.checked_add(1) else {
            return ControlFlow::Continue(());
        };
        let Some(min_z) = self.min_z.checked_add(1) else {
            return ControlFlow::Continue(());
        };
        let Some(max_x) = self.max_x.checked_sub(1) else {
            return ControlFlow::Continue(());
        };
        let Some(max_y) = self.max_y.checked_sub(1) else {
            return ControlFlow::Continue(());
        };
        let Some(max_z) = self.max_z.checked_sub(1) else {
            return ControlFlow::Continue(());
        };
        if min_x > max_x || min_y > max_y || min_z > max_z {
            return ControlFlow::Continue(());
        }

        for z in min_z..=max_z {
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    visit(BlockPos::new(x, y, z), CollisionCursorType::Inside)?;
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn try_for_each_region_candidate<R>(
        self,
        has_special_colliding_blocks: bool,
        visit: impl FnMut(BlockPos, CollisionCursorType) -> ControlFlow<R>,
    ) -> ControlFlow<R> {
        if has_special_colliding_blocks {
            self.try_for_each_candidate(visit)
        } else {
            self.try_for_each_inside_candidate(visit)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollisionCursorType {
    Inside,
    Face,
    Edge,
    Corner,
}

type BlockCollisionCandidate = (BlockPos, BlockStateId, CollisionCursorType);

struct CollisionShape {
    boxes: BlockCollisionBoxes,
}

fn should_resolve_collision_shape(
    block_state: BlockStateId,
    cursor_type: CollisionCursorType,
) -> bool {
    should_resolve_collision_shape_for_block(
        block_state.get_block(),
        block_state
            .get_static_collision_shape()
            .has_large_collision_shape(),
        cursor_type,
    )
}

fn should_resolve_collision_shape_for_block(
    block: BlockRef,
    has_large_static_shape: bool,
    cursor_type: CollisionCursorType,
) -> bool {
    match cursor_type {
        CollisionCursorType::Inside => true,
        CollisionCursorType::Face => {
            crate::physics::block_may_expand_collision_cursor(block, has_large_static_shape)
        }
        CollisionCursorType::Edge => block == &vanilla_blocks::MOVING_PISTON,
        CollisionCursorType::Corner => false,
    }
}

fn translate_collision_shape(shape: &BlockLocalAabb, block_pos: BlockPos) -> WorldAabb {
    shape.at_block(block_pos)
}

impl<'a> WorldCollisionProvider<'a> {
    /// Creates a new collision provider for the given world.
    pub const fn new(world: &'a Arc<World>) -> Self {
        Self {
            world,
            source: None,
            include_entity_collisions: true,
        }
    }

    /// Creates a collision provider for movement authored by `source`.
    pub const fn for_entity(world: &'a Arc<World>, source: &'a dyn Entity) -> Self {
        Self {
            world,
            source: Some(source),
            include_entity_collisions: true,
        }
    }

    /// Creates a collision provider matching vanilla `PathNavigationRegion`.
    pub const fn for_path_navigation(world: &'a Arc<World>, source: &'a dyn Entity) -> Self {
        Self {
            world,
            source: Some(source),
            include_entity_collisions: false,
        }
    }

    fn prefetched_block_collision_candidates(
        &self,
        bounds: BlockCollisionSearchBounds,
    ) -> Option<SmallVec<[BlockCollisionCandidate; 64]>> {
        if bounds
            .block_count()
            .is_none_or(|count| count > MAX_PREFETCHED_COLLISION_BLOCKS)
        {
            return None;
        }

        self.world
            .try_with_block_region(bounds.region_bounds(), |region| {
                let mut candidates = SmallVec::new();
                let has_special_colliding_blocks = region.maybe_has_special_colliding_blocks();
                let _ = bounds.try_for_each_region_candidate(
                    has_special_colliding_blocks,
                    |block_pos, cursor_type| {
                        let Some(block_state) = region.get_block_state(block_pos) else {
                            return ControlFlow::<()>::Continue(());
                        };
                        if !block_state.is_air() {
                            candidates.push((block_pos, block_state, cursor_type));
                        }
                        ControlFlow::<()>::Continue(())
                    },
                );
                candidates
            })
    }

    fn visit_block_collision_candidates<R>(
        &self,
        bounds: BlockCollisionSearchBounds,
        mut visit: impl FnMut(BlockPos, BlockStateId, CollisionCursorType) -> ControlFlow<R>,
    ) -> ControlFlow<R> {
        if let Some(candidates) = self.prefetched_block_collision_candidates(bounds) {
            for (block_pos, block_state, cursor_type) in candidates {
                visit(block_pos, block_state, cursor_type)?;
            }
            return ControlFlow::Continue(());
        }

        bounds.try_for_each_candidate(|block_pos, cursor_type| {
            let block_state = self.world.get_block_state(block_pos);
            if block_state.is_air() {
                return ControlFlow::Continue(());
            }
            visit(block_pos, block_state, cursor_type)
        })
    }

    fn get_collision_shape(
        &self,
        block_state: BlockStateId,
        block_pos: BlockPos,
        context: BlockCollisionContext,
    ) -> CollisionShape {
        let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
        let boxes =
            behavior.get_collision_boxes(block_state, self.world.as_ref(), block_pos, context);

        CollisionShape { boxes }
    }

    fn visit_block_collision_shapes<R>(
        &self,
        bounds: BlockCollisionSearchBounds,
        context: BlockCollisionContext,
        mut visit: impl FnMut(
            BlockPos,
            BlockStateId,
            CollisionCursorType,
            &CollisionShape,
        ) -> ControlFlow<R>,
    ) -> ControlFlow<R> {
        self.visit_block_collision_candidates(bounds, |block_pos, block_state, cursor_type| {
            if !should_resolve_collision_shape(block_state, cursor_type) {
                return ControlFlow::Continue(());
            }
            let collision_shape = self.get_collision_shape(block_state, block_pos, context);
            visit(block_pos, block_state, cursor_type, &collision_shape)
        })
    }

    fn entity_collision_context(
        &self,
        entity_bottom: f64,
        descending: bool,
        placement: bool,
    ) -> BlockCollisionContext {
        let context = if placement {
            BlockCollisionContext::pre_move(entity_bottom, descending)
        } else {
            BlockCollisionContext::entity(entity_bottom, descending)
        };

        if let Some(source) = self.source {
            context
                .with_fall_distance(source.fall_distance())
                .with_can_walk_on_powder_snow(PowderSnowBlock::can_entity_walk_on_powder_snow(
                    source,
                ))
                .with_falling_block(source.entity_type() == &vanilla_entities::FALLING_BLOCK)
        } else {
            context
        }
    }

    /// Returns whether an entity-context collision query intersects anything.
    ///
    /// Mirrors vanilla `Level.noCollision(entity, box)` callers by using the
    /// source entity's normal collision context rather than a source-less check.
    #[must_use]
    pub fn has_entity_context_collision(
        &self,
        aabb: WorldAabb,
        entity_bottom: f64,
        descending: bool,
    ) -> bool {
        self.has_collision_with_context(
            &aabb.deflate(COLLISION_EPSILON),
            self.entity_collision_context(entity_bottom, descending, false),
        )
    }

    /// Finds the block supporting an entity within `aabb`.
    ///
    /// Mirrors vanilla `CollisionGetter.findSupportingBlock`: among colliding
    /// blocks, choose the closest block center to the entity position, then use
    /// vanilla `BlockPos` ordering as a tie-breaker.
    #[must_use]
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla compares equal support distances exactly"
    )]
    pub fn find_supporting_block(
        &self,
        entity_position: DVec3,
        aabb: &WorldAabb,
        descending: bool,
    ) -> Option<BlockPos> {
        let bounds = BlockCollisionSearchBounds::from_aabb(aabb);
        let context = self.entity_collision_context(entity_position.y, descending, false);

        let mut main_support = None;
        let mut main_support_distance = f64::MAX;
        let _ = self.visit_block_collision_shapes(
            bounds,
            context,
            |block_pos, _block_state, _cursor_type, collision_shape| {
                if collision_shape.boxes.is_empty()
                    || !collision_shape
                        .boxes
                        .iter()
                        .map(|shape_aabb| translate_collision_shape(shape_aabb, block_pos))
                        .any(|world_aabb| aabb.intersects(world_aabb))
                {
                    return ControlFlow::<()>::Continue(());
                }

                let distance = block_pos_center_distance_sq(block_pos, entity_position);
                let should_replace = distance < main_support_distance
                    || distance == main_support_distance
                        && main_support
                            .is_none_or(|support| vanilla_block_pos_less(support, block_pos));
                if should_replace {
                    main_support = Some(block_pos);
                    main_support_distance = distance;
                }
                ControlFlow::<()>::Continue(())
            },
        );

        main_support
    }

    /// Returns vanilla `CollisionGetter.findFreePosition` for AABB-backed center shapes.
    #[must_use]
    pub fn find_free_position(
        &self,
        allowed_centers: &[WorldAabb],
        preferred_center: DVec3,
        size_x: f64,
        size_y: f64,
        size_z: f64,
    ) -> Option<DVec3> {
        let allowed_bounds = union_bounds(allowed_centers)?;
        let search_area = allowed_bounds.inflate_xyz(size_x, size_y, size_z);
        let context = self
            .source
            .map_or(BlockCollisionContext::empty(), |source| {
                self.entity_collision_context(source.position().y, source.is_descending(), false)
            });
        let world_border = self.world.world_border_snapshot();
        let expanded_collisions = self
            .get_block_collisions_with_context(&search_area, context)
            .into_iter()
            .filter(|shape| world_border.is_within_bounds(*shape))
            .map(|shape| shape.inflate_xyz(size_x / 2.0, size_y / 2.0, size_z / 2.0));

        closest_free_position(allowed_centers, preferred_center, expanded_collisions)
    }
}

fn union_bounds(boxes: &[WorldAabb]) -> Option<WorldAabb> {
    let mut boxes = boxes.iter().copied().filter(|aabb| !aabb.is_empty());
    let first = boxes.next()?;
    Some(boxes.fold(first, |bounds, aabb| {
        WorldAabb::encapsulating(&bounds, &aabb)
    }))
}

fn closest_free_position(
    allowed_centers: &[WorldAabb],
    preferred_center: DVec3,
    expanded_collisions: impl IntoIterator<Item = WorldAabb>,
) -> Option<DVec3> {
    let mut free_boxes = allowed_centers
        .iter()
        .copied()
        .filter(|aabb| !aabb.is_empty())
        .collect::<Vec<_>>();

    if free_boxes.is_empty() {
        return None;
    }

    for collision in expanded_collisions {
        if collision.is_empty() {
            continue;
        }

        let mut next_boxes = Vec::new();
        for free_box in free_boxes {
            subtract_aabb(free_box, collision, &mut next_boxes);
        }
        free_boxes = next_boxes;
        if free_boxes.is_empty() {
            return None;
        }
    }

    closest_point_to_boxes(&free_boxes, preferred_center)
}

fn subtract_aabb(free: WorldAabb, blocked: WorldAabb, output: &mut Vec<WorldAabb>) {
    if free.is_empty() {
        return;
    }

    if !free.intersects(blocked) {
        output.push(free);
        return;
    }

    let min_x = free.min_x().max(blocked.min_x());
    let max_x = free.max_x().min(blocked.max_x());
    let min_y = free.min_y().max(blocked.min_y());
    let max_y = free.max_y().min(blocked.max_y());
    let min_z = free.min_z().max(blocked.min_z());
    let max_z = free.max_z().min(blocked.max_z());

    push_non_empty_aabb(
        output,
        free.min_x(),
        free.min_y(),
        free.min_z(),
        min_x,
        free.max_y(),
        free.max_z(),
    );
    push_non_empty_aabb(
        output,
        max_x,
        free.min_y(),
        free.min_z(),
        free.max_x(),
        free.max_y(),
        free.max_z(),
    );
    push_non_empty_aabb(
        output,
        min_x,
        free.min_y(),
        free.min_z(),
        max_x,
        min_y,
        free.max_z(),
    );
    push_non_empty_aabb(
        output,
        min_x,
        max_y,
        free.min_z(),
        max_x,
        free.max_y(),
        free.max_z(),
    );
    push_non_empty_aabb(output, min_x, min_y, free.min_z(), max_x, max_y, min_z);
    push_non_empty_aabb(output, min_x, min_y, max_z, max_x, max_y, free.max_z());
}

fn push_non_empty_aabb(
    output: &mut Vec<WorldAabb>,
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
) {
    let aabb = WorldAabb::new(min_x, min_y, min_z, max_x, max_y, max_z);
    if !aabb.is_empty() {
        output.push(aabb);
    }
}

fn closest_point_to_boxes(boxes: &[WorldAabb], preferred_center: DVec3) -> Option<DVec3> {
    let mut closest = None;
    let mut closest_distance = f64::MAX;
    for aabb in boxes {
        let point = aabb.closest_point_to(preferred_center);
        let distance = point.distance_squared(preferred_center);
        if closest.is_none() || distance < closest_distance {
            closest = Some(point);
            closest_distance = distance;
        }
    }
    closest
}

fn block_pos_center_distance_sq(pos: BlockPos, point: DVec3) -> f64 {
    let dx = f64::from(pos.x()) + 0.5 - point.x;
    let dy = f64::from(pos.y()) + 0.5 - point.y;
    let dz = f64::from(pos.z()) + 0.5 - point.z;
    dx * dx + dy * dy + dz * dz
}

const fn vanilla_block_pos_less(left: BlockPos, right: BlockPos) -> bool {
    left.y() < right.y()
        || left.y() == right.y()
            && (left.z() < right.z() || left.z() == right.z() && left.x() < right.x())
}

#[must_use]
const fn bottom_center(aabb: WorldAabb) -> DVec3 {
    DVec3::new(
        f64::midpoint(aabb.min_x(), aabb.max_x()),
        aabb.min_y(),
        f64::midpoint(aabb.min_z(), aabb.max_z()),
    )
}

/// Returns whether an entity box intersects any block collision shape.
#[must_use]
pub fn has_block_collision(world: &impl CollisionWorld, aabb: WorldAabb) -> bool {
    world.has_block_collision(&aabb.deflate(COLLISION_EPSILON))
}

/// Returns whether an entity box intersects any entity or block collision shape.
#[must_use]
pub fn has_collision(world: &impl CollisionWorld, aabb: WorldAabb) -> bool {
    world.has_collision_with_context(
        &aabb.deflate(COLLISION_EPSILON),
        BlockCollisionContext::empty(),
    )
}

/// Returns whether `new_aabb` collides with shapes that `old_aabb` did not.
///
/// Matches vanilla `ServerGamePacketListenerImpl.isEntityCollidingWithAnythingNew()`.
#[must_use]
pub fn is_colliding_with_new_shapes(
    world: &impl CollisionWorld,
    old_aabb: WorldAabb,
    new_aabb: WorldAabb,
    descending: bool,
) -> bool {
    let old_shape = old_aabb.deflate(COLLISION_EPSILON);
    for collision_aabb in world.get_pre_move_collisions(
        &new_aabb.deflate(COLLISION_EPSILON),
        bottom_center(old_aabb),
        descending,
    ) {
        if !join_is_not_empty(&collision_aabb, &old_shape) {
            return true;
        }
    }

    false
}

impl CollisionWorld for WorldCollisionProvider<'_> {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.world.get_block_state(pos)
    }

    fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        self.get_block_collisions_with_context(aabb, BlockCollisionContext::empty())
    }

    fn get_block_collisions_with_context(
        &self,
        aabb: &WorldAabb,
        context: BlockCollisionContext,
    ) -> Vec<WorldAabb> {
        let bounds = BlockCollisionSearchBounds::from_aabb(aabb);
        let mut collisions = Vec::new();
        let _ = self.visit_block_collision_shapes(
            bounds,
            context,
            |block_pos, _block_state, _cursor_type, collision_shape| {
                if collision_shape.boxes.is_empty() {
                    return ControlFlow::<()>::Continue(());
                }

                collisions.extend(
                    collision_shape
                        .boxes
                        .iter()
                        .map(|shape| translate_collision_shape(shape, block_pos))
                        .filter(|shape| aabb.intersects(*shape)),
                );
                ControlFlow::<()>::Continue(())
            },
        );
        collisions
    }

    fn has_block_collision_with_context(
        &self,
        aabb: &WorldAabb,
        context: BlockCollisionContext,
    ) -> bool {
        let bounds = BlockCollisionSearchBounds::from_aabb(aabb);
        self.visit_block_collision_shapes(
            bounds,
            context,
            |block_pos, _block_state, _cursor_type, collision_shape| {
                if collision_shape.boxes.is_empty() {
                    return ControlFlow::Continue(());
                }

                if collision_shape
                    .boxes
                    .iter()
                    .map(|shape| translate_collision_shape(shape, block_pos))
                    .any(|shape| aabb.intersects(shape))
                {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        )
        .is_break()
    }

    fn get_pre_move_collisions(
        &self,
        aabb: &WorldAabb,
        old_bottom_center: DVec3,
        descending: bool,
    ) -> Vec<WorldAabb> {
        let mut collisions = self.get_entity_collisions(aabb);
        collisions.extend(self.get_block_collisions_with_context(
            aabb,
            self.entity_collision_context(old_bottom_center.y, descending, true),
        ));
        collisions
    }

    fn get_entity_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        if !self.include_entity_collisions {
            return Vec::new();
        }
        if aabb.size() < ENTITY_COLLISION_EPSILON {
            return Vec::new();
        }

        let query = aabb.inflate(ENTITY_COLLISION_EPSILON);
        let candidates = EntityCollisionCandidates::for_source(self.source);
        self.world
            .get_movement_collision_boxes_in_aabb_matching(&query, candidates, |entity| match self
                .source
            {
                Some(source) => {
                    entity.id() != source.id()
                        && !entity.is_removed()
                        && !entity.is_spectator()
                        && source.can_collide_with(entity)
                }
                None => {
                    !entity.is_removed()
                        && !entity.is_spectator()
                        && entity.can_be_collided_with(None)
                }
            })
    }

    fn has_entity_collision(&self, aabb: &WorldAabb) -> bool {
        if !self.include_entity_collisions {
            return false;
        }
        if aabb.size() < ENTITY_COLLISION_EPSILON {
            return false;
        }

        let query = aabb.inflate(ENTITY_COLLISION_EPSILON);
        let candidates = EntityCollisionCandidates::for_source(self.source);
        self.world
            .has_movement_collision_in_aabb_matching(&query, candidates, |entity| {
                match self.source {
                    Some(source) => {
                        entity.id() != source.id()
                            && !entity.is_removed()
                            && !entity.is_spectator()
                            && source.can_collide_with(entity)
                    }
                    None => {
                        !entity.is_removed()
                            && !entity.is_spectator()
                            && entity.can_be_collided_with(None)
                    }
                }
            })
    }

    fn get_world_border_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        let Some(source) = self.source else {
            return Vec::new();
        };

        let border = self.world.world_border_snapshot();
        let source_position = source.position();
        if !border.is_inside_close_to_border(source_position.x, source_position.z, *aabb) {
            return Vec::new();
        }

        border.collision_shapes_for(*aabb)
    }

    fn has_world_border_collision(&self, aabb: &WorldAabb) -> bool {
        let Some(source) = self.source else {
            return false;
        };

        let border = self.world.world_border_snapshot();
        let source_position = source.position();
        border.is_inside_close_to_border(source_position.x, source_position.z, *aabb)
            && !border.collision_shapes_for(*aabb).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;
    use steel_registry::{
        blocks::{Block, behavior::BlockConfig},
        init_vanilla_registry,
    };
    use steel_utils::{ChunkPos, Identifier, types::UpdateFlags};

    use crate::{
        behavior::init_behaviors,
        test_support::{fresh_test_world, insert_ready_full_chunk},
    };

    struct TestCollisionWorld {
        block_collisions: Vec<WorldAabb>,
        entity_collisions: Vec<WorldAabb>,
        pre_move_collisions: Vec<WorldAabb>,
    }

    struct BorderPreMoveWorld {
        entity_collisions: Vec<WorldAabb>,
        border_collisions: Vec<WorldAabb>,
    }

    impl CollisionWorld for TestCollisionWorld {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            vanilla_blocks::AIR.default_state()
        }

        fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            self.block_collisions
                .iter()
                .copied()
                .filter(|collision| collision.intersects(*aabb))
                .collect()
        }

        fn get_entity_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            self.entity_collisions
                .iter()
                .copied()
                .filter(|collision| collision.intersects(*aabb))
                .collect()
        }

        fn get_pre_move_collisions(
            &self,
            _aabb: &WorldAabb,
            _old_bottom_center: DVec3,
            _descending: bool,
        ) -> Vec<WorldAabb> {
            self.pre_move_collisions.clone()
        }
    }

    impl CollisionWorld for BorderPreMoveWorld {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            vanilla_blocks::AIR.default_state()
        }

        fn get_block_collisions(&self, _aabb: &WorldAabb) -> Vec<WorldAabb> {
            Vec::new()
        }

        fn get_entity_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            self.entity_collisions
                .iter()
                .copied()
                .filter(|collision| collision.intersects(*aabb))
                .collect()
        }

        fn get_world_border_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            self.border_collisions
                .iter()
                .copied()
                .filter(|collision| collision.intersects(*aabb))
                .collect()
        }
    }

    #[test]
    fn test_intersects_aabb() {
        let aabb1 = WorldAabb::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0);
        let aabb2 = WorldAabb::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0);

        assert!(aabb1.intersects(aabb2));

        let aabb3 = WorldAabb::new(5.0, 5.0, 5.0, 6.0, 6.0, 6.0);

        assert!(!aabb1.intersects(aabb3));
    }

    #[test]
    fn closest_free_position_returns_preferred_center_without_collisions() {
        let allowed = [WorldAabb::new(0.0, 0.0, 0.0, 4.0, 1.0, 1.0)];
        let preferred = DVec3::new(2.0, 0.5, 0.5);

        assert_eq!(
            closest_free_position(&allowed, preferred, iter::empty()),
            Some(preferred)
        );
    }

    #[test]
    fn closest_free_position_excludes_expanded_collisions() {
        let allowed = [WorldAabb::new(0.0, 0.0, 0.0, 4.0, 1.0, 1.0)];
        let collision = WorldAabb::new(0.0, -1.0, -1.0, 3.0, 2.0, 2.0);

        assert_eq!(
            closest_free_position(&allowed, DVec3::new(1.5, 0.5, 0.5), [collision].into_iter()),
            Some(DVec3::new(3.0, 0.5, 0.5))
        );
    }

    #[test]
    fn closest_free_position_returns_none_when_fully_blocked() {
        let allowed = [WorldAabb::new(0.0, 0.0, 0.0, 4.0, 1.0, 1.0)];
        let collision = WorldAabb::new(-1.0, -1.0, -1.0, 5.0, 2.0, 2.0);

        assert_eq!(
            closest_free_position(&allowed, DVec3::new(1.5, 0.5, 0.5), [collision].into_iter()),
            None
        );
    }

    #[test]
    fn block_collision_helper_reports_intersecting_collision_shape() {
        let world = TestCollisionWorld {
            block_collisions: vec![WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)],
            entity_collisions: Vec::new(),
            pre_move_collisions: Vec::new(),
        };

        assert!(has_block_collision(
            &world,
            WorldAabb::new(0.25, 0.25, 0.25, 0.75, 0.75, 0.75)
        ));
        assert!(!has_block_collision(
            &world,
            WorldAabb::new(2.0, 2.0, 2.0, 3.0, 3.0, 3.0)
        ));
    }

    #[test]
    fn live_block_collisions_use_bounded_region_reads() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("bounded_collision_reads");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let block_pos = BlockPos::new(0, 64, 0);
        assert!(world.set_block(
            block_pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let collisions = WorldCollisionProvider::new(&world)
            .get_block_collisions(&WorldAabb::new(0.25, 64.0, 0.25, 0.75, 65.0, 0.75));

        assert!(collisions.contains(&WorldAabb::new(0.0, 64.0, 0.0, 1.0, 65.0, 1.0)));
    }

    #[test]
    fn prefetched_candidates_use_full_cursor_only_for_special_sections() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("special_collision_section_scan");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let bounds = BlockCollisionSearchBounds {
            min_x: 1,
            min_y: 64,
            min_z: 1,
            max_x: 3,
            max_y: 66,
            max_z: 3,
        };
        let face = BlockPos::new(1, 65, 2);
        let inside = BlockPos::new(2, 65, 2);
        assert!(world.set_block(
            face,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            inside,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let provider = WorldCollisionProvider::new(&world);

        let ordinary = provider
            .prefetched_block_collision_candidates(bounds)
            .expect("small loaded collision region should be prefetched");
        assert_eq!(
            ordinary
                .iter()
                .map(|(pos, _, cursor_type)| (*pos, *cursor_type))
                .collect::<Vec<_>>(),
            vec![(inside, CollisionCursorType::Inside)]
        );
        let mut ordinary_shape_callbacks = Vec::new();
        let _ = provider.visit_block_collision_shapes(
            bounds,
            BlockCollisionContext::empty(),
            |pos, _, cursor_type, _| {
                ordinary_shape_callbacks.push((pos, cursor_type));
                ControlFlow::<()>::Continue(())
            },
        );
        assert_eq!(
            ordinary_shape_callbacks,
            vec![(inside, CollisionCursorType::Inside)]
        );

        assert!(world.set_block(
            BlockPos::new(15, 64, 15),
            vanilla_blocks::OAK_FENCE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let special = provider
            .prefetched_block_collision_candidates(bounds)
            .expect("small loaded collision region should be prefetched");
        assert_eq!(
            special
                .iter()
                .map(|(pos, _, cursor_type)| (*pos, *cursor_type))
                .collect::<Vec<_>>(),
            vec![
                (face, CollisionCursorType::Face),
                (inside, CollisionCursorType::Inside),
            ]
        );
        let mut static_shape_callbacks = Vec::new();
        let _ = provider.visit_block_collision_shapes(
            bounds,
            BlockCollisionContext::empty(),
            |pos, _, cursor_type, _| {
                static_shape_callbacks.push((pos, cursor_type));
                ControlFlow::<()>::Continue(())
            },
        );
        assert_eq!(
            static_shape_callbacks,
            vec![(inside, CollisionCursorType::Inside)]
        );

        assert!(world.set_block(
            BlockPos::new(15, 64, 15),
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            face,
            vanilla_blocks::SCAFFOLDING.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let edge = BlockPos::new(1, 64, 2);
        assert!(world.set_block(
            edge,
            vanilla_blocks::MOVING_PISTON.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let dynamic = provider
            .prefetched_block_collision_candidates(bounds)
            .expect("small loaded collision region should be prefetched");
        assert_eq!(dynamic.len(), 3);
        let mut callback_sequence = Vec::new();
        let _ = provider.visit_block_collision_shapes(
            bounds,
            BlockCollisionContext::empty(),
            |pos, _, cursor_type, _| {
                callback_sequence.push((pos, cursor_type));
                ControlFlow::<()>::Continue(())
            },
        );
        assert_eq!(
            callback_sequence,
            vec![
                (edge, CollisionCursorType::Edge),
                (face, CollisionCursorType::Face),
                (inside, CollisionCursorType::Inside),
            ]
        );
    }

    #[test]
    fn prefetched_callbacks_match_full_cursor_across_chunk_section_and_missing_data() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("cross_boundary_collision_reads");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        insert_ready_full_chunk(&world, ChunkPos::new(1, 0));

        let bounds = BlockCollisionSearchBounds {
            min_x: 14,
            min_y: 78,
            min_z: 0,
            max_x: 33,
            max_y: 81,
            max_z: 2,
        };
        for pos in [
            BlockPos::new(14, 79, 1),
            BlockPos::new(15, 79, 1),
            BlockPos::new(16, 80, 1),
            BlockPos::new(31, 79, 1),
        ] {
            assert!(world.set_block(
                pos,
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_NONE,
            ));
        }
        let provider = WorldCollisionProvider::new(&world);

        let compatibility_callbacks = || {
            let mut callbacks = Vec::new();
            let _ = bounds.try_for_each_candidate(|pos, cursor_type| {
                let state = world.get_block_state(pos);
                if !state.is_air() && should_resolve_collision_shape(state, cursor_type) {
                    callbacks.push((pos, cursor_type));
                }
                ControlFlow::<()>::Continue(())
            });
            callbacks
        };
        let provider_callbacks = || {
            let mut callbacks = Vec::new();
            let _ = provider.visit_block_collision_shapes(
                bounds,
                BlockCollisionContext::empty(),
                |pos, _, cursor_type, _| {
                    callbacks.push((pos, cursor_type));
                    ControlFlow::<()>::Continue(())
                },
            );
            callbacks
        };

        assert!(
            provider
                .prefetched_block_collision_candidates(bounds)
                .is_some(),
            "missing Full data should remain a safe air-like prefetched region"
        );
        assert_eq!(provider_callbacks(), compatibility_callbacks());

        assert!(world.set_block(
            BlockPos::new(31, 79, 1),
            vanilla_blocks::OAK_FENCE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert_eq!(provider_callbacks(), compatibility_callbacks());
    }

    #[test]
    fn collision_helper_reports_intersecting_entity_shape() {
        let world = TestCollisionWorld {
            block_collisions: Vec::new(),
            entity_collisions: vec![WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)],
            pre_move_collisions: Vec::new(),
        };

        assert!(has_collision(
            &world,
            WorldAabb::new(0.25, 0.25, 0.25, 0.75, 0.75, 0.75)
        ));
        assert!(!has_block_collision(
            &world,
            WorldAabb::new(0.25, 0.25, 0.25, 0.75, 0.75, 0.75)
        ));
    }

    #[test]
    fn new_shape_collision_helper_ignores_collision_already_touching_old_box() {
        let already_overlapped = WorldAabb::new(0.25, 0.0, 0.25, 0.75, 1.0, 0.75);
        let new_collision = WorldAabb::new(2.0, 0.0, 0.0, 3.0, 1.0, 1.0);
        let old_aabb = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let new_aabb = WorldAabb::new(2.0, 0.0, 0.0, 3.0, 1.0, 1.0);

        let already_stuck_world = TestCollisionWorld {
            block_collisions: Vec::new(),
            entity_collisions: Vec::new(),
            pre_move_collisions: vec![already_overlapped],
        };
        assert!(!is_colliding_with_new_shapes(
            &already_stuck_world,
            old_aabb,
            new_aabb,
            false
        ));

        let newly_blocked_world = TestCollisionWorld {
            block_collisions: Vec::new(),
            entity_collisions: Vec::new(),
            pre_move_collisions: vec![new_collision],
        };
        assert!(is_colliding_with_new_shapes(
            &newly_blocked_world,
            old_aabb,
            new_aabb,
            false
        ));
    }

    #[test]
    fn pre_move_collisions_exclude_world_border_collisions() {
        let entity_collision = WorldAabb::new(0.25, 0.0, 0.25, 0.75, 1.0, 0.75);
        let border_collision = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let world = BorderPreMoveWorld {
            entity_collisions: vec![entity_collision],
            border_collisions: vec![border_collision],
        };
        let aabb = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

        assert_eq!(
            world.get_pre_move_collisions(&aabb, DVec3::ZERO, false),
            vec![entity_collision]
        );
        assert!(world.has_world_border_collision(&aabb));
    }

    #[test]
    fn supporting_block_tie_breaker_matches_vanilla_ordering() {
        assert!(vanilla_block_pos_less(
            BlockPos::new(0, 0, 0),
            BlockPos::new(0, 1, 0)
        ));
        assert!(vanilla_block_pos_less(
            BlockPos::new(0, 1, 0),
            BlockPos::new(0, 1, 1)
        ));
        assert!(vanilla_block_pos_less(
            BlockPos::new(0, 1, 1),
            BlockPos::new(1, 1, 1)
        ));
        assert!(!vanilla_block_pos_less(
            BlockPos::new(1, 1, 1),
            BlockPos::new(0, 1, 1)
        ));
    }

    #[test]
    fn supporting_block_distance_uses_block_center() {
        let distance =
            block_pos_center_distance_sq(BlockPos::new(1, 2, 3), DVec3::new(1.5, 1.5, 5.5));

        assert!((distance - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn block_collision_search_bounds_match_vanilla_epsilon_range() {
        let bounds =
            BlockCollisionSearchBounds::from_aabb(&WorldAabb::new(0.0, 0.25, 0.0, 1.0, 1.0, 1.0));

        assert_eq!(bounds.min_x, -2);
        assert_eq!(bounds.max_x, 2);
        assert_eq!(bounds.min_y, -1);
        assert_eq!(bounds.max_y, 2);
        assert_eq!(bounds.min_z, -2);
        assert_eq!(bounds.max_z, 2);
    }

    #[test]
    fn collision_cursor_type_matches_vanilla_boundary_count() {
        let bounds = BlockCollisionSearchBounds::from_aabb(&WorldAabb::new(
            0.25, 0.25, 0.25, 0.75, 0.75, 0.75,
        ));

        assert_eq!(bounds.cursor_type(0, 0, 0), CollisionCursorType::Inside);
        assert_eq!(
            bounds.cursor_type(bounds.min_x, 0, 0),
            CollisionCursorType::Face
        );
        assert_eq!(
            bounds.cursor_type(bounds.min_x, bounds.min_y, 0),
            CollisionCursorType::Edge
        );
        assert_eq!(
            bounds.cursor_type(bounds.min_x, bounds.min_y, bounds.min_z),
            CollisionCursorType::Corner
        );
    }

    #[test]
    fn collision_cursor_callbacks_match_vanilla_x_y_z_order() {
        let bounds = BlockCollisionSearchBounds {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 2,
            max_y: 2,
            max_z: 2,
        };
        let mut visited = Vec::new();
        let _ = bounds.try_for_each_candidate(|pos, _| {
            visited.push(pos);
            ControlFlow::<()>::Continue(())
        });

        assert_eq!(
            visited,
            vec![
                BlockPos::new(1, 0, 0),
                BlockPos::new(0, 1, 0),
                BlockPos::new(1, 1, 0),
                BlockPos::new(2, 1, 0),
                BlockPos::new(1, 2, 0),
                BlockPos::new(0, 0, 1),
                BlockPos::new(1, 0, 1),
                BlockPos::new(2, 0, 1),
                BlockPos::new(0, 1, 1),
                BlockPos::new(1, 1, 1),
                BlockPos::new(2, 1, 1),
                BlockPos::new(0, 2, 1),
                BlockPos::new(1, 2, 1),
                BlockPos::new(2, 2, 1),
                BlockPos::new(1, 0, 2),
                BlockPos::new(0, 1, 2),
                BlockPos::new(1, 1, 2),
                BlockPos::new(2, 1, 2),
                BlockPos::new(1, 2, 2),
            ]
        );
    }

    #[test]
    fn ordinary_cursor_handles_degenerate_and_extreme_bounds() {
        let thin = BlockCollisionSearchBounds {
            min_x: 4,
            min_y: 8,
            min_z: 12,
            max_x: 5,
            max_y: 10,
            max_z: 14,
        };
        let mut thin_visited = Vec::new();
        let _ = thin.try_for_each_region_candidate(false, |pos, cursor_type| {
            thin_visited.push((pos, cursor_type));
            ControlFlow::<()>::Continue(())
        });
        assert!(thin_visited.is_empty());

        for bounds in [
            BlockCollisionSearchBounds {
                min_x: i32::MIN,
                min_y: i32::MIN,
                min_z: i32::MIN,
                max_x: i32::MIN + 2,
                max_y: i32::MIN + 2,
                max_z: i32::MIN + 2,
            },
            BlockCollisionSearchBounds {
                min_x: i32::MAX - 2,
                min_y: i32::MAX - 2,
                min_z: i32::MAX - 2,
                max_x: i32::MAX,
                max_y: i32::MAX,
                max_z: i32::MAX,
            },
        ] {
            let mut compatibility = Vec::new();
            let _ = bounds.try_for_each_candidate(|pos, cursor_type| {
                if cursor_type == CollisionCursorType::Inside {
                    compatibility.push((pos, cursor_type));
                }
                ControlFlow::<()>::Continue(())
            });
            let mut optimized = Vec::new();
            let _ = bounds.try_for_each_region_candidate(false, |pos, cursor_type| {
                optimized.push((pos, cursor_type));
                ControlFlow::<()>::Continue(())
            });
            assert_eq!(optimized, compatibility);
        }
    }

    #[test]
    fn ordinary_section_candidate_and_callback_sequences_match_compatibility_cursor() {
        init_vanilla_registry();

        for seed in 1_u64..=128 {
            let mut random = seed;
            let mut next = || {
                random = random
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                random
            };
            let min_x = (next() & 31) as i32 - 16;
            let min_y = (next() & 31) as i32 - 16;
            let min_z = (next() & 31) as i32 - 16;
            let bounds = BlockCollisionSearchBounds {
                min_x,
                min_y,
                min_z,
                max_x: min_x + 2 + (next() & 7) as i32,
                max_y: min_y + 2 + (next() & 7) as i32,
                max_z: min_z + 2 + (next() & 7) as i32,
            };
            let occupancy_seed = next();
            let is_occupied = |pos: BlockPos| {
                let mixed = (pos.x() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    ^ (pos.y() as u64).rotate_left(21)
                    ^ (pos.z() as u64).rotate_left(43)
                    ^ occupancy_seed;
                mixed.wrapping_mul(0xbf58_476d_1ce4_e5b9) >> 61 != 0
            };

            let mut compatibility_candidates = Vec::new();
            let _ = bounds.try_for_each_candidate(|pos, cursor_type| {
                if cursor_type == CollisionCursorType::Inside && is_occupied(pos) {
                    compatibility_candidates.push((pos, cursor_type));
                }
                ControlFlow::<()>::Continue(())
            });
            let mut optimized_candidates = Vec::new();
            let _ = bounds.try_for_each_region_candidate(false, |pos, cursor_type| {
                if is_occupied(pos) {
                    optimized_candidates.push((pos, cursor_type));
                }
                ControlFlow::<()>::Continue(())
            });
            assert_eq!(
                optimized_candidates, compatibility_candidates,
                "seed={seed}"
            );

            let stone = vanilla_blocks::STONE.default_state();
            let mut compatibility_callbacks = Vec::new();
            let _ = bounds.try_for_each_candidate(|pos, cursor_type| {
                if is_occupied(pos) && should_resolve_collision_shape(stone, cursor_type) {
                    compatibility_callbacks.push(pos);
                }
                ControlFlow::<()>::Continue(())
            });
            let optimized_callbacks = optimized_candidates
                .iter()
                .map(|(pos, _)| *pos)
                .collect::<Vec<_>>();
            assert_eq!(optimized_callbacks, compatibility_callbacks, "seed={seed}");
        }
    }

    #[test]
    fn special_section_candidate_sequence_uses_full_compatibility_cursor() {
        let bounds = BlockCollisionSearchBounds {
            min_x: -2,
            min_y: 4,
            min_z: 7,
            max_x: 2,
            max_y: 8,
            max_z: 11,
        };
        let mut compatibility = Vec::new();
        let _ = bounds.try_for_each_candidate(|pos, cursor_type| {
            compatibility.push((pos, cursor_type));
            ControlFlow::<()>::Continue(())
        });
        let mut optimized = Vec::new();
        let _ = bounds.try_for_each_region_candidate(true, |pos, cursor_type| {
            optimized.push((pos, cursor_type));
            ControlFlow::<()>::Continue(())
        });

        assert_eq!(optimized, compatibility);
    }

    #[test]
    fn static_looking_plugin_block_keeps_full_cursor_callback_order() {
        static PLUGIN_BLOCK: Block = Block::new(
            Identifier::new_static("collision_test", "static_cube"),
            BlockConfig::new(),
            &[],
        );

        assert!(!PLUGIN_BLOCK.config.dynamic_shape);
        let has_large_static_shape = false;
        let has_special_colliding_blocks = crate::physics::block_may_expand_collision_cursor(
            &PLUGIN_BLOCK,
            has_large_static_shape,
        );
        assert!(has_special_colliding_blocks);

        let bounds = BlockCollisionSearchBounds {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 2,
            max_y: 2,
            max_z: 2,
        };
        let fixture_positions = [
            BlockPos::new(1, 1, 0),
            BlockPos::new(0, 1, 1),
            BlockPos::new(1, 1, 1),
            BlockPos::new(2, 1, 1),
            BlockPos::new(1, 1, 2),
        ];

        let mut compatibility_callbacks = Vec::new();
        let _ = bounds.try_for_each_candidate(|pos, cursor_type| {
            if fixture_positions.contains(&pos)
                && should_resolve_collision_shape_for_block(
                    &PLUGIN_BLOCK,
                    has_large_static_shape,
                    cursor_type,
                )
            {
                compatibility_callbacks.push(pos);
            }
            ControlFlow::<()>::Continue(())
        });

        let mut optimized_callbacks = Vec::new();
        let _ = bounds.try_for_each_region_candidate(
            has_special_colliding_blocks,
            |pos, cursor_type| {
                if fixture_positions.contains(&pos)
                    && should_resolve_collision_shape_for_block(
                        &PLUGIN_BLOCK,
                        has_large_static_shape,
                        cursor_type,
                    )
                {
                    optimized_callbacks.push(pos);
                }
                ControlFlow::<()>::Continue(())
            },
        );

        assert_eq!(compatibility_callbacks, fixture_positions);
        assert_eq!(optimized_callbacks, compatibility_callbacks);
    }

    #[test]
    fn collision_shape_resolution_filter_matches_vanilla_cursor_rules() {
        init_vanilla_registry();

        let stone = vanilla_blocks::STONE.default_state();
        let fence = vanilla_blocks::OAK_FENCE.default_state();
        let scaffolding = vanilla_blocks::SCAFFOLDING.default_state();
        let moving_piston = vanilla_blocks::MOVING_PISTON.default_state();

        assert!(should_resolve_collision_shape(
            stone,
            CollisionCursorType::Inside
        ));
        assert!(!should_resolve_collision_shape(
            stone,
            CollisionCursorType::Face
        ));
        assert!(should_resolve_collision_shape(
            fence,
            CollisionCursorType::Face
        ));
        assert!(should_resolve_collision_shape(
            scaffolding,
            CollisionCursorType::Face
        ));
        assert!(!should_resolve_collision_shape(
            fence,
            CollisionCursorType::Edge
        ));
        assert!(should_resolve_collision_shape(
            moving_piston,
            CollisionCursorType::Edge
        ));
        assert!(!should_resolve_collision_shape(
            moving_piston,
            CollisionCursorType::Corner
        ));
    }
}
