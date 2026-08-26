use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_protocol::packet_writer::TCPNetworkEncoder;
use steel_utils::{locks::SyncMutex, translations};
use text_components::TextComponent;
use tokio::{fs, io::AsyncWrite, runtime::Builder, sync::mpsc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    player::connection::{JavaConnection, NetworkConnection, OutboundPacket},
    player::{ClientInformation, GameProfile, Player, PlayerConnection},
    server::DuplicatePlayerWaitError,
    world::World,
};

use super::{PlayerAdmissionState, Server, fresh_test_world, test_server, test_storage_root};

fn java_test_player(
    server: &Arc<Server>,
    world: Arc<World>,
    uuid: Uuid,
) -> (Arc<Player>, mpsc::UnboundedReceiver<OutboundPacket>) {
    let (outgoing_packets, receiver) = mpsc::unbounded_channel();
    let cancel_token = CancellationToken::new();
    let player = Arc::new_cyclic(|player_weak| {
        let connection = Arc::new(PlayerConnection::Java(JavaConnection::new(
            outgoing_packets,
            cancel_token,
            None,
            1,
            player_weak.clone(),
        )));
        Player::new(
            GameProfile {
                id: uuid,
                name: "TestPlayer".to_owned(),
                properties: Vec::new(),
                profile_actions: None,
            },
            connection,
            world,
            Arc::downgrade(server),
            Arc::clone(&server.config),
            1,
            ClientInformation::default(),
        )
    });
    (player, receiver)
}

#[derive(Default)]
struct BlockingWriterState {
    released: bool,
    waker: Option<Waker>,
}

struct BlockingWriter {
    state: Arc<SyncMutex<BlockingWriterState>>,
}

impl BlockingWriter {
    fn release(&self) {
        let waker = {
            let mut state = self.state.lock();
            state.released = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl AsyncWrite for BlockingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.state.lock();
        if state.released {
            Poll::Ready(Ok(bytes.len()))
        } else {
            state.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn blocked_disconnect_write_does_not_delay_player_removal() {
    let world = fresh_test_world("blocked_disconnect_write");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let storage_root = test_storage_root("blocked-disconnect-write");
        let server = test_server(
            Arc::clone(&world),
            super::PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let (player, mut receiver) =
            java_test_player(&server, Arc::clone(&world), Uuid::from_u128(1));

        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), super::ResetReason::InitialJoin));
        let _ = player.mark_joined_world();
        assert!(player.has_joined_world());

        let writer_state = Arc::new(SyncMutex::new(BlockingWriterState::default()));
        let writer_control = BlockingWriter {
            state: Arc::clone(&writer_state),
        };
        let mut encoder = TCPNetworkEncoder::new(BlockingWriter {
            state: writer_state,
        });
        {
            let PlayerConnection::Java(connection) = player.connection.as_ref() else {
                panic!("test player should use a Java connection");
            };
            let sender = connection.send_packets(&mut encoder, &mut receiver);
            tokio::pin!(sender);

            player.disconnect("test disconnect");
            assert!(matches!(futures::poll!(&mut sender), Poll::Pending));

            let pending = server.process_player_disconnects();
            assert_eq!(pending.len(), 1);
            assert!(
                server
                    .online_players
                    .get_by_uuid(&player.gameprofile.id)
                    .is_none()
            );
            assert!(world.players.get_by_uuid(&player.gameprofile.id).is_none());
            assert!(matches!(futures::poll!(&mut sender), Poll::Pending));

            drop(pending);
            writer_control.release();
            sender.await;
        }

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

struct DisconnectRecordingConnection {
    reasons: Arc<SyncMutex<Vec<TextComponent>>>,
    closed: AtomicBool,
}

impl NetworkConnection for DisconnectRecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, _packet: EncodedPacket) {}

    fn send_encoded_batch(&self, _packets: Vec<EncodedPacket>) {}

    fn send_encoded_bundle(&self, _packets: Vec<EncodedPacket>) {}

    fn disconnect_with_reason(&self, reason: TextComponent) {
        self.reasons.lock().push(reason);
        self.close();
    }

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

#[test]
fn duplicate_login_evicts_relocating_player_and_waits_for_disconnect_admission_release() {
    let world = fresh_test_world("duplicate_relocation_wait");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let storage_root = test_storage_root("duplicate-relocation-wait");
        let server = test_server(
            Arc::clone(&world),
            super::PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let uuid = Uuid::from_u128(1);
        let reasons = Arc::new(SyncMutex::new(Vec::new()));
        let connection = Arc::new(PlayerConnection::Other(Box::new(
            DisconnectRecordingConnection {
                reasons: Arc::clone(&reasons),
                closed: AtomicBool::new(false),
            },
        )));
        let player = Arc::new(Player::new(
            GameProfile {
                id: uuid,
                name: "TestPlayer".to_owned(),
                properties: Vec::new(),
                profile_actions: None,
            },
            connection,
            Arc::clone(&world),
            Arc::downgrade(&server),
            Arc::clone(&server.config),
            1,
            ClientInformation::default(),
        ));

        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), super::ResetReason::InitialJoin));
        let _ = player.mark_joined_world();
        assert!(player.has_joined_world());
        assert!(
            server
                .player_admissions
                .lock()
                .insert(uuid, PlayerAdmissionState::Relocating)
                .is_none()
        );

        let pending = {
            let replacement_cancel = CancellationToken::new();
            let replacement =
                server.disconnect_duplicate_player_and_wait(uuid, &replacement_cancel, 601);
            tokio::pin!(replacement);

            assert!(matches!(futures::poll!(&mut replacement), Poll::Pending));
            assert_eq!(
                reasons.lock().as_slice(),
                &[TextComponent::translated(
                    translations::MULTIPLAYER_DISCONNECT_DUPLICATE_LOGIN.msg(),
                )]
            );

            server.release_player_admission(uuid, PlayerAdmissionState::Relocating);
            let pending = server.process_player_disconnects();
            assert_eq!(pending.len(), 1);
            assert_eq!(
                server.player_admissions.lock().get(&uuid),
                Some(&PlayerAdmissionState::Disconnecting)
            );
            assert!(matches!(futures::poll!(&mut replacement), Poll::Pending));

            server.release_player_admission(uuid, PlayerAdmissionState::Disconnecting);
            assert!(matches!(
                futures::poll!(&mut replacement),
                Poll::Ready(Ok(()))
            ));
            pending
        };

        let first_reservation = server.try_reserve_player_join(uuid);
        let Some(first_reservation) = first_reservation else {
            panic!("configuration should reserve the released UUID");
        };
        assert!(server.try_reserve_player_join(uuid).is_none());
        drop(first_reservation);
        let second_reservation = server.try_reserve_player_join(uuid);
        assert!(second_reservation.is_some());
        drop(second_reservation);

        drop(pending);
        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn duplicate_login_wait_matches_vanillas_deadline_ordering() {
    let world = fresh_test_world("duplicate_login_deadline");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let storage_root = test_storage_root("duplicate-login-deadline");
        let server = test_server(
            Arc::clone(&world),
            super::PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let uuid = Uuid::from_u128(1);
        assert!(
            server
                .player_admissions
                .lock()
                .insert(uuid, PlayerAdmissionState::Disconnecting)
                .is_none()
        );

        {
            let replacement_cancel = CancellationToken::new();
            let replacement =
                server.disconnect_duplicate_player_and_wait(uuid, &replacement_cancel, 601);
            tokio::pin!(replacement);

            assert!(matches!(futures::poll!(&mut replacement), Poll::Pending));
            for _ in 0..600 {
                let _ = server.advance_server_tick();
            }
            assert_eq!(server.current_tick(), 600);
            assert!(matches!(futures::poll!(&mut replacement), Poll::Pending));

            let _ = server.advance_server_tick();
            assert!(matches!(
                futures::poll!(&mut replacement),
                Poll::Ready(Err(DuplicatePlayerWaitError::TimedOut))
            ));
        }

        server.release_player_admission(uuid, PlayerAdmissionState::Disconnecting);

        let boundary_uuid = Uuid::from_u128(2);
        assert!(
            server
                .player_admissions
                .lock()
                .insert(boundary_uuid, PlayerAdmissionState::Disconnecting)
                .is_none()
        );
        {
            let replacement_cancel = CancellationToken::new();
            let replacement = server.disconnect_duplicate_player_and_wait(
                boundary_uuid,
                &replacement_cancel,
                602,
            );
            tokio::pin!(replacement);

            assert!(matches!(futures::poll!(&mut replacement), Poll::Pending));
            server.release_player_admission(boundary_uuid, PlayerAdmissionState::Disconnecting);
            let _ = server.advance_server_tick();
            assert!(matches!(
                futures::poll!(&mut replacement),
                Poll::Ready(Ok(()))
            ));
        }

        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}
