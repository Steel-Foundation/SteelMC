use super::{
    Arc, Axis, BLOCK_BEHAVIORS, BlockCollisionContext, BlockPos, BlockStateExt, BlockStateId,
    BooleanOp, DVec3, World, WorldAabb, collide, join_unoptimized_boxes,
};

/// Vanilla `Block.pushEntitiesUp` for block-state replacements that add collision.
///
/// Returns `new_state` so callers can mirror vanilla call sites that transform
/// the replacement state before setting it in the world.
pub(crate) fn push_entities_up(
    old_state: BlockStateId,
    new_state: BlockStateId,
    world: &Arc<World>,
    pos: BlockPos,
) -> BlockStateId {
    let added_collision = added_collision_boxes(old_state, new_state, world, pos);
    let Some(query_box) = world_aabb_bounds(&added_collision) else {
        return new_state;
    };

    for entity in world.get_entities_in_aabb(&query_box) {
        let offset = collide(
            Axis::Y,
            &entity.bounding_box().translate(DVec3::ZERO.with_y(1.0)),
            &added_collision,
            -1.0,
        );
        if let Err(error) =
            entity.try_set_position(entity.position() + DVec3::new(0.0, 1.0 + offset, 0.0))
        {
            log::debug!(
                "Failed to push entity {} up after block collision change at {pos:?}: {error}",
                entity.id()
            );
        }
    }

    new_state
}

fn added_collision_boxes(
    old_state: BlockStateId,
    new_state: BlockStateId,
    world: &Arc<World>,
    pos: BlockPos,
) -> Vec<WorldAabb> {
    let context = BlockCollisionContext::empty();
    let old_shape = BLOCK_BEHAVIORS
        .get_behavior(old_state.get_block())
        .get_collision_shape(old_state, world.as_ref(), pos, context);
    let new_shape = BLOCK_BEHAVIORS
        .get_behavior(new_state.get_block())
        .get_collision_shape(new_state, world.as_ref(), pos, context);

    join_unoptimized_boxes(old_shape, new_shape, BooleanOp::OnlySecond)
        .into_iter()
        .map(|aabb| aabb.at_block(pos))
        .collect()
}

pub(super) fn world_aabb_bounds(boxes: &[WorldAabb]) -> Option<WorldAabb> {
    let first = boxes.first()?;
    let mut min_x = first.min_x();
    let mut min_y = first.min_y();
    let mut min_z = first.min_z();
    let mut max_x = first.max_x();
    let mut max_y = first.max_y();
    let mut max_z = first.max_z();

    for aabb in boxes {
        min_x = min_x.min(aabb.min_x());
        min_y = min_y.min(aabb.min_y());
        min_z = min_z.min(aabb.min_z());
        max_x = max_x.max(aabb.max_x());
        max_y = max_y.max(aabb.max_y());
        max_z = max_z.max(aabb.max_z());
    }

    Some(WorldAabb::new(min_x, min_y, min_z, max_x, max_y, max_z))
}
