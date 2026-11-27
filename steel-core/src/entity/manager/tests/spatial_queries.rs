use super::*;

fn ids(entities: &[SharedEntity]) -> Vec<i32> {
    entities.iter().map(|entity| entity.id()).collect()
}

#[test]
fn collision_snapshot_keeps_candidates_moved_into_bounds_by_an_earlier_callback() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));
    let first = entity(1, 1, DVec3::new(0.0, 64.0, 0.0));
    let later = entity(2, 2, DVec3::new(1.8, 64.0, 0.0));

    for entity in [&first, &later] {
        manager
            .add_live_entity(Arc::clone(entity), EntityOwnership::ManagerOwned)
            .expect("register collision candidate");
    }

    let query = WorldAabb::new(-0.5, 63.0, -0.5, 0.5, 66.0, 0.5);
    assert!(!later.bounding_box().intersects(query));

    let mut visited = Vec::new();
    let boxes = manager.get_movement_collision_boxes_in_aabb_matching(
        &query,
        EntityCollisionCandidates::All,
        |candidate| {
            visited.push(candidate.id());

            if candidate.id() == first.id() {
                let position = DVec3::new(0.1, 64.0, 0.0);
                later.base().set_position_local(position);
                manager
                    .commit_move(later.id(), position)
                    .expect("callbacks run without the manager read guard");
            }

            true
        },
    );

    assert_eq!(visited, vec![first.id(), later.id()]);
    assert_eq!(boxes, vec![first.bounding_box(), later.bounding_box()]);
}

#[test]
fn overlapping_cell_updates_keep_both_indexes_ordered() {
    let manager = WorldEntityManager::new();

    for x in 0..=1 {
        load_chunk(&manager, ChunkPos::new(x, 0));
    }

    let first = entity(1, 1, DVec3::new(15.9, 64.0, 1.0));
    let second = entity(2, 2, DVec3::new(16.1, 64.0, 1.0));
    let non_hard =
        ManagerTestEntity::shared_non_hard(3, Uuid::from_u128(3), DVec3::new(16.2, 64.0, 1.0));

    for entity in [&first, &second, &non_hard] {
        manager
            .add_live_entity(Arc::clone(entity), EntityOwnership::ManagerOwned)
            .expect("register overlapping cells");
    }

    let query = WorldAabb::new(12.0, 62.0, -2.0, 20.0, 68.0, 4.0);
    let verify_order = |all: &[i32], hard: &[i32]| {
        assert_eq!(ids(&manager.get_entities_in_aabb(&query)), all);

        for (candidates, expected) in [
            (EntityCollisionCandidates::All, all),
            (EntityCollisionCandidates::HardCollisionRelevant, hard),
        ] {
            let snapshot = manager.movement_collision_candidate_snapshot(&query, candidates);
            assert_eq!(ids(&snapshot), expected);
        }
    };

    verify_order(&[1, 2, 3], &[1, 2]);

    for position in [DVec3::new(16.0, 64.0, 1.0), DVec3::new(17.95, 64.0, 1.0)] {
        first.base().set_position_local(position);
        manager
            .commit_move(first.id(), position)
            .expect("move across overlapping cells");
        verify_order(&[2, 3, 1], &[2, 1]);
    }

    for bounds in [
        WorldAabb::new(13.0, 63.0, -1.0, 19.0, 66.0, 3.0),
        WorldAabb::new(-32.0, 48.0, -32.0, 64.0, 88.0, 32.0),
    ] {
        first.base().set_bounding_box(bounds);
        manager.commit_bounding_box_change(first.id());
        verify_order(&[2, 3, 1], &[2, 1]);
    }

    let position = DVec3::new(15.9, 64.0, 1.0);
    first.base().set_position_local(position);
    manager
        .commit_move(first.id(), position)
        .expect("return to the earlier section");
    verify_order(&[1, 2, 3], &[1, 2]);
}

#[test]
fn borrowed_query_fallback_preserves_order_and_slot_reuse() {
    let manager = WorldEntityManager::new();

    for chunk in [
        ChunkPos::new(0, 0),
        ChunkPos::new(0, 1),
        ChunkPos::new(1, 0),
    ] {
        load_chunk(&manager, chunk);
    }

    let first = entity(1, 1, DVec3::new(1.0, 64.0, 17.0));
    let second = entity(2, 2, DVec3::new(1.99, 64.0, 1.99));
    let third = entity(3, 3, DVec3::new(1.8, 64.0, 1.8));
    let hidden = entity(4, 4, DVec3::new(17.0, 64.0, 1.0));

    for entity in [&first, &second, &third, &hidden] {
        manager
            .add_live_entity(Arc::clone(entity), EntityOwnership::ManagerOwned)
            .expect("register bounded query fixture");
    }

    manager.update_chunk_visibility(ChunkPos::new(1, 0), EntityVisibility::Hidden);
    let query = WorldAabb::new(-1.0, 63.0, -1.0, 20.0, 66.0, 20.0);
    let expected = [2, 3, 1];

    for limit in [0, 1, 2] {
        let snapshot = manager
            .try_with_entities_in_aabb(&query, Some(limit), |_| {
                panic!("too many hits must skip the borrowed callback");
            })
            .expect("owned snapshot beyond the borrow limit");

        assert_eq!(ids(&snapshot), expected);
        manager
            .commit_move(second.id(), second.position())
            .expect("owned fallback releases the manager read lock");
    }

    for limit in [None, Some(3), Some(4)] {
        let mut visited = Vec::new();
        let snapshot = manager.try_with_entities_in_aabb(&query, limit, |entities| {
            visited.extend(entities.iter().map(|entity| entity.id()));
            true
        });

        assert!(snapshot.is_none());
        assert_eq!(visited, expected);

        let snapshot = manager
            .try_with_entities_in_aabb(&query, limit, |_| false)
            .expect("declined borrowed action returns owned hits");

        assert_eq!(ids(&snapshot), expected);
    }

    manager
        .remove_live_entity(second.id(), RemovalReason::Discarded)
        .expect("remove a query candidate");
    manager
        .add_live_entity(
            entity(5, 5, second.position()),
            EntityOwnership::ManagerOwned,
        )
        .expect("reuse the removed candidate slot");
    let snapshot = manager
        .try_with_entities_in_aabb(&query, Some(0), |_| panic!("owned fallback expected"))
        .expect("owned hits after slot reuse");
    assert_eq!(ids(&snapshot), [3, 5, 1]);

    manager.begin_chunk_unload(ChunkPos::new(0, 0));
    manager
        .add_live_entity(
            entity(6, 6, DVec3::new(1.9, 64.0, 17.9)),
            EntityOwnership::ManagerOwned,
        )
        .expect("reuse a slot from the unloaded chunk");
    assert_eq!(ids(&manager.get_entities_in_aabb(&query)), [1, 6]);
}
