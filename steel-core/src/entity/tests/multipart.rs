use super::*;

use steel_registry::entity_type::EntityDimensions;

use crate::entity::{EntityVisibility, PartEntity, PartEntityBase, reserve_entity_ids};
use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};

const PART_COUNT: u32 = 2;

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
                    base: EntityBase::new(
                        ids.part(index),
                        position,
                        vanilla_entities::ITEM.dimensions,
                        Weak::clone(&world),
                    ),
                    part_base: PartEntityBase::new(
                        "test_part",
                        EntityDimensions::with_default_eye_height(1.0, 1.0),
                    ),
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
}

impl PartEntity for MultipartTestPart {
    fn part_base(&self) -> &PartEntityBase {
        &self.part_base
    }
}

/// Somewhere inside the single chunk the tests make ready.
const SPAWN: DVec3 = DVec3::new(8.5, 64.0, 8.5);

fn spawn_multipart(test_name: &'static str) -> (TestWorld, SharedEntity) {
    init_vanilla_registry();
    let world_fixture = fresh_test_world(test_name);
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::from_entity_pos(SPAWN));
    let entity = MultipartTestEntity::shared(SPAWN, Arc::downgrade(world));
    world
        .try_add_entity(Arc::clone(&entity))
        .expect("multipart entity should be addable");
    (world_fixture, entity)
}

fn box_around(center: DVec3, half_size: f64) -> WorldAabb {
    WorldAabb::new(
        center.x - half_size,
        center.y - half_size,
        center.z - half_size,
        center.x + half_size,
        center.y + half_size,
        center.z + half_size,
    )
}

/// A box comfortably containing [`SPAWN`] and everything spawned there.
fn spawn_area() -> WorldAabb {
    box_around(SPAWN, 4.0)
}

fn part_ids(entity: &SharedEntity) -> Vec<i32> {
    entity.parts().iter().map(|part| part.id()).collect()
}

#[test]
fn parts_take_the_ids_directly_after_their_parent() {
    let (_world_fixture, entity) = spawn_multipart("multipart_part_ids");

    for (index, part) in entity.parts().iter().enumerate() {
        assert_eq!(part.id(), entity.id() + index as i32 + 1);
    }
}

#[test]
fn tracking_a_multipart_entity_publishes_its_parts() {
    let (world_fixture, entity) = spawn_multipart("multipart_registers_parts");
    let world = &world_fixture.world;

    for part in entity.parts() {
        let found = world
            .get_accessible_entity_or_part_by_id(part.id())
            .expect("part should resolve through the world");
        assert_eq!(found.id(), part.id());
    }
}

#[test]
fn parts_are_not_reachable_through_the_plain_entity_lookup() {
    let (world_fixture, entity) = spawn_multipart("multipart_plain_lookup");
    let world = &world_fixture.world;
    let part_id = entity.parts()[0].id();

    assert!(world.get_entity_by_id(part_id).is_none());
    assert!(world.get_accessible_entity_by_id(part_id).is_none());
}

#[test]
fn parts_bind_their_parent_when_tracking_starts() {
    let (_world_fixture, entity) = spawn_multipart("multipart_binds_parent");

    for part in entity.parts() {
        let parent = part.parent().expect("part should know its parent");
        assert_eq!(parent.id(), entity.id());
        assert!(part.is_same_entity(entity.as_ref()));
    }
}

#[test]
fn parts_appear_in_bounding_box_queries() {
    let (world_fixture, entity) = spawn_multipart("multipart_aabb_query");
    let world = &world_fixture.world;

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
    let (world_fixture, entity) = spawn_multipart("multipart_aabb_excluding");
    let world = &world_fixture.world;

    let found = world.get_entities_in_aabb_excluding(&spawn_area(), entity.as_ref(), |_| true);

    assert!(found.is_empty(), "no part of `except` should be returned");
}

#[test]
fn parts_are_removed_when_the_parent_stops_being_tracked() {
    let (world_fixture, entity) = spawn_multipart("multipart_unregisters_parts");
    let world = &world_fixture.world;
    let part_ids = part_ids(&entity);

    // Losing chunk visibility ends tracking, which unregisters the parts.
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
fn parts_are_removed_when_the_parent_is_discarded() {
    let (world_fixture, entity) = spawn_multipart("multipart_discard_unregisters_parts");
    let world = &world_fixture.world;
    let part_ids = part_ids(&entity);

    // Removal goes through the entity callback, not a lifecycle batch.
    entity.set_removed(RemovalReason::Discarded);

    for part_id in part_ids {
        assert!(
            world.get_accessible_entity_or_part_by_id(part_id).is_none(),
            "part {part_id} should be gone once its parent is discarded"
        );
    }
}

/// Moves the first part away from its parent and returns a small box around it only.
fn separate_first_part(entity: &SharedEntity) -> WorldAabb {
    let offset = SPAWN + DVec3::new(4.0, 0.0, 4.0);
    entity.parts()[0].set_part_position(offset);
    box_around(offset, 0.5)
}

#[test]
fn class_queries_skip_parts_whose_parent_is_outside_the_box() {
    let (world_fixture, entity) = spawn_multipart("multipart_class_query_needs_parent");
    let world = &world_fixture.world;
    let part_only = separate_first_part(&entity);

    assert!(
        !world.get_entities_in_aabb(&part_only).is_empty(),
        "the predicate query tests each part's own box"
    );
    assert!(
        world
            .get_entities_of_class_matching(&part_only, |_| true)
            .is_empty(),
        "a lone part must not match without its parent"
    );
}

#[test]
fn class_queries_reach_every_part_through_a_parent_in_the_box() {
    let (world_fixture, entity) = spawn_multipart("multipart_class_query_via_parent");
    let world = &world_fixture.world;
    separate_first_part(&entity);

    let found_ids = world
        .get_entities_of_class_matching(&box_around(SPAWN, 0.5), |_| true)
        .iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();

    assert!(found_ids.contains(&entity.id()));
    for part_id in part_ids(&entity) {
        assert!(
            found_ids.contains(&part_id),
            "part {part_id} should be reached through its parent"
        );
    }
}

#[test]
fn refreshing_a_part_keeps_its_own_size() {
    let (_world_fixture, entity) = spawn_multipart("multipart_part_dimensions");
    let part = &entity.parts()[0];

    part.refresh_dimensions();

    let size = part.part_base().size();
    let dimensions = part.base().dimensions();
    assert!((dimensions.width - size.width).abs() < f32::EPSILON);
    assert!((dimensions.height - size.height).abs() < f32::EPSILON);
}

#[test]
fn parts_are_never_tracked_for_clients() {
    let (world_fixture, entity) = spawn_multipart("multipart_not_tracked");
    let world = &world_fixture.world;

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
fn removed_parts_drop_out_of_queries() {
    let (world_fixture, entity) = spawn_multipart("multipart_removed_parts");
    let world = &world_fixture.world;
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
    let _ = reserve_entity_ids(PART_COUNT + 1).part(PART_COUNT);
}

#[test]
#[should_panic(expected = "part index outside the reserved block")]
fn the_largest_part_index_is_rejected_instead_of_wrapping() {
    let _ = reserve_entity_ids(PART_COUNT + 1).part(u32::MAX);
}
