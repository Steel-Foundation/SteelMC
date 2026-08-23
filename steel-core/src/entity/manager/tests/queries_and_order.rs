use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use super::*;

struct SourceAwareHardCollisionEntity {
    base: EntityBase,
    accepted_source_id: i32,
    callback_count: AtomicUsize,
}

impl SourceAwareHardCollisionEntity {
    fn shared(id: i32, uuid: Uuid, position: DVec3, accepted_source_id: i32) -> Arc<Self> {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
            accepted_source_id,
            callback_count: AtomicUsize::new(0),
        })
    }
}

crate::entity::impl_test_downcast_type!(SourceAwareHardCollisionEntity);

impl Entity for SourceAwareHardCollisionEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn can_be_collided_with(&self, other: Option<&dyn Entity>) -> bool {
        self.callback_count.fetch_add(1, Ordering::Relaxed);
        other.is_some_and(|source| source.id() == self.accepted_source_id)
    }
}

struct BroadCollisionSourceEntity {
    base: EntityBase,
    accepted_target_id: i32,
}

impl BroadCollisionSourceEntity {
    fn shared(id: i32, uuid: Uuid, position: DVec3, accepted_target_id: i32) -> Arc<Self> {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
            accepted_target_id,
        })
    }
}

crate::entity::impl_test_downcast_type!(BroadCollisionSourceEntity);

impl Entity for BroadCollisionSourceEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn can_collide_with(&self, other: &dyn Entity) -> bool {
        other.id() == self.accepted_target_id
    }
}

fn hard_collision_candidate_ids(manager: &WorldEntityManager, aabb: &WorldAabb) -> Vec<i32> {
    let mut ids = Vec::new();
    let boxes = manager.get_movement_collision_boxes_in_aabb_matching(
        aabb,
        EntityCollisionCandidates::HardCollisionRelevant,
        |entity| {
            ids.push(entity.id());
            false
        },
    );
    assert_eq!(boxes, Vec::new());
    ids
}

#[test]
fn aabb_matching_query_filters_accessible_entities() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let first = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let second = entity(2, 2, DVec3::new(3.0, 64.0, 1.0));
    let outside = entity(3, 3, DVec3::new(30.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(first, EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(second.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(matches!(
        manager.add_live_entity(outside, EntityOwnership::ManagerOwned),
        Err(AddEntityError::ChunkNotLoaded { .. })
    ));

    let aabb = WorldAabb::new(0.0, 63.0, 0.0, 5.0, 66.0, 3.0);
    let result = manager.get_entities_in_aabb_matching(&aabb, |entity| entity.id() == 2);

    assert_eq!(result.len(), 1);
    assert!(Arc::ptr_eq(&result[0], &second));
}

#[test]
fn hard_collision_broad_phase_matches_full_scan_with_source_dependent_targets() {
    const NON_HARD_DISTRACTOR_COUNT: i32 = 128;
    const BACKGROUND_GRID_SIDE: i32 = 4;
    const SOURCE_ENTITY_ID: i32 = NON_HARD_DISTRACTOR_COUNT + 1;
    const LATER_SECTION_ENTITY_ID: i32 = SOURCE_ENTITY_ID + 1;
    const FIRST_SAME_SECTION_ENTITY_ID: i32 = SOURCE_ENTITY_ID + 2;
    const SECOND_SAME_SECTION_ENTITY_ID: i32 = SOURCE_ENTITY_ID + 3;
    const NEXT_CHUNK_INTERIOR_Z: f64 = 17.0;

    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));
    load_chunk(&manager, ChunkPos::new(0, 1));

    let source = ManagerTestEntity::shared_non_hard(
        SOURCE_ENTITY_ID,
        Uuid::from_u128(SOURCE_ENTITY_ID as u128),
        DVec3::new(1.0, 64.0, 1.0),
    );
    for id in 1..=NON_HARD_DISTRACTOR_COUNT {
        let position = DVec3::new(
            f64::from(id % BACKGROUND_GRID_SIDE),
            64.0,
            f64::from((id / BACKGROUND_GRID_SIDE) % BACKGROUND_GRID_SIDE),
        );
        let entity = ManagerTestEntity::shared_non_hard(id, Uuid::from_u128(id as u128), position);
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let later_section = SourceAwareHardCollisionEntity::shared(
        LATER_SECTION_ENTITY_ID,
        Uuid::from_u128(LATER_SECTION_ENTITY_ID as u128),
        DVec3::new(1.0, 64.0, NEXT_CHUNK_INTERIOR_Z),
        source.id(),
    );
    let first_same_section = SourceAwareHardCollisionEntity::shared(
        FIRST_SAME_SECTION_ENTITY_ID,
        Uuid::from_u128(FIRST_SAME_SECTION_ENTITY_ID as u128),
        DVec3::new(1.0, 64.0, 1.0),
        source.id(),
    );
    let second_same_section = SourceAwareHardCollisionEntity::shared(
        SECOND_SAME_SECTION_ENTITY_ID,
        Uuid::from_u128(SECOND_SAME_SECTION_ENTITY_ID as u128),
        DVec3::new(2.0, 64.0, 1.0),
        source.id(),
    );
    for entity in [
        Arc::clone(&later_section),
        Arc::clone(&first_same_section),
        Arc::clone(&second_same_section),
    ] {
        let entity: SharedEntity = entity;
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    assert!(!later_section.can_be_collided_with(None));
    assert!(later_section.can_be_collided_with(Some(source.as_ref())));
    assert_eq!(
        EntityCollisionCandidates::for_source(Some(source.as_ref())),
        EntityCollisionCandidates::HardCollisionRelevant
    );

    let aabb = WorldAabb::new(-1.0, 63.0, -1.0, 4.0, 66.0, 19.0);
    let full = manager.get_movement_collision_boxes_in_aabb_matching(
        &aabb,
        EntityCollisionCandidates::All,
        |entity| source.can_collide_with(entity),
    );
    let optimized = manager.get_movement_collision_boxes_in_aabb_matching(
        &aabb,
        EntityCollisionCandidates::for_source(Some(source.as_ref())),
        |entity| source.can_collide_with(entity),
    );

    assert_eq!(optimized, full);
    assert_eq!(
        optimized,
        vec![
            first_same_section.bounding_box(),
            second_same_section.bounding_box(),
            later_section.bounding_box(),
        ]
    );
}

#[test]
fn conservative_custom_source_scans_non_hard_targets() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let target =
        ManagerTestEntity::shared_non_hard(1, Uuid::from_u128(1), DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(Arc::clone(&target), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    let source = BroadCollisionSourceEntity::shared(
        2,
        Uuid::from_u128(2),
        DVec3::new(1.0, 64.0, 1.0),
        target.id(),
    );
    assert_eq!(
        EntityCollisionCandidates::for_source(Some(source.as_ref())),
        EntityCollisionCandidates::All
    );

    let optimized = manager.get_movement_collision_boxes_in_aabb_matching(
        &target.bounding_box(),
        EntityCollisionCandidates::for_source(Some(source.as_ref())),
        |entity| source.can_collide_with(entity),
    );
    let incorrectly_narrowed = manager.get_movement_collision_boxes_in_aabb_matching(
        &target.bounding_box(),
        EntityCollisionCandidates::HardCollisionRelevant,
        |entity| source.can_collide_with(entity),
    );

    assert_eq!(optimized, vec![target.bounding_box()]);
    assert_eq!(incorrectly_narrowed, Vec::new());
}

#[test]
fn hard_collision_index_tracks_move_bounds_unload_restore_and_remove() {
    let manager = WorldEntityManager::new();
    let first_chunk = ChunkPos::new(0, 0);
    let second_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, first_chunk);
    load_chunk(&manager, second_chunk);

    let hard = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let non_hard =
        ManagerTestEntity::shared_non_hard(2, Uuid::from_u128(2), DVec3::new(1.5, 64.0, 1.0));
    for entity in [Arc::clone(&hard), non_hard] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let original_bounds = WorldAabb::new(0.0, 63.0, 0.0, 3.0, 66.0, 3.0);
    assert_eq!(
        hard_collision_candidate_ids(&manager, &original_bounds),
        vec![1]
    );

    let moved_position = DVec3::new(17.0, 80.0, 1.0);
    hard.base().set_position_local(moved_position);
    assert!(manager.commit_move(hard.id(), moved_position).is_ok());
    assert_eq!(
        hard_collision_candidate_ids(&manager, &original_bounds),
        Vec::<i32>::new()
    );
    assert_eq!(
        hard_collision_candidate_ids(&manager, &hard.bounding_box()),
        vec![1]
    );

    let moved_bounds = WorldAabb::new(24.0, 80.0, 0.0, 25.0, 81.0, 1.0);
    hard.base().set_bounding_box(moved_bounds);
    manager.commit_bounding_box_change(hard.id());
    assert_eq!(
        hard_collision_candidate_ids(&manager, &moved_bounds),
        vec![1]
    );

    let unload = manager.begin_chunk_unload(second_chunk);
    assert_eq!(unload.retained.len(), 1);
    assert_eq!(
        hard_collision_candidate_ids(&manager, &moved_bounds),
        Vec::<i32>::new()
    );
    let restored = manager.on_chunk_loaded(second_chunk);
    assert_eq!(restored.restored.len(), 1);
    let _ = manager.update_chunk_visibility(second_chunk, EntityVisibility::Ticking);
    assert_eq!(
        hard_collision_candidate_ids(&manager, &moved_bounds),
        vec![1]
    );

    assert!(
        manager
            .remove_live_entity(hard.id(), RemovalReason::Discarded)
            .is_some()
    );
    assert_eq!(
        hard_collision_candidate_ids(&manager, &moved_bounds),
        Vec::<i32>::new()
    );
}

#[test]
fn movement_collision_uses_live_bounds_across_vanilla_query_phases() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));
    let first = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let second = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
    for entity in [Arc::clone(&first), Arc::clone(&second)] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let query = WorldAabb::new(0.0, 63.0, 0.0, 4.0, 66.0, 3.0);
    let moved_first = WorldAabb::new(1.25, 64.0, 1.0, 1.75, 64.5, 1.5);
    let moved_second = WorldAabb::new(20.0, 64.0, 1.0, 20.5, 64.5, 1.5);
    let mut callback_ids = Vec::new();

    let result = manager.get_movement_collision_boxes_in_aabb_matching(
        &query,
        EntityCollisionCandidates::All,
        |entity| {
            callback_ids.push(entity.id());
            if entity.id() == first.id() {
                first.base().set_bounding_box(moved_first);
                manager.commit_bounding_box_change(first.id());
                second.base().set_bounding_box(moved_second);
                manager.commit_bounding_box_change(second.id());
            }
            true
        },
    );

    assert_eq!(callback_ids, vec![first.id()]);
    assert_eq!(result, vec![moved_first]);
}

#[test]
fn visibility_transitions_separate_tracking_and_ticking() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    let result = manager.on_chunk_loaded(chunk);
    assert!(result.restored.is_empty());
    assert!(result.tracking_started.is_empty());
    assert!(result.ticking_started.is_empty());

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let changes = match manager.add_live_entity(entity.clone(), EntityOwnership::ManagerOwned) {
        Ok(changes) => changes,
        Err(error) => panic!("entity should register in active hidden chunk: {error}"),
    };
    assert_empty_lifecycle(changes);
    assert!(
        manager
            .get_entities_in_aabb(&entity.bounding_box())
            .is_empty()
    );

    let changes = manager.update_chunk_visibility(chunk, EntityVisibility::Tracked);
    assert_eq!(changes.tracking_started.len(), 1);
    assert!(Arc::ptr_eq(&changes.tracking_started[0], &entity));
    assert!(changes.ticking_started.is_empty());
    manager.tick_entities(0, true);
    assert_eq!(entity.tick_count(), 0);

    let changes = manager.update_chunk_visibility(chunk, EntityVisibility::Ticking);
    assert!(changes.tracking_started.is_empty());
    assert_eq!(changes.ticking_started.len(), 1);
    assert!(Arc::ptr_eq(&changes.ticking_started[0], &entity));
    manager.tick_entities(1, true);
    assert_eq!(entity.tick_count(), 1);

    let changes = manager.update_chunk_visibility(chunk, EntityVisibility::Tracked);
    assert!(changes.tracking_stopped.is_empty());
    assert_eq!(changes.ticking_stopped.len(), 1);
    assert!(Arc::ptr_eq(&changes.ticking_stopped[0], &entity));
    manager.tick_entities(2, true);
    assert_eq!(entity.tick_count(), 1);

    let changes = manager.update_chunk_visibility(chunk, EntityVisibility::Hidden);
    assert_eq!(changes.tracking_stopped.len(), 1);
    assert!(Arc::ptr_eq(&changes.tracking_stopped[0], &entity));
    assert!(changes.ticking_stopped.is_empty());
    assert!(
        manager
            .get_entities_in_aabb(&entity.bounding_box())
            .is_empty()
    );
}

#[test]
fn has_aabb_matching_query_respects_bounds_accessibility_and_predicate() {
    let manager = WorldEntityManager::new();
    let loaded_chunk = ChunkPos::new(0, 0);
    let hidden_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, loaded_chunk);
    load_chunk(&manager, hidden_chunk);

    let filtered_out = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let matching = entity(2, 2, DVec3::new(3.0, 64.0, 1.0));
    let hidden = entity(3, 3, DVec3::new(17.0, 64.0, 1.0));
    for entity in [filtered_out, matching, hidden] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let loaded_aabb = WorldAabb::new(0.0, 63.0, 0.0, 5.0, 66.0, 3.0);
    assert!(manager.has_entity_in_aabb_matching(&loaded_aabb, |entity| entity.id() == 2));
    assert!(!manager.has_entity_in_aabb_matching(&loaded_aabb, |entity| entity.id() == 3));

    manager.begin_chunk_unload(hidden_chunk);
    let hidden_aabb = WorldAabb::new(16.0, 63.0, 0.0, 18.0, 66.0, 3.0);
    assert!(!manager.has_entity_in_aabb_matching(&hidden_aabb, |entity| entity.id() == 3));
}

#[test]
fn aabb_matching_bounding_box_query_returns_only_matching_intersections() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let filtered_out = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let matching = entity(2, 2, DVec3::new(3.0, 64.0, 1.0));
    let outside = entity(3, 3, DVec3::new(8.0, 64.0, 1.0));
    let expected_box = matching.bounding_box();
    for entity in [filtered_out, matching, outside] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let aabb = WorldAabb::new(2.0, 63.0, 0.0, 4.0, 66.0, 3.0);
    let mut saw_outside_entity = false;
    let result = manager.get_entity_bounding_boxes_in_aabb_matching(&aabb, |entity| {
        saw_outside_entity |= entity.id() == 3;
        entity.id() > 1
    });

    assert_eq!(result, vec![expected_box]);
    assert!(!saw_outside_entity);
}

#[test]
fn nearest_aabb_matching_query_returns_closest_match() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let near_filtered_out = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let near_match = entity(2, 2, DVec3::new(3.0, 64.0, 1.0));
    let far_match = entity(3, 3, DVec3::new(8.0, 64.0, 1.0));
    for entity in [near_filtered_out, near_match.clone(), far_match] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let aabb = WorldAabb::new(0.0, 63.0, 0.0, 10.0, 66.0, 3.0);
    let result =
        manager.nearest_entity_in_aabb_matching(&aabb, DVec3::ZERO, |entity| entity.id() > 1);

    let Some(result) = result else {
        panic!("nearest matching entity should be found");
    };
    assert!(Arc::ptr_eq(&result, &near_match));
}

#[test]
fn accessible_entities_keep_tracking_start_order() {
    let manager = WorldEntityManager::new();
    let first_chunk = ChunkPos::new(0, 0);
    let second_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, first_chunk);
    load_chunk(&manager, second_chunk);

    let first = entity(30, 30, DVec3::new(1.0, 80.0, 1.0));
    let second = entity(10, 10, DVec3::new(17.0, 64.0, 1.0));
    let third = entity(20, 20, DVec3::new(2.0, 64.0, 1.0));
    for entity in [Arc::clone(&first), Arc::clone(&second), Arc::clone(&third)] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let entity_ids = manager
        .get_accessible_entities()
        .into_iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    assert_eq!(entity_ids, vec![30, 10, 20]);

    let changes = manager.update_chunk_visibility(first_chunk, EntityVisibility::Hidden);
    assert_eq!(changes.tracking_stopped.len(), 2);
    let changes = manager.update_chunk_visibility(first_chunk, EntityVisibility::Tracked);
    assert_eq!(changes.tracking_started.len(), 2);

    let entity_ids = manager
        .get_accessible_entities()
        .into_iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    assert_eq!(entity_ids, vec![10, 20, 30]);
}

#[test]
fn indexed_ordered_entity_ids_compaction_preserves_order_across_reinsertion() {
    const ENTITY_COUNT: i32 = (IndexedOrderedEntityIds::MIN_COMPACTION_SIZE * 2) as i32;

    let mut ids = IndexedOrderedEntityIds::default();
    for entity_id in 0..ENTITY_COUNT {
        assert!(ids.insert(entity_id));
    }

    for entity_id in (0..ENTITY_COUNT).step_by(2) {
        assert!(ids.remove(entity_id));
    }

    assert_eq!(
        ids.iter().copied().collect::<Vec<_>>(),
        (0..ENTITY_COUNT)
            .filter(|entity_id| entity_id % 2 != 0)
            .collect::<Vec<_>>()
    );

    for entity_id in (0..ENTITY_COUNT).step_by(2).rev() {
        assert!(ids.insert(entity_id));
    }

    assert_eq!(
        ids.iter().copied().collect::<Vec<_>>(),
        (0..ENTITY_COUNT)
            .filter(|entity_id| entity_id % 2 != 0)
            .chain((0..ENTITY_COUNT).step_by(2).rev())
            .collect::<Vec<_>>()
    );
}

#[test]
fn aabb_queries_use_vanilla_section_order_then_section_insertion_order() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));
    load_chunk(&manager, ChunkPos::new(0, 1));

    let later_section = entity(1, 1, DVec3::new(1.0, 64.0, 17.0));
    let first_same_section = entity(2, 2, DVec3::new(1.0, 64.0, 1.0));
    let second_same_section = entity(3, 3, DVec3::new(2.0, 64.0, 1.0));
    for entity in [
        later_section,
        Arc::clone(&first_same_section),
        Arc::clone(&second_same_section),
    ] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let aabb = WorldAabb::new(0.0, 63.0, 0.0, 18.0, 66.0, 18.0);
    let entity_ids = manager
        .get_entities_in_aabb(&aabb)
        .into_iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    assert_eq!(entity_ids, vec![2, 3, 1]);
}

#[test]
fn spatial_cell_reentry_preserves_section_insertion_order() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let first = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let second = entity(2, 2, DVec3::new(1.5, 64.0, 1.0));
    for entity in [Arc::clone(&first), second] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    for position in [DVec3::new(9.0, 64.0, 1.0), DVec3::new(1.0, 64.0, 1.0)] {
        first.base().set_position_local(position);
        assert!(manager.commit_move(first.id(), position).is_ok());
    }

    let entity_ids = manager
        .get_entities_in_aabb(&WorldAabb::new(0.0, 63.0, 0.0, 2.0, 66.0, 2.0))
        .into_iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    assert_eq!(entity_ids, vec![1, 2]);
}

#[test]
fn spatial_query_candidates_skip_distant_entities_in_the_same_section() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let nearby = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let distant = entity(2, 2, DVec3::new(13.0, 64.0, 1.0));
    for entity in [Arc::clone(&nearby), distant] {
        assert!(
            manager
                .add_live_entity(entity, EntityOwnership::ManagerOwned)
                .is_ok()
        );
    }

    let state = manager.state.read();
    let candidate_ids = WorldEntityManager::entity_query_entries(
        &state,
        &WorldAabb::new(0.0, 63.0, 0.0, 2.0, 66.0, 2.0),
    )
    .into_iter()
    .map(|entry| entry.entity.id())
    .collect::<Vec<_>>();

    assert_eq!(candidate_ids, vec![nearby.id()]);
}

#[test]
fn bounding_box_change_updates_spatial_index_without_position_change() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let old_bounds = entity.bounding_box();
    assert!(
        manager
            .add_live_entity(Arc::clone(&entity), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let new_bounds = WorldAabb::new(8.0, 64.0, 0.0, 9.0, 65.0, 1.0);
    entity.base().set_bounding_box(new_bounds);
    manager.commit_bounding_box_change(entity.id());

    assert!(manager.get_entities_in_aabb(&old_bounds).is_empty());
    let moved_bounds = manager.get_entities_in_aabb(&new_bounds);
    assert_eq!(moved_bounds.len(), 1);
    assert!(Arc::ptr_eq(&moved_bounds[0], &entity));
}

#[test]
fn delayed_bounding_box_callback_cannot_restore_stale_bounds() {
    let manager = Arc::new(WorldEntityManager::new());
    load_chunk(&manager, ChunkPos::new(0, 0));

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(Arc::clone(&entity), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let first_callback_entered = Arc::new(Barrier::new(2));
    let release_first_callback = Arc::new(Barrier::new(2));
    entity.set_level_callback(Arc::new(DelayedFirstBoundsCallback {
        entity_id: entity.id(),
        manager: Arc::clone(&manager),
        first_callback_entered: Arc::clone(&first_callback_entered),
        release_first_callback: Arc::clone(&release_first_callback),
        callback_count: AtomicUsize::new(0),
    }));

    let stale_bounds = WorldAabb::new(4.0, 64.0, 0.0, 5.0, 65.0, 1.0);
    let current_bounds = WorldAabb::new(8.0, 64.0, 0.0, 9.0, 65.0, 1.0);
    let first_entity = Arc::clone(&entity);
    let first_update = thread::spawn(move || {
        first_entity.base().set_bounding_box(stale_bounds);
    });

    first_callback_entered.wait();
    entity.base().set_bounding_box(current_bounds);
    release_first_callback.wait();
    assert!(first_update.join().is_ok());

    assert!(manager.get_entities_in_aabb(&stale_bounds).is_empty());
    let current = manager.get_entities_in_aabb(&current_bounds);
    assert_eq!(current.len(), 1);
    assert!(Arc::ptr_eq(&current[0], &entity));
}
