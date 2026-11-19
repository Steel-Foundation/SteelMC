use std::{
    future::Future,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

use steel_registry::{
    RegistryExt, data_components::vanilla_components::SeededContainerLoot, vanilla_blocks,
    vanilla_entities,
};
use steel_utils::{BlockPos, ChunkPos, Identifier, WorldAabb, types::UpdateFlags};
use tokio::runtime::Builder;

use super::{test_connection, test_player_with_connection, test_server};
use crate::{
    bootstrap::init_globals,
    entity::Entity,
    inventory::lock::ContainerLockGuard,
    loot::{LOOT_TABLES, OperationError},
    permission::PermissionSubjectIndex,
    player::ResetReason,
    test_support::{fresh_test_world, insert_ready_full_chunk, use_block},
};

const POS: BlockPos = BlockPos::new(8, 64, 8);

#[test]
fn concurrent_container_access_joins_one_generation_and_waits_without_inventory_locks() {
    init_globals();
    let fixture = fresh_test_world("loot_container_join");
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let world = &fixture.world;
            let server = test_server(Arc::clone(world), PermissionSubjectIndex::new())
                .await
                .expect("server");
            insert_ready_full_chunk(world, ChunkPos::from_block_pos(POS));
            assert!(world.set_block(
                POS,
                vanilla_blocks::BARREL.default_state(),
                UpdateFlags::UPDATE_NONE
            ));
            let owner = world.get_block_entity(POS).expect("barrel");
            let loot = owner.as_randomizable_container().expect("loot capability");
            loot.set_loot_table(Some(SeededContainerLoot::new(
                Identifier::vanilla_static("chests/spawn_bonus_chest"),
                12345,
            )));
            let lifecycle = Arc::clone(&world.loot_lifecycle.read());
            let saving = lifecycle.save_barrier().await;
            let Err(OperationError::Pending(first)) = loot.unpack_loot_table(None) else {
                panic!("access must wait for save")
            };
            let Err(OperationError::Pending(second)) = loot.unpack_loot_table(None) else {
                panic!("second access must join")
            };
            assert_eq!(server.jobs.len(), 1);
            let container = owner.container_ref().expect("inventory");
            assert!(matches!(
                ContainerLockGuard::lock_all(&[&container]),
                Err(OperationError::Pending(_))
            ));
            let deliveries = Arc::new(AtomicUsize::new(0));
            for completion in [&first, &second] {
                let deliveries = Arc::clone(&deliveries);
                completion.on_complete(move |result| {
                    result.expect("generation");
                    deliveries.fetch_add(1, Ordering::Relaxed);
                });
            }
            drop(saving);
            server.tick_jobs(1, false);
            assert!(matches!(first.poll_result(), Poll::Ready(Ok(()))));
            assert_eq!(deliveries.load(Ordering::Relaxed), 2);
            assert!(loot.loot_table().is_none());
            let guard = ContainerLockGuard::lock_all(&[&container]).expect("ready inventory");
            assert!(
                !guard
                    .get(container.container_id())
                    .expect("container")
                    .is_empty()
            );
        });
}

#[test]
fn removal_delivers_pending_contents_before_a_save_can_snapshot_entities() {
    init_globals();
    let fixture = fresh_test_world("loot_container_removal");
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let world = &fixture.world;
            let server = test_server(Arc::clone(world), PermissionSubjectIndex::new())
                .await
                .expect("server");
            insert_ready_full_chunk(world, ChunkPos::from_block_pos(POS));
            assert!(world.set_block(
                POS,
                vanilla_blocks::BARREL.default_state(),
                UpdateFlags::UPDATE_NONE
            ));
            let owner = world.get_block_entity(POS).expect("barrel");
            let loot = owner.as_randomizable_container().expect("loot");
            let key = Identifier::vanilla_static("chests/spawn_bonus_chest");
            let reservation = world
                .request_loot_random(LOOT_TABLES.by_key(&key), 0)
                .await
                .expect("reserve stream");
            loot.set_loot_table(Some(SeededContainerLoot::new(key, 0)));
            assert!(matches!(
                loot.unpack_loot_table(None),
                Err(OperationError::Pending(_))
            ));
            owner.pre_remove_side_effects(POS, vanilla_blocks::BARREL.default_state());
            let lifecycle = Arc::clone(&world.loot_lifecycle.read());
            let mut save = pin!(lifecycle.save_barrier());
            let mut task = Context::from_waker(Waker::noop());
            assert!(save.as_mut().poll(&mut task).is_pending());
            drop(reservation);
            server.tick_jobs(1, false);
            let Poll::Ready(_snapshot) = save.as_mut().poll(&mut task) else {
                panic!("loot must finish")
            };
            let bounds = WorldAabb::new(0.0, 60.0, 0.0, 16.0, 70.0, 16.0);
            assert!(
                !world
                    .get_entities_in_aabb_matching(&bounds, |entity| entity.entity_type()
                        == &vanilla_entities::ITEM)
                    .is_empty()
            );
            let container = owner.container_ref().expect("inventory");
            let guard = ContainerLockGuard::lock_all(&[&container]).expect("ready");
            assert!(
                guard
                    .get(container.container_id())
                    .expect("container")
                    .is_empty()
            );
        });
}

#[test]
fn pending_barrel_open_resumes_once_and_close_supersedes_it() {
    for cancel in [false, true] {
        init_globals();
        let fixture = fresh_test_world("loot_container_open");
        Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async {
                let world = &fixture.world;
                let server = test_server(Arc::clone(world), PermissionSubjectIndex::new())
                    .await
                    .expect("server");
                insert_ready_full_chunk(world, ChunkPos::from_block_pos(POS));
                assert!(world.set_block(
                    POS,
                    vanilla_blocks::BARREL.default_state(),
                    UpdateFlags::UPDATE_NONE
                ));
                let player = test_player_with_connection(
                    &server,
                    Arc::clone(world),
                    "LootOpener",
                    1,
                    test_connection().connection,
                );
                player.set_client_loaded(true);
                player
                    .try_set_position(POS.get_center())
                    .expect("player position");
                assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
                let owner = world.get_block_entity(POS).expect("barrel");
                let loot = owner.as_randomizable_container().expect("loot");
                let key = Identifier::vanilla_static("chests/spawn_bonus_chest");
                let reservation = world
                    .request_loot_random(LOOT_TABLES.by_key(&key), 0)
                    .await
                    .expect("reserve");
                loot.set_loot_table(Some(SeededContainerLoot::new(key, 0)));
                use_block(world, POS, &player);
                assert!(!player.has_container_open());
                if cancel {
                    player.close_container();
                }
                drop(reservation);
                server.tick_jobs(1, false);
                assert_eq!(player.has_container_open(), !cancel);
                assert!(loot.loot_table().is_none());
            });
    }
}
