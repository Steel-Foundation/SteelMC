//! Vanilla `EntityType` spawn/create helpers used by spawn eggs and similar items.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, WorldAabb, axis::Axis, wrap_degrees};

use super::{ENTITIES, EntitySpawnReason, LivingEntity, SharedEntity, next_entity_id};
use crate::behavior::BlockCollisionContext;
use crate::physics::{CollisionWorld, WorldCollisionProvider, collide};
use crate::world::World;

/// Vanilla `EntityType.canSpawn`.
#[must_use]
pub fn can_spawn(entity_type: EntityTypeRef, world: &World) -> bool {
    entity_type.allowed_in_peaceful || world.difficulty() != Difficulty::Peaceful
}

/// Vanilla `EntityType.spawn` that applies default item-stack post-spawn config.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla EntityType.spawn(level, stack, user, pos, reason, tryMoveDown, movedUp)"
)]
pub fn spawn(
    entity_type: EntityTypeRef,
    world: &Arc<World>,
    item_stack: Option<&ItemStack>,
    user: Option<&dyn LivingEntity>,
    spawn_pos: BlockPos,
    spawn_reason: EntitySpawnReason,
    try_move_down: bool,
    moved_up: bool,
) -> Option<SharedEntity> {
    let entity = create(
        entity_type,
        world,
        item_stack,
        user,
        spawn_pos,
        spawn_reason,
        try_move_down,
        moved_up,
    )?;

    if let Err(error) = world.try_add_entity(Arc::clone(&entity)) {
        log::debug!(
            "failed to add spawned {} at {spawn_pos:?}: {error}",
            entity_type.key,
        );
    }

    if let Some(mob) = entity.as_mob() {
        mob.play_ambient_sound();
    }

    Some(entity)
}

/// Vanilla `EntityType.create` with spawn-egg positioning and post-spawn config.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla EntityType.create(level, config, pos, reason, tryMoveDown, movedUp)"
)]
pub fn create(
    entity_type: EntityTypeRef,
    world: &Arc<World>,
    item_stack: Option<&ItemStack>,
    _user: Option<&dyn LivingEntity>,
    spawn_pos: BlockPos,
    spawn_reason: EntitySpawnReason,
    try_move_down: bool,
    moved_up: bool,
) -> Option<SharedEntity> {
    if !can_spawn(entity_type, world) {
        return None;
    }

    let entity = ENTITIES.create_or_raw(
        entity_type,
        next_entity_id(),
        DVec3::new(
            f64::from(spawn_pos.x()) + 0.5,
            f64::from(spawn_pos.y()),
            f64::from(spawn_pos.z()) + 0.5,
        ),
        Arc::downgrade(world),
    );

    let y_off = if try_move_down {
        let raised = DVec3::new(
            f64::from(spawn_pos.x()) + 0.5,
            f64::from(spawn_pos.y()) + 1.0,
            f64::from(spawn_pos.z()) + 0.5,
        );
        if entity.try_set_position(raised).is_err() {
            return None;
        }
        get_y_offset(world, spawn_pos, moved_up, entity.bounding_box())
    } else {
        0.0
    };

    let yaw = wrap_degrees(rand::random::<f32>() * 360.0);
    if entity
        .snap_to(
            DVec3::new(
                f64::from(spawn_pos.x()) + 0.5,
                f64::from(spawn_pos.y()) + y_off,
                f64::from(spawn_pos.z()) + 0.5,
            ),
            yaw,
            0.0,
        )
        .is_err()
    {
        return None;
    }

    if let Some(mob) = entity.as_mob() {
        let yaw = mob.rotation().0;
        mob.set_y_head_rot(yaw);
        mob.set_y_body_rot(yaw);
        let _ = mob.finalize_spawn(world, spawn_reason, None);
    }

    if let Some(item_stack) = item_stack {
        entity.apply_components_from_item_stack(item_stack);
    }

    Some(entity)
}

/// Vanilla `EntityType.getYOffset`.
#[must_use]
pub fn get_y_offset(
    world: &Arc<World>,
    spawn_pos: BlockPos,
    moved_up: bool,
    entity_box: WorldAabb,
) -> f64 {
    let min = DVec3::new(
        f64::from(spawn_pos.x()),
        f64::from(spawn_pos.y()),
        f64::from(spawn_pos.z()),
    );
    let mut aabb = WorldAabb::from_min_max(min, min + DVec3::ONE);
    if moved_up {
        aabb = aabb.expand_towards(DVec3::new(0.0, -1.0, 0.0));
    }

    let collisions = WorldCollisionProvider::new(world)
        .get_collisions_with_context(&aabb, BlockCollisionContext::empty());
    let desired = if moved_up { -2.0 } else { -1.0 };
    1.0 + collide(Axis::Y, &entity_box, &collisions, desired)
}

#[cfg(test)]
mod tests {
    use super::get_y_offset;
    use crate::bootstrap::init_globals_once;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use steel_registry::vanilla_blocks;
    use steel_utils::{BlockPos, ChunkPos, WorldAabb, types::UpdateFlags};

    #[test]
    fn y_offset_is_zero_when_the_spawn_cube_is_empty() {
        init_globals_once();
        let world = fresh_test_world("spawn_egg_y_offset_air");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let spawn_pos = BlockPos::new(0, 64, 0);
        let entity_box = WorldAabb::entity_box(0.5, 65.0, 0.5, 0.45, 0.9);
        assert_eq!(get_y_offset(&world, spawn_pos, false, entity_box), 0.0);
    }

    #[test]
    fn y_offset_rests_on_a_solid_spawn_cube() {
        init_globals_once();
        let world = fresh_test_world("spawn_egg_y_offset_stone");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let spawn_pos = BlockPos::new(0, 64, 0);
        assert!(world.set_block(
            spawn_pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        ));

        let entity_box = WorldAabb::entity_box(0.5, 65.0, 0.5, 0.45, 0.9);
        let offset = get_y_offset(&world, spawn_pos, false, entity_box);
        assert!(
            offset > 0.9,
            "solid spawn cube should keep the entity on top, got {offset}"
        );
    }
}
