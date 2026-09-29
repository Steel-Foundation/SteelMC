use std::sync::{Arc, Barrier};
use std::thread;

use uuid::Uuid;

use super::PlayerMap;
use crate::player::Player;
use crate::test_support::{TestPlayerBuilder, test_world};

fn player(uuid: Uuid, name: &str, entity_id: i32) -> Arc<Player> {
    TestPlayerBuilder::new(Arc::clone(test_world()), name, entity_id)
        .uuid(uuid)
        .build()
}

fn indexed_player(map: &PlayerMap, uuid: &Uuid, entity_id: i32) -> Arc<Player> {
    let Some(by_uuid) = map.get_by_uuid(uuid) else {
        panic!("player should remain indexed by UUID");
    };
    let Some(by_entity_id) = map.get_by_entity_id(entity_id) else {
        panic!("player should remain indexed by entity ID");
    };
    assert!(Arc::ptr_eq(&by_uuid, &by_entity_id));
    by_uuid
}

#[test]
fn replacement_updates_both_indexes_without_reordering() {
    let replaced_uuid = Uuid::from_u128(1);
    let other_uuid = Uuid::from_u128(2);
    let original = player(replaced_uuid, "Original", 10);
    let other = player(other_uuid, "Other", 20);
    let replacement = player(replaced_uuid, "Replacement", 10);
    let map = PlayerMap::new();

    assert!(map.insert(Arc::clone(&original)));
    assert!(map.insert(other));
    assert!(map.replace_player(&original, Arc::clone(&replacement)));

    let indexed = indexed_player(&map, &replaced_uuid, 10);
    assert!(Arc::ptr_eq(&indexed, &replacement));
    assert!(!Arc::ptr_eq(&indexed, &original));

    let mut order = Vec::new();
    map.iter_players(|uuid, _| {
        order.push(*uuid);
        true
    });
    assert_eq!(order, [replaced_uuid, other_uuid]);
}

#[test]
fn stale_expected_player_cannot_replace_current_player() {
    let uuid = Uuid::from_u128(3);
    let original = player(uuid, "Original", 30);
    let current = player(uuid, "Current", 30);
    let rejected = player(uuid, "Rejected", 30);
    let map = PlayerMap::new();

    assert!(map.insert(Arc::clone(&original)));
    assert!(map.replace_player(&original, Arc::clone(&current)));
    assert!(!map.replace_player(&original, rejected));

    let indexed = indexed_player(&map, &uuid, 30);
    assert!(Arc::ptr_eq(&indexed, &current));
}

#[test]
fn replacement_rejects_changed_index_keys() {
    let uuid = Uuid::from_u128(4);
    let original = player(uuid, "Original", 40);
    let changed_uuid = player(Uuid::from_u128(5), "ChangedUuid", 40);
    let changed_entity_id = player(uuid, "ChangedEntityId", 41);
    let map = PlayerMap::new();

    assert!(map.insert(Arc::clone(&original)));
    assert!(!map.replace_player(&original, changed_uuid));
    assert!(!map.replace_player(&original, changed_entity_id));

    let indexed = indexed_player(&map, &uuid, 40);
    assert!(Arc::ptr_eq(&indexed, &original));
}

#[test]
fn concurrent_replacements_with_the_same_expected_player_have_one_winner() {
    let uuid = Uuid::from_u128(6);
    let original = player(uuid, "Original", 60);
    let first_replacement = player(uuid, "First", 60);
    let second_replacement = player(uuid, "Second", 60);
    let map = Arc::new(PlayerMap::new());
    let barrier = Arc::new(Barrier::new(3));
    assert!(map.insert(Arc::clone(&original)));

    let first_attempt = {
        let map = Arc::clone(&map);
        let barrier = Arc::clone(&barrier);
        let original = Arc::clone(&original);
        let replacement = Arc::clone(&first_replacement);
        thread::spawn(move || {
            barrier.wait();
            map.replace_player(&original, replacement)
        })
    };
    let second_attempt = {
        let map = Arc::clone(&map);
        let barrier = Arc::clone(&barrier);
        let original = Arc::clone(&original);
        let replacement = Arc::clone(&second_replacement);
        thread::spawn(move || {
            barrier.wait();
            map.replace_player(&original, replacement)
        })
    };
    barrier.wait();

    let Ok(first_succeeded) = first_attempt.join() else {
        panic!("first replacement thread should finish");
    };
    let Ok(second_succeeded) = second_attempt.join() else {
        panic!("second replacement thread should finish");
    };
    assert_ne!(first_succeeded, second_succeeded);

    let indexed = indexed_player(&map, &uuid, 60);
    let winner_is_first = Arc::ptr_eq(&indexed, &first_replacement);
    let winner_is_second = Arc::ptr_eq(&indexed, &second_replacement);
    assert_ne!(winner_is_first, winner_is_second);
}
