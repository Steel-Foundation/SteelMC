use super::*;
use std::sync::Arc;

use crate::server::world_tick_workers::WorldTickWorkers;

#[test]
fn disconnect_releases_self_damage_history_while_frozen() {
    assert_disconnect_releases_damage_history("self_damage_disconnect", false, false);
}

#[test]
fn disconnect_releases_mutual_damage_history_while_frozen() {
    assert_disconnect_releases_damage_history("mutual_damage_disconnect", true, false);
}

#[test]
fn disconnect_outside_world_membership_releases_history_while_frozen() {
    assert_disconnect_releases_damage_history("detached_damage_disconnect", true, true);
}

fn assert_disconnect_releases_damage_history(
    world_name: &'static str,
    mutual_damage: bool,
    detached: bool,
) {
    let world = fresh_test_world(world_name);
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let root = test_storage_root(world_name);
        let server = test_server(Arc::clone(&world), PermissionSubjectIndex::new(), &root)
            .await
            .expect("test server");
        let first =
            test_player_with_packets(&server, Arc::clone(&world), "First", next_entity_id()).0;
        let second =
            test_player_with_packets(&server, Arc::clone(&world), "Second", next_entity_id()).0;
        for player in [&first, &second] {
            assert!(server.online_players.insert(Arc::clone(player)));
            assert!(world.add_player(Arc::clone(player), ResetReason::InitialJoin));
            let _ = player.mark_joined_world();
        }

        let attacker = if mutual_damage { &second } else { &first };
        first.record_last_damage_source(
            &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
                .with_causing_entity(attacker.clone()),
        );
        if mutual_damage {
            second.record_last_damage_source(
                &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
                    .with_causing_entity(first.clone()),
            );
        }
        let first_weak = Arc::downgrade(&first);
        let second_weak = Arc::downgrade(&second);
        let damage_time = world.game_time();
        server.tick_rate_manager.write().set_frozen(true);

        if detached {
            world.remove_player_for_world_change(&first);
            world.remove_player_for_world_change(&second);
            server.damage_history.expire();
            assert!(
                first
                    .last_damage_source()
                    .expect("online history")
                    .causing_entity()
                    .is_some()
            );
        }

        for player in [first, second] {
            player.connection.close();
            server.queue_player_disconnect(player);
        }
        let mut saves = JoinSet::new();
        server.start_player_disconnect_saves(&mut saves);
        while let Some(result) = saves.join_next().await {
            result.expect("disconnect save task");
        }

        let workers = WorldTickWorkers::spawn(server.worlds.values()).expect("world workers");
        server
            .tick_worlds_game(&workers, 1, false)
            .await
            .expect("frozen tick");
        drop(workers);
        assert_eq!(world.game_time(), damage_time);
        fs::remove_dir_all(root)
            .await
            .expect("test storage cleanup");

        assert!(
            first_weak.upgrade().is_none(),
            "server history must release the disconnected player without advancing game time"
        );
        assert!(
            second_weak.upgrade().is_none(),
            "mutual damage must not retain the other disconnected player"
        );
    });
}
