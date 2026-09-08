use super::*;

use steel_registry::entity_type::EntityDimensions;

use crate::entity::{EntityVisibility, PartEntity, PartEntityBase, reserve_entity_ids};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

const PART_COUNT: u32 = 2;

/// Deliberately unlike the stub's entity type dimensions, so the tests would catch a
/// part falling back to its parent's hitbox.
const PART_SIZE: EntityDimensions = EntityDimensions::with_default_eye_height(1.0, 1.0);

/// A stand-in for a multipart mob, so the world plumbing can be tested without
/// depending on any concrete boss.
struct MultipartTestEntity {
    base: EntityBase,
    parts: Vec<Arc<dyn PartEntity>>,
}

impl MultipartTestEntity {
    fn shared(position: DVec3, world: Weak<World>) -> SharedEntity {
        let ids = reserve_entity_ids(PART_COUNT + 1);
        let parts = (0..PART_COUNT)
            .map(|index| {
                let part: Arc<dyn PartEntity> = Arc::new(MultipartTestPart {
                    base: EntityBase::new(ids.part(index), position, PART_SIZE, world.clone()),
                    part_base: PartEntityBase::new("test_part", PART_SIZE),
                });
                part
            })
            .collect();

        Arc::new(Self {
            base: EntityBase::new(
                ids.first(),
                position,
                vanilla_entities::ITEM.dimensions,
                world,
            ),
            parts,
        })
    }
}

crate::entity::impl_test_downcast_type!(MultipartTestEntity);

impl Entity for MultipartTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn parts(&self) -> &[Arc<dyn PartEntity>] {
        &self.parts
    }
}

struct MultipartTestPart {
    base: EntityBase,
    part_base: PartEntityBase,
}

crate::entity::impl_test_downcast_type!(MultipartTestPart);

impl Entity for MultipartTestPart {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        self.part_base.size()
    }

    fn is_same_entity(&self, other: &dyn Entity) -> bool {
        self.id() == other.id()
            || self
                .parent()
                .is_some_and(|parent| parent.id() == other.id())
    }
}

impl PartEntity for MultipartTestPart {
    fn part_base(&self) -> &PartEntityBase {
        &self.part_base
    }
}

/// Somewhere inside the single chunk the tests make ready.
const SPAWN: DVec3 = DVec3::new(8.5, 64.0, 8.5);

fn spawn_multipart(world: &Arc<World>) -> SharedEntity {
    insert_ready_full_chunk(world, ChunkPos::from_entity_pos(SPAWN));
    let entity = MultipartTestEntity::shared(SPAWN, Arc::downgrade(world));
    world
        .try_add_entity(Arc::clone(&entity))
        .expect("multipart entity should be addable");
    entity
}

/// A box comfortably containing [`SPAWN`] and everything spawned there.
fn spawn_area() -> WorldAabb {
    WorldAabb::new(
        SPAWN.x - 4.0,
        SPAWN.y - 4.0,
        SPAWN.z - 4.0,
        SPAWN.x + 4.0,
        SPAWN.y + 4.0,
        SPAWN.z + 4.0,
    )
}

fn part_ids(entity: &SharedEntity) -> Vec<i32> {
    entity.parts().iter().map(|part| part.id()).collect()
}

#[test]
fn parts_take_the_ids_directly_after_their_parent() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_part_ids");
    let entity = spawn_multipart(&world);

    // This is the client-side contract: parts are synthesized at parent + 1..=n and
    // never spawned over the wire.
    for (index, part) in entity.parts().iter().enumerate() {
        assert_eq!(part.id(), entity.id() + index as i32 + 1);
    }
}

#[test]
fn tracking_a_multipart_entity_publishes_its_parts() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_registers_parts");
    let entity = spawn_multipart(&world);

    for part in entity.parts() {
        let found = world
            .get_accessible_entity_or_part_by_id(part.id())
            .expect("part should resolve through the world");
        assert_eq!(found.id(), part.id());
    }
}

#[test]
fn parts_are_not_reachable_through_the_plain_entity_lookup() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_plain_lookup");
    let entity = spawn_multipart(&world);
    let part_id = entity.parts()[0].id();

    // Vanilla keeps `getEntity` and `getEntityOrPart` separate; only the paths that
    // accept a client-supplied ID consult the parts map.
    assert!(world.get_entity_by_id(part_id).is_none());
    assert!(world.get_accessible_entity_by_id(part_id).is_none());
}

#[test]
fn parts_bind_their_parent_when_tracking_starts() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_binds_parent");
    let entity = spawn_multipart(&world);

    for part in entity.parts() {
        let parent = part.parent().expect("part should know its parent");
        assert_eq!(parent.id(), entity.id());
        assert!(part.is_same_entity(entity.as_ref()));
    }
}

#[test]
fn parts_appear_in_bounding_box_queries() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_aabb_query");
    let entity = spawn_multipart(&world);

    let found = world.get_entities_in_aabb(&spawn_area());

    let found_ids = found.iter().map(|entity| entity.id()).collect::<Vec<_>>();
    for part_id in part_ids(&entity) {
        assert!(
            found_ids.contains(&part_id),
            "part {part_id} should be returned by an intersecting query"
        );
    }
}

#[test]
fn bounding_box_queries_can_exclude_a_parent_and_its_parts() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_aabb_excluding");
    let entity = spawn_multipart(&world);

    // Vanilla excludes both the part itself and any part whose parent is `except`,
    // which is what stops a multipart mob shoving its own hitboxes.
    let found = world.get_entities_in_aabb_excluding(&spawn_area(), entity.as_ref(), |_| true);

    assert!(found.is_empty(), "no part of `except` should be returned");
}

#[test]
fn parts_are_removed_when_the_parent_stops_being_tracked() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_unregisters_parts");
    let entity = spawn_multipart(&world);
    let part_ids = part_ids(&entity);

    // Vanilla unregisters parts from `onTrackingEnd`, which is what a chunk losing
    // entity visibility drives here.
    world
        .update_entity_chunk_visibility(ChunkPos::from_entity_pos(SPAWN), EntityVisibility::Hidden);

    for part_id in part_ids {
        assert!(
            world.get_accessible_entity_or_part_by_id(part_id).is_none(),
            "part {part_id} should be gone once its parent stops being tracked"
        );
    }
}

#[test]
fn parts_are_never_tracked_for_clients() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_not_tracked");
    let entity = spawn_multipart(&world);

    // A part reports its parent's entity type, so it inherits a live tracking range;
    // only the explicit guard in `EntityTracker::add` keeps it off the wire.
    for part in entity.parts() {
        assert!(
            world
                .entity_tracker()
                .tracking_player_ids(part.id())
                .is_empty(),
            "a part must never be tracked"
        );
    }
}

#[test]
fn a_multipart_entity_does_not_collide_with_its_own_parts() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_self_collision");
    let entity = spawn_multipart(&world);

    // Vanilla passes the moving entity as `except` to `Level.getEntities`, which drops
    // a part both when it is `except` and when its parent is. `is_same_entity` carries
    // that rule, and the collision predicate relies on it.
    for part in entity.parts() {
        assert!(
            part.is_same_entity(entity.as_ref()),
            "a part must report its parent as itself so collision can exclude it"
        );
    }

    let found = world.get_entities_in_aabb_excluding(&spawn_area(), entity.as_ref(), |_| true);
    assert!(found.is_empty());
}

#[test]
fn removed_parts_drop_out_of_queries() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_removed_parts");
    let entity = spawn_multipart(&world);
    let removed = &entity.parts()[0];
    let surviving = entity.parts()[1].id();

    removed.set_removed(RemovalReason::Discarded);

    let found_ids = world
        .get_entities_in_aabb(&spawn_area())
        .iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    assert!(!found_ids.contains(&removed.id()));
    assert!(found_ids.contains(&surviving));
}

#[test]
#[should_panic(expected = "part index outside the reserved block")]
fn reading_past_the_reserved_block_is_rejected() {
    // Silently returning the next entity's ID would corrupt both the parts map and
    // the client's `parent + 1 + i` synthesis.
    let _ = reserve_entity_ids(PART_COUNT + 1).part(PART_COUNT);
}

#[test]
fn a_part_keeps_its_own_hitbox_across_a_dimension_refresh() {
    init_vanilla_registry();
    let world = fresh_test_world("multipart_part_hitbox");
    let entity = spawn_multipart(&world);
    let part = &entity.parts()[0];

    // A part reports its parent's entity type, so without the `dimensions_for_pose`
    // override this would snap to the parent's much larger box.
    part.refresh_dimensions();

    let box_ = part.bounding_box();
    let width = box_.max_x() - box_.min_x();
    let height = box_.max_y() - box_.min_y();
    assert!((width - f64::from(PART_SIZE.width)).abs() < 1.0e-9);
    assert!((height - f64::from(PART_SIZE.height)).abs() < 1.0e-9);
}
