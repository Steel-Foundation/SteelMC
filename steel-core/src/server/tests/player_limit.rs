use std::sync::Arc;

use steel_utils::{Identifier, translations};
use text_components::TextComponent;
use tokio::{fs, runtime::Builder};
use uuid::Uuid;

use crate::entity::Entity;
use crate::permission::{
    OP_GROUP, PermissionGroupConfig, PermissionGroupsConfig, PermissionMetadataEntry,
    PermissionMetadataRuleConfig, PermissionMetadataSet, PermissionMetadataValue, PermissionSet,
    PermissionSubjectIndex, PermissionSubjectState,
};

use super::super::player_admission::PlayerJoinError;
use super::{
    DomainPlayerData, DomainPlayerState, PendingPlayerJoin, PreparedSpawn, fresh_test_world,
    test_connection, test_player_with_connection, test_player_with_packets,
    test_player_with_uuid_and_packets, test_server, test_server_with_max_players,
    test_storage_root,
};

#[test]
fn max_players_counts_admitted_players_not_pending_preparation() -> Result<(), String> {
    let world = fresh_test_world("player_limit_preparation");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let storage_root = test_storage_root("player-limit-preparation");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await?;
        let (slow, _) = test_player_with_packets(&server, Arc::clone(&world), "Slow", 1);
        let (fast, _) = test_player_with_packets(&server, world, "Fast", 2);

        let slow_reservation = server.try_reserve_player_join(slow.gameprofile.id);
        let fast_reservation = server.try_reserve_player_join(fast.gameprofile.id);
        assert!(slow_reservation.is_some());
        assert!(fast_reservation.is_some());
        assert!(!server.is_player_limit_reached(fast.gameprofile.id));

        assert_eq!(server.admit_reserved_player(Arc::clone(&fast)), Ok(()));
        assert!(server.is_player_limit_reached(slow.gameprofile.id));
        assert!(server.is_player_limit_reached(fast.gameprofile.id));
        assert_eq!(
            server.admit_reserved_player(Arc::clone(&slow)),
            Err(PlayerJoinError::ServerFull),
        );
        assert_eq!(server.player_count(), 1);
        assert!(
            !server
                .player_admissions
                .lock()
                .contains_key(&slow.gameprofile.id)
        );
        drop(slow_reservation);
        drop(fast_reservation);

        assert!(server.reserve_player_disconnect(&fast));
        assert!(server.remove_online_player_sync(&fast).is_some());
        assert!(!server.is_player_limit_reached(slow.gameprofile.id));
        let retry = server.try_reserve_player_join(slow.gameprofile.id);
        assert!(retry.is_some());
        assert_eq!(server.admit_reserved_player(slow), Ok(()));
        drop(retry);

        fs::remove_dir_all(storage_root)
            .await
            .map_err(|error| error.to_string())
    })
}

#[test]
fn max_players_rechecks_group_bypass_after_preparation() -> Result<(), String> {
    let world = fresh_test_world("player_limit_bypass_refresh");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let storage_root = test_storage_root("player-limit-bypass-refresh");
        let uuid = Uuid::from_u128(1);
        let mut subjects = PermissionSubjectIndex::new();
        subjects.set(
            uuid,
            PermissionSubjectState::new(vec!["reserved".to_owned()], PermissionSet::new()),
        );
        let server =
            test_server_with_max_players(Arc::clone(&world), subjects, &storage_root, 0).await?;
        let mut groups = PermissionGroupsConfig::default();
        groups.groups.insert(
            "reserved".to_owned(),
            PermissionGroupConfig {
                metadata: vec![PermissionMetadataRuleConfig {
                    key: "steel:bypasses_player_limit".to_owned(),
                    value: PermissionMetadataValue::Bool(true),
                }],
                ..PermissionGroupConfig::default()
            },
        );
        server
            .replace_permission_groups(groups)
            .await
            .map_err(|error| error.to_string())?;
        assert!(!server.is_player_limit_reached(uuid));
        let (player, _) = test_player_with_uuid_and_packets(&server, world, uuid, "Candidate", 1);
        assert!(server.reserve_player_join(&player));

        server
            .replace_permission_groups(PermissionGroupsConfig::default())
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(
            server.admit_reserved_player(player),
            Err(PlayerJoinError::ServerFull)
        );
        assert_eq!(server.player_count(), 0);
        fs::remove_dir_all(storage_root)
            .await
            .map_err(|error| error.to_string())
    })
}

#[test]
fn max_players_rejected_prepared_join_disconnects_and_releases_uuid() -> Result<(), String> {
    let world = fresh_test_world("player_limit_disconnect");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let storage_root = test_storage_root("player-limit-disconnect");
        let server = test_server_with_max_players(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
            0,
        )
        .await?;
        let handles = test_connection();
        let player = test_player_with_connection(
            &server,
            Arc::clone(&world),
            "Rejected",
            1,
            handles.connection,
        );
        let uuid = player.gameprofile.id;
        assert!(server.is_player_limit_reached(uuid));
        assert!(server.reserve_player_join(&player));
        let position = player.position();
        let state = DomainPlayerState {
            world: Arc::clone(&world),
            data: DomainPlayerData::FirstVisit {
                spawn: PreparedSpawn {
                    position,
                    rotation: (0.0, 0.0),
                },
            },
            spawn_chunk_request: world.request_player_spawn_chunks(position),
        };
        server.finish_prepared_player_join(PendingPlayerJoin {
            player: Arc::clone(&player),
            state: Ok(state),
        });

        assert_eq!(
            *handles.disconnect_reason.lock(),
            Some(TextComponent::translated(
                translations::MULTIPLAYER_DISCONNECT_SERVER_FULL.msg()
            )),
        );
        assert_eq!(server.player_count(), 0);
        assert!(!world.contains_player(&player));
        assert!(handles.sent_packets.lock().is_empty());
        assert!(!server.player_admissions.lock().contains_key(&uuid));
        assert!(server.try_reserve_player_join(uuid).is_some());
        fs::remove_dir_all(storage_root)
            .await
            .map_err(|error| error.to_string())
    })
}

#[test]
fn max_players_bypass_requires_explicit_boolean_metadata() -> Result<(), String> {
    let world = fresh_test_world("player_limit_bypass");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let storage_root = test_storage_root("player-limit-bypass");
        let mut subjects = PermissionSubjectIndex::new();
        subjects.set(
            Uuid::from_u128(1),
            PermissionSubjectState::new(vec![OP_GROUP.to_owned()], PermissionSet::new()),
        );
        let mut candidates = vec![(1, false)];
        for (id, value, bypasses) in [
            (2, PermissionMetadataValue::Bool(false), false),
            (3, PermissionMetadataValue::String("true".to_owned()), false),
            (4, PermissionMetadataValue::Bool(true), true),
        ] {
            candidates.push((id, bypasses));
            subjects.set(
                Uuid::from_u128(id),
                PermissionSubjectState::new_with_metadata(
                    Vec::new(),
                    PermissionSet::new(),
                    PermissionMetadataSet::from_entries([PermissionMetadataEntry::new(
                        Identifier::new_static(
                            Identifier::STEEL_NAMESPACE,
                            "bypasses_player_limit",
                        ),
                        value,
                    )]),
                ),
            );
        }
        let server =
            test_server_with_max_players(Arc::clone(&world), subjects, &storage_root, 0).await?;
        for (id, bypasses) in candidates {
            let uuid = Uuid::from_u128(id);
            assert_eq!(server.is_player_limit_reached(uuid), !bypasses);
            let (player, _) = test_player_with_uuid_and_packets(
                &server,
                Arc::clone(&world),
                uuid,
                "Candidate",
                id as i32,
            );
            assert!(server.reserve_player_join(&player));
            assert_eq!(
                server.admit_reserved_player(player),
                if bypasses {
                    Ok(())
                } else {
                    Err(PlayerJoinError::ServerFull)
                }
            );
        }
        assert_eq!(server.player_count(), 1);
        assert!(server.is_player_limit_reached(Uuid::from_u128(5)));
        fs::remove_dir_all(storage_root)
            .await
            .map_err(|error| error.to_string())
    })
}
