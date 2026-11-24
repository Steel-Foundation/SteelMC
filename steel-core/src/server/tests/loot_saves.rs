use std::{
    future::{Future, pending, poll_fn},
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};
use tokio::task::JoinSet;

use crate::{
    inventory::container::Container,
    player::{ResetReason, connection::NetworkConnection},
};
use steel_registry::{item_stack::ItemStack, vanilla_items};
use tokio::{runtime::Builder, sync::oneshot};

use super::{test_connection, test_player_with_connection, test_server};
use crate::{loot::*, permission::PermissionSubjectIndex, test_support::fresh_test_world};

#[test]
fn save_waits_for_delivery_and_prevents_later_loot_from_crossing_the_snapshot() {
    let fixture = fresh_test_world("loot_save_barrier");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let world = &fixture.world;
        let server = test_server(Arc::clone(world), PermissionSubjectIndex::new())
            .await
            .expect("server");
        let delivered = Arc::new(AtomicUsize::new(0));
        let (release, wait) = oneshot::channel();
        let delivery = Arc::clone(&delivered);
        let first = world
            .run_loot(
                Box::pin(async move {
                    wait.await.map_err(|_| OperationError::Cancelled)?;
                    Ok(7)
                }),
                move |result| {
                    delivery.store(result?, Ordering::Release);
                    Ok(())
                },
            )
            .expect("first loot operation");
        assert!(first.poll_result().is_pending());

        let lifecycle = Arc::clone(&world.loot_lifecycle.read());
        let mut barrier = pin!(lifecycle.save_barrier());
        let mut task = Context::from_waker(Waker::noop());
        assert!(barrier.as_mut().poll(&mut task).is_pending());
        let delivery = Arc::clone(&delivered);
        let later = world
            .run_loot(Box::pin(async { Ok(11) }), move |result| {
                delivery.store(result?, Ordering::Release);
                Ok(())
            })
            .expect("later loot operation");
        assert!(later.poll_result().is_pending());
        release.send(()).expect("release pending work");
        server.tick_jobs(1, false);
        assert!(matches!(first.poll_result(), Poll::Ready(Ok(()))));
        let Poll::Ready(snapshot_guard) = barrier.as_mut().poll(&mut task) else {
            panic!("save should proceed after delivery");
        };
        assert_eq!(delivered.load(Ordering::Acquire), 7);
        server.tick_jobs(2, false);
        assert!(later.poll_result().is_pending());
        drop(snapshot_guard);
        server.tick_jobs(3, false);
        assert!(matches!(later.poll_result(), Poll::Ready(Ok(()))));
        assert_eq!(delivered.load(Ordering::Acquire), 11);
    });
}

#[test]
fn cancelling_a_queued_operation_releases_random_ownership_and_save_admission() {
    let fixture = fresh_test_world("loot_cancel_ownership");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let world = &fixture.world;
        let server = test_server(Arc::clone(world), PermissionSubjectIndex::new())
            .await
            .expect("server");
        let request_world = Arc::clone(world);
        let completion = world
            .run_loot(
                Box::pin(async move {
                    let _random = request_world.request_loot_random(None, 0).await?;
                    pending::<OperationResult<()>>().await
                }),
                |result| result,
            )
            .expect("retained operation");
        let mut next = pin!(world.request_loot_random(None, 0));
        let mut task = Context::from_waker(Waker::noop());
        assert!(next.as_mut().poll(&mut task).is_pending());
        server.jobs.cancel_all();
        assert!(matches!(
            completion.poll_result(),
            Poll::Ready(Err(OperationError::Cancelled))
        ));
        assert!(matches!(next.as_mut().poll(&mut task), Poll::Ready(Ok(_))));
        assert!(world.loot_lifecycle.read().try_save_barrier().is_some());
    });
}

#[test]
fn disconnect_defers_its_player_snapshot_until_pending_loot_is_delivered() {
    let fixture = fresh_test_world("loot_disconnect_snapshot");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let world = &fixture.world;
        let server = test_server(Arc::clone(world), PermissionSubjectIndex::new())
            .await
            .expect("server");
        let player = test_player_with_connection(
            &server,
            Arc::clone(world),
            "LootPlayer",
            1,
            test_connection().connection,
        );
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let (release, wait) = oneshot::channel();
        let recipient = Arc::clone(&player);
        let completion = world
            .run_loot(
                Box::pin(async move {
                    wait.await.map_err(|_| OperationError::Cancelled)?;
                    Ok(ItemStack::with_count(&vanilla_items::DIAMOND, 7))
                }),
                move |result| {
                    recipient.inventory.lock().set_item(0, result?);
                    Ok(())
                },
            )
            .expect("pending delivery");
        player.connection.close();
        assert!(server.process_player_disconnects().is_empty());
        assert!(world.contains_player(&player));
        release.send(()).expect("release loot");
        server.tick_jobs(1, false);
        assert!(matches!(completion.poll_result(), Poll::Ready(Ok(()))));
        let mut disconnect_tasks = JoinSet::new();
        server.start_player_disconnect_saves(&mut disconnect_tasks);
        while let Some(result) = disconnect_tasks.join_next().await {
            result.expect("player save");
        }
        let saved = server
            .player_data_storage
            .load_domain(world.domain(), player.gameprofile.id())
            .await
            .expect("load data")
            .expect("saved player");
        let item = saved
            .inventory
            .iter()
            .find(|slot| slot.slot == 0)
            .expect("delivered slot");
        assert!(item.item.is(&vanilla_items::DIAMOND));
        assert_eq!(item.item.count(), 7);
    });
}

#[test]
fn shutdown_finishes_required_loot_before_saving_player_inventory() {
    let fixture = fresh_test_world("loot_shutdown_snapshot");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let world = &fixture.world;
        let server = test_server(Arc::clone(world), PermissionSubjectIndex::new())
            .await
            .expect("server");
        let player = test_player_with_connection(
            &server,
            Arc::clone(world),
            "LootPlayer",
            1,
            test_connection().connection,
        );
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let recipient = Arc::clone(&player);
        let mut first = true;
        let completion = world
            .run_loot(
                Box::pin(poll_fn(move |_| {
                    if first {
                        first = false;
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok(ItemStack::with_count(&vanilla_items::DIAMOND, 9)))
                })),
                move |result| {
                    recipient.inventory.lock().set_item(0, result?);
                    Ok(())
                },
            )
            .expect("pending delivery");
        assert!(completion.poll_result().is_pending());
        server.save_and_shutdown().await;
        assert!(matches!(completion.poll_result(), Poll::Ready(Ok(()))));
        let saved = server
            .player_data_storage
            .load_domain(world.domain(), player.gameprofile.id())
            .await
            .expect("load data")
            .expect("saved player");
        let item = saved
            .inventory
            .iter()
            .find(|slot| slot.slot == 0)
            .expect("delivered slot");
        assert!(item.item.is(&vanilla_items::DIAMOND));
        assert_eq!(item.item.count(), 9);
    });
}
