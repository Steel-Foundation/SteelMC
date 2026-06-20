//! World collision queries for physics simulation.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::{
    blocks::{
        block_state_ext::BlockStateExt,
        shapes::{OffsetVoxelShape, VoxelShape},
    },
    vanilla_blocks, vanilla_entities,
};
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId, WorldAabb};

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::entity::Entity;
use crate::physics::COLLISION_EPSILON;
use crate::physics::shapes::join_is_not_empty;
use crate::world::World;

const BLOCK_COLLISION_EPSILON: f64 = 1.0e-7;
const ENTITY_COLLISION_EPSILON: f64 = 1.0e-7;

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

    /// Queries entity collisions followed by block collisions with a vanilla context.
    fn get_collisions_with_context(
        &self,
        aabb: &WorldAabb,
        context: BlockCollisionContext,
    ) -> Vec<WorldAabb> {
        let mut collisions = self.get_entity_collisions(aabb);
        collisions.extend(self.get_block_collisions_with_context(aabb, context));
        collisions
    }

    /// Returns whether any entity or block collision shape intersects with the given AABB.
    fn has_collision_with_context(&self, aabb: &WorldAabb, context: BlockCollisionContext) -> bool {
        self.has_entity_collision(aabb) || self.has_block_collision_with_context(aabb, context)
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
        self.get_collisions_with_context(
            aabb,
            BlockCollisionContext::pre_move(old_bottom_center.y, descending),
        )
    }
}

/// Implements `CollisionWorld` for the Steel World struct.
pub struct WorldCollisionProvider<'a> {
    world: &'a Arc<World>,
    source: Option<&'a dyn Entity>,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollisionCursorType {
    Inside,
    Face,
    Edge,
    Corner,
}

#[derive(Clone, Copy)]
struct CollisionShape {
    shape: VoxelShape,
    offset: DVec3,
}

impl CollisionShape {
    fn has_large_collision_shape(self) -> bool {
        OffsetVoxelShape::new(self.shape, self.offset).has_large_collision_shape()
    }
}

fn should_query_collision_shape(
    block_state: BlockStateId,
    collision_shape: CollisionShape,
    cursor_type: CollisionCursorType,
) -> bool {
    match cursor_type {
        CollisionCursorType::Inside => true,
        CollisionCursorType::Face => {
            block_state.get_block().config.dynamic_shape
                || collision_shape.has_large_collision_shape()
        }
        CollisionCursorType::Edge => block_state.get_block() == &vanilla_blocks::MOVING_PISTON,
        CollisionCursorType::Corner => false,
    }
}

fn translate_collision_shape(
    shape: &BlockLocalAabb,
    block_pos: BlockPos,
    offset: DVec3,
) -> WorldAabb {
    shape.translate(offset).at_block(block_pos)
}

impl<'a> WorldCollisionProvider<'a> {
    /// Creates a new collision provider for the given world.
    pub const fn new(world: &'a Arc<World>) -> Self {
        Self {
            world,
            source: None,
        }
    }

    /// Creates a collision provider for movement authored by `source`.
    pub const fn for_entity(world: &'a Arc<World>, source: &'a dyn Entity) -> Self {
        Self {
            world,
            source: Some(source),
        }
    }

    fn get_collision_shape(
        &self,
        block_state: BlockStateId,
        block_pos: BlockPos,
        context: BlockCollisionContext,
    ) -> CollisionShape {
        let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
        let shape =
            behavior.get_collision_shape(block_state, self.world.as_ref(), block_pos, context);
        let offset = if shape.is_empty() {
            DVec3::ZERO
        } else {
            behavior.get_collision_shape_offset(
                block_state,
                self.world.as_ref(),
                block_pos,
                context,
            )
        };

        CollisionShape { shape, offset }
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
                .with_can_walk_on_powder_snow(source.can_walk_on_powder_snow())
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

        for y in bounds.min_y..=bounds.max_y {
            for z in bounds.min_z..=bounds.max_z {
                for x in bounds.min_x..=bounds.max_x {
                    let cursor_type = bounds.cursor_type(x, y, z);
                    if cursor_type == CollisionCursorType::Corner {
                        continue;
                    }

                    let block_pos = BlockPos::new(x, y, z);
                    let block_state = self.world.get_block_state(block_pos);
                    if block_state.is_air() {
                        continue;
                    }

                    let collision_shape = self.get_collision_shape(block_state, block_pos, context);
                    if collision_shape.shape.is_empty() {
                        continue;
                    }
                    if !should_query_collision_shape(block_state, collision_shape, cursor_type) {
                        continue;
                    }

                    let supports_entity = collision_shape
                        .shape
                        .into_iter()
                        .map(|shape_aabb| {
                            translate_collision_shape(shape_aabb, block_pos, collision_shape.offset)
                        })
                        .any(|world_aabb| aabb.intersects(world_aabb));
                    if !supports_entity {
                        continue;
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
                }
            }
        }

        main_support
    }
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
        let mut collisions = Vec::new();

        let bounds = BlockCollisionSearchBounds::from_aabb(aabb);

        for y in bounds.min_y..=bounds.max_y {
            for z in bounds.min_z..=bounds.max_z {
                for x in bounds.min_x..=bounds.max_x {
                    let cursor_type = bounds.cursor_type(x, y, z);
                    if cursor_type == CollisionCursorType::Corner {
                        continue;
                    }

                    let block_pos = BlockPos::new(x, y, z);
                    let block_state = self.world.get_block_state(block_pos);

                    if block_state.is_air() {
                        continue;
                    }

                    let collision_shape = self.get_collision_shape(block_state, block_pos, context);

                    if collision_shape.shape.is_empty() {
                        continue;
                    }
                    if !should_query_collision_shape(block_state, collision_shape, cursor_type) {
                        continue;
                    }

                    for shape_aabb in collision_shape.shape {
                        let world_aabb = translate_collision_shape(
                            shape_aabb,
                            block_pos,
                            collision_shape.offset,
                        );

                        if aabb.intersects(world_aabb) {
                            collisions.push(world_aabb);
                        }
                    }
                }
            }
        }

        collisions
    }

    fn has_block_collision_with_context(
        &self,
        aabb: &WorldAabb,
        context: BlockCollisionContext,
    ) -> bool {
        let bounds = BlockCollisionSearchBounds::from_aabb(aabb);

        for y in bounds.min_y..=bounds.max_y {
            for z in bounds.min_z..=bounds.max_z {
                for x in bounds.min_x..=bounds.max_x {
                    let cursor_type = bounds.cursor_type(x, y, z);
                    if cursor_type == CollisionCursorType::Corner {
                        continue;
                    }

                    let block_pos = BlockPos::new(x, y, z);
                    let block_state = self.world.get_block_state(block_pos);

                    if block_state.is_air() {
                        continue;
                    }

                    let collision_shape = self.get_collision_shape(block_state, block_pos, context);

                    if collision_shape.shape.is_empty() {
                        continue;
                    }
                    if !should_query_collision_shape(block_state, collision_shape, cursor_type) {
                        continue;
                    }

                    for shape_aabb in collision_shape.shape {
                        let world_aabb = translate_collision_shape(
                            shape_aabb,
                            block_pos,
                            collision_shape.offset,
                        );

                        if aabb.intersects(world_aabb) {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    fn get_pre_move_collisions(
        &self,
        aabb: &WorldAabb,
        old_bottom_center: DVec3,
        descending: bool,
    ) -> Vec<WorldAabb> {
        self.get_collisions_with_context(
            aabb,
            self.entity_collision_context(old_bottom_center.y, descending, true),
        )
    }

    fn get_entity_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        if aabb.size() < ENTITY_COLLISION_EPSILON {
            return Vec::new();
        }

        let query = aabb.inflate(ENTITY_COLLISION_EPSILON);
        self.world
            .get_entities_in_aabb(&query)
            .into_iter()
            .filter(|entity| !entity.is_removed())
            .filter(|entity| match self.source {
                Some(source) => {
                    entity.id() != source.id()
                        && !entity.is_spectator()
                        && source.can_collide_with(entity.as_ref())
                }
                None => !entity.is_spectator() && entity.can_be_collided_with(None),
            })
            .map(|entity| entity.bounding_box())
            .collect()
    }

    fn has_entity_collision(&self, aabb: &WorldAabb) -> bool {
        if aabb.size() < ENTITY_COLLISION_EPSILON {
            return false;
        }

        let query = aabb.inflate(ENTITY_COLLISION_EPSILON);
        self.world
            .get_entities_in_aabb(&query)
            .into_iter()
            .any(|entity| {
                !entity.is_removed()
                    && match self.source {
                        Some(source) => {
                            entity.id() != source.id()
                                && !entity.is_spectator()
                                && source.can_collide_with(entity.as_ref())
                        }
                        None => !entity.is_spectator() && entity.can_be_collided_with(None),
                    }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::test_support;
    use steel_utils::BlockLocalAabb;

    const LARGE_COLLISION_SHAPE: &[BlockLocalAabb] =
        &[BlockLocalAabb::new(-0.25, 0.0, 0.0, 1.0, 1.0, 1.0)];

    struct TestCollisionWorld {
        block_collisions: Vec<WorldAabb>,
        entity_collisions: Vec<WorldAabb>,
        pre_move_collisions: Vec<WorldAabb>,
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

    #[test]
    fn test_intersects_aabb() {
        let aabb1 = WorldAabb::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0);
        let aabb2 = WorldAabb::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0);

        assert!(aabb1.intersects(aabb2));

        let aabb3 = WorldAabb::new(5.0, 5.0, 5.0, 6.0, 6.0, 6.0);

        assert!(!aabb1.intersects(aabb3));
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
    fn collision_shape_filter_matches_vanilla_cursor_rules() {
        test_support::init_test_registry();

        let stone = vanilla_blocks::STONE.default_state();
        let moving_piston = vanilla_blocks::MOVING_PISTON.default_state();
        let large_shape = VoxelShape::from_boxes(LARGE_COLLISION_SHAPE);
        let shape = |shape| CollisionShape {
            shape,
            offset: DVec3::ZERO,
        };

        assert!(should_query_collision_shape(
            stone,
            shape(VoxelShape::FULL_BLOCK),
            CollisionCursorType::Inside
        ));
        assert!(!should_query_collision_shape(
            stone,
            shape(VoxelShape::FULL_BLOCK),
            CollisionCursorType::Face
        ));
        assert!(should_query_collision_shape(
            stone,
            shape(large_shape),
            CollisionCursorType::Face
        ));
        assert!(!should_query_collision_shape(
            stone,
            shape(large_shape),
            CollisionCursorType::Edge
        ));
        assert!(should_query_collision_shape(
            moving_piston,
            shape(VoxelShape::FULL_BLOCK),
            CollisionCursorType::Edge
        ));
        assert!(!should_query_collision_shape(
            moving_piston,
            shape(large_shape),
            CollisionCursorType::Corner
        ));
    }

    #[test]
    fn collision_shape_filter_uses_position_resolved_offset_bounds() {
        test_support::init_test_registry();

        let stone = vanilla_blocks::STONE.default_state();
        let shifted_full_block = CollisionShape {
            shape: VoxelShape::FULL_BLOCK,
            offset: DVec3::new(0.25, 0.0, 0.0),
        };

        assert!(should_query_collision_shape(
            stone,
            shifted_full_block,
            CollisionCursorType::Face
        ));
    }
}
