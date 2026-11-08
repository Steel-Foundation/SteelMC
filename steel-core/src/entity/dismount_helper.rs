//! Vanilla dismount location helper

use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::{
    block_state_ext::BlockStateExt,
    properties::BlockStateProperties,
    shapes::{OffsetVoxelShape, VoxelShape},
};
use steel_registry::entity_data::EntityPose;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{vanilla_blocks, vanilla_entities};
use steel_utils::{BlockPos, BlockStateId, WorldAabb, axis::Axis};

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::entity::Entity;
use crate::entity::ai::walk::WalkPathEvaluator;
use crate::physics::{CollisionWorld, WorldCollisionProvider};
use crate::world::World;

/// The block offsets a passenger tries, in order, when leaving a vehicle
/// that faces `forward`.
#[must_use]
pub(crate) const fn offsets_for_direction(forward: steel_utils::Direction) -> [(i32, i32); 8] {
    let right = forward.rotate_y_clockwise();
    let left = right.opposite();
    let back = forward.opposite();
    let (right_x, right_z) = right.offset_xz();
    let (left_x, left_z) = left.offset_xz();
    let (back_x, back_z) = back.offset_xz();
    let (forward_x, forward_z) = forward.offset_xz();

    [
        (right_x, right_z),
        (left_x, left_z),
        (back_x + right_x, back_z + right_z),
        (back_x + left_x, back_z + left_z),
        (forward_x + right_x, forward_z + right_z),
        (forward_x + left_x, forward_z + left_z),
        (back_x, back_z),
        (forward_x, forward_z),
    ]
}

#[must_use]
pub(crate) fn can_dismount_to(world: &Arc<World>, aabb: &WorldAabb) -> bool {
    let collision_world = WorldCollisionProvider::new(world);
    !collision_world.has_block_collision_with_context(aabb, BlockCollisionContext::empty())
        && world.world_border_snapshot().is_within_bounds(*aabb)
}

/// Whether `passenger` fits in `aabb`, colliding with blocks as itself.
#[must_use]
pub(crate) fn can_passenger_dismount_to(
    world: &Arc<World>,
    passenger: &dyn Entity,
    aabb: &WorldAabb,
) -> bool {
    let provider = WorldCollisionProvider::for_entity(world, passenger);
    !provider.has_block_collision_with_context(aabb, provider.source_collision_context())
        && world.world_border_snapshot().is_within_bounds(*aabb)
}

/// Whether `passenger` fits at `location` in `dismount_pose`.
#[must_use]
pub(crate) fn can_dismount_to_pose(
    world: &Arc<World>,
    location: DVec3,
    passenger: &dyn Entity,
    dismount_pose: EntityPose,
) -> bool {
    can_passenger_dismount_to(
        world,
        passenger,
        &local_bounds_for_pose(passenger, dismount_pose, location),
    )
}

/// Vanilla `LivingEntity.getLocalBoundsForPose`, moved to `location`.
fn local_bounds_for_pose(passenger: &dyn Entity, pose: EntityPose, location: DVec3) -> WorldAabb {
    let dimensions = passenger.dimensions_for_pose(pose);
    WorldAabb::entity_box(
        location.x,
        location.y,
        location.z,
        f64::from(dimensions.half_width()),
        f64::from(dimensions.height),
    )
}

/// The lowest collision surface in the `blocks` blocks from `pos` upwards.
#[must_use]
pub(crate) fn find_ceiling_from(
    pos: BlockPos,
    blocks: i32,
    mut shape_getter: impl FnMut(BlockPos) -> OffsetVoxelShape,
) -> f64 {
    let mut y = 0;
    while y < blocks {
        let cursor = BlockPos::new(pos.x(), pos.y() + y, pos.z());
        let collision_shape = shape_getter(cursor);
        if !collision_shape.is_empty() {
            return f64::from(pos.y() + y) + collision_shape.min(Axis::Y);
        }
        y += 1;
    }

    f64::INFINITY
}

#[must_use]
pub(crate) fn find_safe_dismount_location(
    world: &Arc<World>,
    entity: &dyn Entity,
    block_pos: BlockPos,
    check_dangerous: bool,
) -> Option<DVec3> {
    if check_dangerous && is_block_dangerous(entity, world.get_block_state(block_pos)) {
        return None;
    }

    let floor_height = block_floor_height(
        non_climbable_shape(world, block_pos),
        non_climbable_shape(world, block_pos.below()),
    );
    if !is_block_floor_valid(floor_height) {
        return None;
    }

    if check_dangerous
        && floor_height <= 0.0
        && is_block_dangerous(entity, world.get_block_state(block_pos.below()))
    {
        return None;
    }

    let position = up_from_bottom_center_of(block_pos, floor_height);
    let dimensions = entity.dimensions_for_pose(EntityPose::Standing);
    let aabb = WorldAabb::entity_box(
        position.x,
        position.y,
        position.z,
        f64::from(dimensions.half_width()),
        f64::from(dimensions.height),
    );
    if !can_dismount_to(world, &aabb) {
        return None;
    }

    if entity.entity_type() == &vanilla_entities::PLAYER
        && (world
            .get_block_state(block_pos)
            .get_block()
            .has_tag(&BlockTag::INVALID_SPAWN_INSIDE)
            || world
                .get_block_state(block_pos.above())
                .get_block()
                .has_tag(&BlockTag::INVALID_SPAWN_INSIDE))
    {
        return None;
    }

    Some(position)
}

/// The block's collision shape, treating climbables and open trapdoors as
/// empty.
pub(crate) fn non_climbable_shape(world: &Arc<World>, pos: BlockPos) -> OffsetVoxelShape {
    let state = world.get_block_state(pos);
    let block = state.get_block();
    let is_open_trapdoor = block.has_tag(&BlockTag::TRAPDOORS)
        && state.try_get_value(&BlockStateProperties::OPEN) == Some(true);

    if block.has_tag(&BlockTag::CLIMBABLE) || is_open_trapdoor {
        return OffsetVoxelShape::without_offset(VoxelShape::EMPTY);
    }
    collision_shape(world, pos)
}

/// The block's collision shape without an entity context.
pub(crate) fn collision_shape(world: &Arc<World>, pos: BlockPos) -> OffsetVoxelShape {
    let state = world.get_block_state(pos);
    let block = state.get_block();
    let behavior = BLOCK_BEHAVIORS.get_behavior(block);
    let shape =
        behavior.get_collision_shape(state, world.as_ref(), pos, BlockCollisionContext::empty());
    if shape.is_empty() {
        return OffsetVoxelShape::without_offset(shape);
    }

    OffsetVoxelShape::new(
        shape,
        behavior.get_collision_shape_offset(
            state,
            world.as_ref(),
            pos,
            BlockCollisionContext::empty(),
        ),
    )
}

/// Vanilla `BlockGetter.getBlockFloorHeight(pos)`: the floor at `pos` from
/// plain collision shapes.
pub(crate) fn block_floor_height_at(world: &Arc<World>, pos: BlockPos) -> f64 {
    block_floor_height(
        collision_shape(world, pos),
        collision_shape(world, pos.below()),
    )
}

/// The height of the floor in a block, from the block's shape or, when
/// empty, a full block below it.
pub(crate) fn block_floor_height(
    block_shape: OffsetVoxelShape,
    below_block_shape: OffsetVoxelShape,
) -> f64 {
    if !block_shape.is_empty() {
        return block_shape.max(Axis::Y);
    }

    let below_floor = below_block_shape.max(Axis::Y);
    if below_floor >= 1.0 {
        below_floor - 1.0
    } else {
        f64::NEG_INFINITY
    }
}

/// Vanilla `Vec3.upFromBottomCenterOf`: `height` above the bottom center
/// of `pos`.
#[must_use]
pub(crate) fn up_from_bottom_center_of(pos: BlockPos, height: f64) -> DVec3 {
    DVec3::new(
        f64::from(pos.x()) + 0.5,
        f64::from(pos.y()) + height,
        f64::from(pos.z()) + 0.5,
    )
}

/// Whether a floor height lies within its block.
pub(crate) fn is_block_floor_valid(block_floor_height: f64) -> bool {
    !block_floor_height.is_infinite() && block_floor_height < 1.0
}

fn is_block_dangerous(entity: &dyn Entity, state: BlockStateId) -> bool {
    // TODO: mirror vanilla entitytype.immuneto when the entity types carry entity specific immune block tag
    if !entity.fire_immune() && WalkPathEvaluator::is_burning_block(state) {
        return true;
    }

    let block = state.get_block();
    block == &vanilla_blocks::WITHER_ROSE
        || block == &vanilla_blocks::SWEET_BERRY_BUSH
        || block == &vanilla_blocks::CACTUS
        || block == &vanilla_blocks::POWDER_SNOW
}
