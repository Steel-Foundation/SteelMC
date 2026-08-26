//! Pre-play TCP client connection handler.
//!
//! Handles the connection lifecycle from handshake through login and configuration,
//! until the connection is upgraded to play state.

use std::{
    cmp::Ordering,
    fmt::{self, Debug, Formatter},
    future::{Future, pending},
    io::Cursor,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
};

use crossbeam::atomic::AtomicCell;
use steel_core::player::{
    ClientInformation, PlayerConnection,
    connection::{GRACEFUL_CLOSE_TIMEOUT, JavaPacketWriter, OutboundPacket},
};
use steel_core::server::Server;
use steel_protocol::{
    packet_reader::TCPNetworkDecoder,
    packet_traits::{ClientPacket, CompressionInfo, EncodedPacket, ServerPacket},
    packet_writer::TCPNetworkEncoder,
    packets::{
        common::{CDisconnect, SClientInformation, SCustomPayload, SPingRequest},
        config::SSelectKnownPacks,
        handshake::{ClientIntent, SClientIntention},
        login::{CLoginDisconnect, SHello, SKey},
    },
    utils::{ConnectionProtocol, PacketError, RawPacket},
};
use steel_registry::packets::{
    CURRENT_MC_PROTOCOL, config, handshake, login as login_packets, status,
};
use steel_utils::{
    MC_VERSION,
    locks::{AsyncMutex, SyncMutex},
    translations,
};
use text_components::{
    TextComponent, content::Resolvable, custom::CustomData, resolving::TextResolutor,
};
use tokio::{
    io::{AsyncWrite, BufReader, BufWriter},
    net::{TcpStream, tcp::OwnedReadHalf},
    select,
    sync::{
        Notify,
        broadcast::{self, Sender, error::RecvError},
        mpsc::{self, UnboundedReceiver, UnboundedSender, error::TryRecvError},
    },
    task::yield_now,
    time::sleep,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use uuid::Uuid;

use crate::pre_play_state::{PacketSequenceError, PrePlayPacket, PrePlayState};

const MAX_TICKS_BEFORE_LOGIN: u64 = 600;
const MAX_CLOSE_SCAN_ITEMS_PER_QUANTUM: usize = 32;

type JavaNetworkWriter = Arc<AsyncMutex<Option<JavaPacketWriter>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LoginDeadline {
    expires_at_tick: u64,
}

impl LoginDeadline {
    const fn from_start_tick(start_tick: u64) -> Self {
        Self {
            // Vanilla initializes its counter to zero and checks `tick++ == 600`.
            expires_at_tick: start_tick.saturating_add(MAX_TICKS_BEFORE_LOGIN + 1),
        }
    }

    pub(crate) const fn expires_at_tick(self) -> u64 {
        self.expires_at_tick
    }
}

enum LoginOperationResult<T> {
    Completed(T),
    Cancelled,
    TimedOut,
}

enum CloseDeadlineResult<T> {
    Completed(T),
    Elapsed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OutboundEncryptionState {
    #[default]
    Plaintext,
    Requested,
    Enabled,
}

#[derive(Default)]
struct OutboundEncryptionTransition {
    state: AtomicCell<OutboundEncryptionState>,
    enabled: Notify,
}

impl OutboundEncryptionTransition {
    fn begin(&self) {
        self.state.store(OutboundEncryptionState::Requested);
    }

    fn finish(&self) {
        self.state.store(OutboundEncryptionState::Enabled);
        self.enabled.notify_waiters();
    }

    fn is_pending(&self) -> bool {
        self.state.load() == OutboundEncryptionState::Requested
    }

    async fn wait_until_enabled(&self) {
        loop {
            let notified = self.enabled.notified();
            if self.state.load() == OutboundEncryptionState::Enabled {
                return;
            }
            notified.await;
        }
    }
}

async fn complete_before_deadline<T, O, D>(
    operation: O,
    mut deadline: Pin<&mut D>,
) -> CloseDeadlineResult<T>
where
    O: Future<Output = T>,
    D: Future<Output = ()> + ?Sized,
{
    tokio::pin!(operation);
    select! {
        biased;
        () = deadline.as_mut() => CloseDeadlineResult::Elapsed,
        output = operation.as_mut() => CloseDeadlineResult::Completed(output),
    }
}

enum IncomingEvent {
    Packet(Result<RawPacket, PacketError>),
    ConnectionUpdate(Result<ConnectionUpdate, RecvError>),
}

async fn await_login_operation<T, O, D>(
    cancel_token: &CancellationToken,
    login_deadline: &AtomicCell<Option<LoginDeadline>>,
    operation: O,
    deadline: D,
) -> LoginOperationResult<T>
where
    O: Future<Output = T>,
    D: Future<Output = ()>,
{
    tokio::pin!(operation);
    tokio::pin!(deadline);

    tokio::select! {
        biased;
        () = cancel_token.cancelled() => LoginOperationResult::Cancelled,
        result = &mut operation => LoginOperationResult::Completed(result),
        () = &mut deadline => {
            if login_deadline.load().is_some() {
                LoginOperationResult::TimedOut
            } else {
                tokio::select! {
                    biased;
                    () = cancel_token.cancelled() => LoginOperationResult::Cancelled,
                    result = &mut operation => LoginOperationResult::Completed(result),
                }
            }
        }
    }
}

/// Represents updates to the connection state.
#[derive(Clone)]
pub enum ConnectionUpdate {
    /// Enable encryption on the connection.
    EnableEncryption([u8; 16]),
    /// Upgrade the connection to the play state.
    Upgrade(Arc<PlayerConnection>),
}

enum PrePlayWrite {
    Packet(EncodedPacket),
    Batch(Vec<EncodedPacket>),
}

struct PrePlayClosingWrites {
    queued: Vec<PrePlayWrite>,
    disconnect: EncodedPacket,
}

impl Debug for ConnectionUpdate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnableEncryption(_) => f.write_str("EnableEncryption(<redacted>)"),
            Self::Upgrade(_) => f.debug_tuple("Upgrade").finish(),
        }
    }
}

/// Session id owned by the active server connection listener
#[derive(Default)]
pub struct ServerConnectionSession {
    session_id: SyncMutex<Option<Uuid>>,
}

impl ServerConnectionSession {
    /// Returns the listener session id generating it on first use
    #[must_use]
    pub fn session_id(&self) -> Uuid {
        let mut session_id = self.session_id.lock();
        *session_id.get_or_insert_with(Uuid::new_v4)
    }
}

#[derive(Default)]
pub(crate) struct ConnectionAction {
    reader_encryption: Option<[u8; 16]>,
    reader_compression: Option<CompressionInfo>,
    upgrade: Option<Arc<PlayerConnection>>,
}

impl ConnectionAction {
    pub(crate) const fn none() -> Self {
        Self {
            reader_encryption: None,
            reader_compression: None,
            upgrade: None,
        }
    }

    pub(crate) const fn reader_compression(compression: CompressionInfo) -> Self {
        Self {
            reader_encryption: None,
            reader_compression: Some(compression),
            upgrade: None,
        }
    }

    pub(crate) const fn upgrade(connection: Arc<PlayerConnection>) -> Self {
        Self {
            reader_encryption: None,
            reader_compression: None,
            upgrade: Some(connection),
        }
    }

    pub(crate) const fn with_reader_encryption(mut self, key: [u8; 16]) -> Self {
        self.reader_encryption = Some(key);
        self
    }
}

/// Connection for pre-play packets.
///
/// Gets dropped by `incoming_packet_task` if closed or upgraded to play connection.
pub struct JavaTcpClient {
    /// The unique ID of the client.
    pub id: u64,
    /// The client's settings (view distance, language, etc.) received during config.
    pub client_information: AsyncMutex<ClientInformation>,
    /// The current connection state of the client (e.g., Handshaking, Status, Play).
    pub protocol: Arc<AtomicCell<ConnectionProtocol>>,
    /// The client's IP address.
    pub address: SocketAddr,
    /// A token to cancel the client's operations. Called when the connection is closed.
    pub cancel_token: CancellationToken,

    /// A queue of encoded packets to send to the network.
    pub outgoing_queue: UnboundedSender<OutboundPacket>,
    /// The packet encoder for outgoing packets.
    network_writer: JavaNetworkWriter,
    /// Current compression settings.
    pub compression: Arc<AtomicCell<Option<CompressionInfo>>>,

    /// The shared server state.
    pub server: Arc<Server>,
    /// The session id state for the active server connection listener
    pub connection_session: Arc<ServerConnectionSession>,
    /// The challenge sent to the client during login.
    pub challenge: AtomicCell<[u8; 4]>,

    /// Channel for broadcasting connection state updates.
    pub connection_updates: Sender<ConnectionUpdate>,
    /// Notification that the outbound encoder has moved into the play sender.
    pub(crate) connection_upgraded: Arc<Notify>,
    outbound_encryption: Arc<OutboundEncryptionTransition>,

    pub(crate) pre_play_state: SyncMutex<PrePlayState>,
    pub(crate) login_deadline: AtomicCell<Option<LoginDeadline>>,
    task_tracker: TaskTracker,
}

impl JavaTcpClient {
    /// Creates a new `JavaTcpClient`.
    #[must_use]
    pub fn new(
        tcp_stream: TcpStream,
        address: SocketAddr,
        id: u64,
        cancel_token: CancellationToken,
        server: Arc<Server>,
        connection_session: Arc<ServerConnectionSession>,
        task_tracker: TaskTracker,
    ) -> (
        Self,
        UnboundedReceiver<OutboundPacket>,
        TCPNetworkDecoder<BufReader<OwnedReadHalf>>,
    ) {
        let (read, write) = tcp_stream.into_split();
        let (outgoing_queue, recv) = mpsc::unbounded_channel();
        let (connection_updates, _) = broadcast::channel(128);

        let client = Self {
            id,
            client_information: AsyncMutex::new(ClientInformation::default()),
            address,
            protocol: Arc::new(AtomicCell::new(ConnectionProtocol::Handshake)),
            cancel_token,

            outgoing_queue,
            network_writer: Arc::new(AsyncMutex::new(Some(TCPNetworkEncoder::new(
                BufWriter::new(write),
            )))),
            compression: Arc::new(AtomicCell::new(None)),
            server,
            connection_session,
            challenge: AtomicCell::new([0; 4]),
            connection_updates,
            connection_upgraded: Arc::new(Notify::new()),
            outbound_encryption: Arc::new(OutboundEncryptionTransition::default()),
            pre_play_state: SyncMutex::new(PrePlayState::new()),
            login_deadline: AtomicCell::new(None),
            task_tracker,
        };

        (client, recv, TCPNetworkDecoder::new(BufReader::new(read)))
    }

    /// Closes the connection.
    pub fn close(&self) {
        self.cancel_token.cancel();
    }

    pub(crate) fn begin_outbound_encryption(&self) {
        self.outbound_encryption.begin();
    }

    pub(crate) async fn wait_for_outbound_encryption(&self) {
        self.outbound_encryption.wait_until_enabled().await;
    }

    /// Sends a packet immediately, without queuing.
    ///
    /// # Panics
    /// This function will panic if the packet cannot be encoded. Should never happen.
    pub async fn send_bare_packet_now<P: ClientPacket>(&self, packet: P) {
        let compression = self.compression.load();
        let protocol = self.protocol.load();
        let packet = EncodedPacket::from_bare(packet, compression, protocol)
            .expect("Failed to encode packet");

        if let Err(err) = Self::write_network_packet(&self.network_writer, &packet).await
            && !self.cancel_token.is_cancelled()
        {
            log::warn!("Failed to send packet to client {}: {}", self.id, err);
            self.close();
        }
    }

    /// Sends an already encoded packet immediately, without queuing.
    pub async fn send_packet_now(&self, packet: &EncodedPacket) {
        if let Err(err) = Self::write_network_packet(&self.network_writer, packet).await
            && !self.cancel_token.is_cancelled()
        {
            log::warn!("Failed to send packet to client {}: {}", self.id, err);
            self.close();
        }
    }

    async fn write_network_packet<W>(
        network_writer: &Arc<AsyncMutex<Option<TCPNetworkEncoder<W>>>>,
        packet: &EncodedPacket,
    ) -> Result<(), PacketError>
    where
        W: AsyncWrite + Unpin,
    {
        let mut network_writer = network_writer.lock().await;
        let Some(mut writer) = network_writer.take() else {
            return Err(PacketError::ConnectionClosed);
        };
        // Cancellation or failure drops the encoder instead of reusing a partial encrypted write.
        match writer.write_packet(packet).await {
            Ok(()) => {
                *network_writer = Some(writer);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn write_network_batch<W>(
        network_writer: &Arc<AsyncMutex<Option<TCPNetworkEncoder<W>>>>,
        packets: &[EncodedPacket],
    ) -> Result<(), PacketError>
    where
        W: AsyncWrite + Unpin,
    {
        let mut network_writer = network_writer.lock().await;
        let Some(mut writer) = network_writer.take() else {
            return Err(PacketError::ConnectionClosed);
        };
        // Pre-play preserves immediate flushes while holding exclusive ownership across the group.
        for packet in packets {
            writer.write_packet(packet).await?;
        }
        *network_writer = Some(writer);
        Ok(())
    }

    // A concurrent direct write owns and will either restore or drop the encoder itself.
    fn try_release_network_writer(network_writer: &JavaNetworkWriter) {
        if let Ok(mut writer) = network_writer.try_lock() {
            writer.take();
        }
    }

    /// Queues an already encoded packet to be sent.
    pub fn send_packet(&self, packet: EncodedPacket) -> Result<(), PacketError> {
        self.outgoing_queue
            .send(OutboundPacket::Packet(packet))
            .map_err(|e| {
                PacketError::SendError(format!(
                    "Failed to send packet to client {}: {}",
                    self.id, e
                ))
            })?;
        Ok(())
    }

    /// Starts a task that will send packets to the client from the outgoing packet queue.
    /// On play-state upgrade, it transfers the encoder and queue to the play sender.
    #[expect(
        clippy::too_many_lines,
        reason = "keeping the pre-play ownership and cancellation state machine linear makes its ordering auditable"
    )]
    pub fn start_outgoing_packet_task(
        self: &Arc<Self>,
        mut sender_recv: UnboundedReceiver<OutboundPacket>,
    ) {
        let cancel_token = self.cancel_token.clone();
        let network_writer = self.network_writer.clone();
        let id = self.id;
        let mut connection_updates_recv = self.connection_updates.subscribe();
        let connection_upgraded = self.connection_upgraded.clone();
        let outbound_encryption = Arc::clone(&self.outbound_encryption);

        self.task_tracker.spawn(async move {
            let mut play_connection = None;
            loop {
                select! {
                    biased;
                    () = cancel_token.cancelled() => {
                        let mut deadline = Box::pin(sleep(GRACEFUL_CLOSE_TIMEOUT));
                        let closing = match complete_before_deadline(
                            Self::take_closing_writes(&mut sender_recv),
                            deadline.as_mut(),
                        ).await {
                            CloseDeadlineResult::Completed(closing) => closing,
                            CloseDeadlineResult::Elapsed => {
                                Self::log_close_timeout(id);
                                break;
                            }
                        };
                        if let Some(closing) = closing {
                            Self::write_closing_before_deadline(
                                &network_writer,
                                closing,
                                id,
                                deadline.as_mut(),
                            ).await;
                        }
                        break;
                    }
                    outbound = sender_recv.recv() => {
                        let Some(outbound) = outbound else {
                            cancel_token.cancel();
                            break;
                        };
                        let queued_write = match outbound {
                            OutboundPacket::Packet(packet) => PrePlayWrite::Packet(packet),
                            OutboundPacket::PacketBatch(packets) => PrePlayWrite::Batch(*packets),
                            OutboundPacket::Disconnect(disconnect) => {
                                sender_recv.close();
                                cancel_token.cancel();
                                let mut deadline = Box::pin(sleep(GRACEFUL_CLOSE_TIMEOUT));
                                Self::write_closing_before_deadline(
                                    &network_writer,
                                    PrePlayClosingWrites {
                                        queued: Vec::new(),
                                        disconnect,
                                    },
                                    id,
                                    deadline.as_mut(),
                                ).await;
                                break;
                            }
                        };

                        let write_result = Self::write_pre_play(&network_writer, queued_write);
                        tokio::pin!(write_result);
                        select! {
                            biased;
                            () = cancel_token.cancelled() => {
                                let mut deadline = Box::pin(sleep(GRACEFUL_CLOSE_TIMEOUT));
                                let closing = match complete_before_deadline(
                                    Self::take_closing_writes(&mut sender_recv),
                                    deadline.as_mut(),
                                ).await {
                                    CloseDeadlineResult::Completed(closing) => closing,
                                    CloseDeadlineResult::Elapsed => {
                                        Self::log_close_timeout(id);
                                        break;
                                    }
                                };
                                let Some(closing) = closing else {
                                    break;
                                };
                                match complete_before_deadline(
                                    write_result.as_mut(),
                                    deadline.as_mut(),
                                ).await {
                                    CloseDeadlineResult::Completed(Ok(())) => {}
                                    CloseDeadlineResult::Completed(Err(error)) => {
                                        log::debug!(
                                            "Best-effort close for client {id} failed while finishing the active outbound write: {error}"
                                        );
                                        break;
                                    }
                                    CloseDeadlineResult::Elapsed => {
                                        Self::log_close_timeout(id);
                                        break;
                                    }
                                }
                                Self::write_closing_before_deadline(
                                    &network_writer,
                                    closing,
                                    id,
                                    deadline.as_mut(),
                                ).await;
                                break;
                            },
                            result = write_result.as_mut() => {
                                if let Err(error) = result {
                                    log::warn!("Failed to send outbound write to client {id}: {error}");
                                    cancel_token.cancel();
                                    break;
                                }
                            }
                        }
                    }
                    connection_update = connection_updates_recv.recv() => {
                        match connection_update {
                            Ok(connection_update) => {
                                match connection_update {
                                    ConnectionUpdate::EnableEncryption(key) => {
                                        let mut slot = select! {
                                            biased;
                                            () = cancel_token.cancelled() => break,
                                            slot = network_writer.lock() => slot,
                                        };
                                        let Some(writer) = slot.as_mut() else {
                                            cancel_token.cancel();
                                            continue;
                                        };
                                        writer.set_encryption(&key);
                                        drop(slot);
                                        outbound_encryption.finish();
                                    },
                                    ConnectionUpdate::Upgrade(upgrade) => {
                                        let mut slot = select! {
                                            biased;
                                            () = cancel_token.cancelled() => break,
                                            slot = network_writer.lock() => slot,
                                        };
                                        let Some(writer) = slot.take() else {
                                            cancel_token.cancel();
                                            break;
                                        };
                                        drop(slot);
                                        play_connection = Some((upgrade, writer));
                                        connection_upgraded.notify_one();
                                        break;
                                    }
                                }
                            }
                            Err(err) => {
                                if err != RecvError::Closed {
                                    log::warn!("Internal connection_updates_recv channel closed for client {id}: {err}");
                                }
                                cancel_token.cancel();
                            }
                        }
                    }
                }
            }

            drop(cancel_token);
            drop(connection_updates_recv);
            drop(connection_upgraded);
            drop(outbound_encryption);

            if let Some((connection, writer)) = play_connection {
                drop(network_writer);
                match &*connection {
                    PlayerConnection::Java(java) => java.sender(sender_recv, writer).await,
                    PlayerConnection::Other(_) => unreachable!("Expected Java connection"),
                }
            } else {
                Self::try_release_network_writer(&network_writer);
                drop(network_writer);
            }
        });
    }

    async fn write_pre_play(
        network_writer: &JavaNetworkWriter,
        queued_write: PrePlayWrite,
    ) -> Result<(), PacketError> {
        match queued_write {
            PrePlayWrite::Packet(packet) => {
                Self::write_network_packet(network_writer, &packet).await
            }
            PrePlayWrite::Batch(packets) => {
                Self::write_network_batch(network_writer, &packets).await
            }
        }
    }

    async fn take_closing_writes(
        sender_recv: &mut UnboundedReceiver<OutboundPacket>,
    ) -> Option<PrePlayClosingWrites> {
        sender_recv.close();
        let mut queued = Vec::new();
        let mut scanned_since_yield = 0usize;
        loop {
            let queued_write = match sender_recv.try_recv() {
                Ok(OutboundPacket::Packet(packet)) => PrePlayWrite::Packet(packet),
                Ok(OutboundPacket::PacketBatch(batch)) => PrePlayWrite::Batch(*batch),
                Ok(OutboundPacket::Disconnect(disconnect)) => {
                    return Some(PrePlayClosingWrites { queued, disconnect });
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
            };
            queued.push(queued_write);
            scanned_since_yield += 1;
            if scanned_since_yield == MAX_CLOSE_SCAN_ITEMS_PER_QUANTUM {
                scanned_since_yield = 0;
                yield_now().await;
            }
        }
    }

    async fn write_closing_before_deadline<W, D>(
        network_writer: &Arc<AsyncMutex<Option<TCPNetworkEncoder<W>>>>,
        closing: PrePlayClosingWrites,
        id: u64,
        mut deadline: Pin<&mut D>,
    ) where
        W: AsyncWrite + Unpin,
        D: Future<Output = ()> + ?Sized,
    {
        let mut slot =
            match complete_before_deadline(network_writer.lock(), deadline.as_mut()).await {
                CloseDeadlineResult::Completed(slot) => slot,
                CloseDeadlineResult::Elapsed => {
                    Self::log_close_timeout(id);
                    return;
                }
            };
        let Some(mut writer) = slot.take() else {
            return;
        };
        drop(slot);

        for queued_write in closing.queued {
            match queued_write {
                PrePlayWrite::Packet(packet) => {
                    if !Self::write_before_deadline(&mut writer, &packet, id, deadline.as_mut())
                        .await
                    {
                        return;
                    }
                }
                PrePlayWrite::Batch(packets) => {
                    for packet in packets {
                        if !Self::write_before_deadline(&mut writer, &packet, id, deadline.as_mut())
                            .await
                        {
                            return;
                        }
                    }
                }
            }
        }
        let _ = Self::write_before_deadline(&mut writer, &closing.disconnect, id, deadline).await;
    }

    async fn write_before_deadline<W, D>(
        writer: &mut TCPNetworkEncoder<W>,
        packet: &EncodedPacket,
        id: u64,
        deadline: Pin<&mut D>,
    ) -> bool
    where
        W: AsyncWrite + Unpin,
        D: Future<Output = ()> + ?Sized,
    {
        match complete_before_deadline(writer.write_packet(packet), deadline).await {
            CloseDeadlineResult::Completed(Ok(())) => true,
            CloseDeadlineResult::Completed(Err(error)) => {
                log::debug!("Best-effort close for client {id} failed: {error}");
                false
            }
            CloseDeadlineResult::Elapsed => {
                Self::log_close_timeout(id);
                false
            }
        }
    }

    fn log_close_timeout(id: u64) {
        log::debug!("Best-effort graceful close for client {id} timed out");
    }

    /// Starts a task that will receive packets from the client.
    /// This task will run until the client is closed or the cancellation token is cancelled.
    pub fn start_incoming_packet_task(
        self: &Arc<Self>,
        mut reader: TCPNetworkDecoder<BufReader<OwnedReadHalf>>,
    ) {
        let cancel_token = self.cancel_token.clone();
        let id = self.id;
        let mut connection_updates_recv = self.connection_updates.subscribe();

        let self_clone = self.clone();

        self.task_tracker.spawn(async move {
            let mut connection = None;
            loop {
                let incoming_event = if self_clone.login_deadline_expired() {
                    LoginOperationResult::TimedOut
                } else {
                    let incoming_event = async {
                        select! {
                            packet = reader.get_raw_packet() => IncomingEvent::Packet(packet),
                            connection_update = connection_updates_recv.recv() => {
                                IncomingEvent::ConnectionUpdate(connection_update)
                            }
                        }
                    };
                    await_login_operation(
                        &cancel_token,
                        &self_clone.login_deadline,
                        incoming_event,
                        self_clone.wait_for_login_deadline(),
                    )
                    .await
                };

                match incoming_event {
                    LoginOperationResult::Completed(IncomingEvent::Packet(Ok(packet))) => {
                        match self_clone.process_packet_until_login_deadline(packet).await {
                            LoginOperationResult::Completed(Ok(action)) => {
                                if self_clone.login_deadline_expired() {
                                    self_clone.disconnect_slow_login().await;
                                    break;
                                }
                                if let Some(key) = action.reader_encryption {
                                    reader.set_encryption(&key);
                                }
                                if let Some(compression) = action.reader_compression {
                                    reader.set_compression(compression.threshold);
                                }
                                if let Some(upgrade) = action.upgrade {
                                    connection = Some(upgrade);
                                    break;
                                }
                            }
                            LoginOperationResult::Completed(Err(err)) => {
                                log::warn!("Failed to get packet from client {id}: {err}");
                            }
                            LoginOperationResult::Cancelled => break,
                            LoginOperationResult::TimedOut => {
                                self_clone.disconnect_slow_login().await;
                                break;
                            }
                        }
                    }
                    LoginOperationResult::Completed(IncomingEvent::Packet(Err(err))) => {
                        log::info!("Failed to get raw packet from client {id}: {err}");
                        cancel_token.cancel();
                    }
                    LoginOperationResult::Completed(IncomingEvent::ConnectionUpdate(
                        connection_update,
                    )) => match connection_update {
                        Ok(ConnectionUpdate::EnableEncryption(_)) => {}
                        Ok(ConnectionUpdate::Upgrade(upgrade)) => {
                            connection = Some(upgrade);
                            break;
                        }
                        Err(err) => {
                            if err != RecvError::Closed {
                                log::info!(
                                    "Internal connection_updates_recv channel closed for client {id}: {err}"
                                );
                            }
                            cancel_token.cancel();
                        }
                    },
                    LoginOperationResult::Cancelled => break,
                    LoginOperationResult::TimedOut => {
                        if self_clone.login_deadline_expired() {
                            self_clone.disconnect_slow_login().await;
                        }
                        break;
                    }
                }
            }

            drop(cancel_token);
            drop(connection_updates_recv);

            if let Some(connection) = connection {
                let server = self_clone.server.clone();
                drop(self_clone);

                match &*connection {
                    PlayerConnection::Java(java) => java.listener(reader, server).await,
                    PlayerConnection::Other(_) => unreachable!("Expected Java connection"),
                }
            }
        });
    }

    fn login_deadline_expired(&self) -> bool {
        self.login_deadline
            .load()
            .is_some_and(|deadline| self.server.current_tick() >= deadline.expires_at_tick())
    }

    async fn wait_for_login_deadline(&self) {
        match self.login_deadline.load() {
            Some(deadline) => {
                self.server
                    .wait_until_tick(deadline.expires_at_tick())
                    .await;
            }
            None => pending().await,
        }
    }

    async fn process_packet_until_login_deadline(
        &self,
        packet: RawPacket,
    ) -> LoginOperationResult<Result<ConnectionAction, PacketError>> {
        if self.login_deadline_expired() {
            return LoginOperationResult::TimedOut;
        }

        await_login_operation(
            &self.cancel_token,
            &self.login_deadline,
            self.process_packet(packet),
            self.wait_for_login_deadline(),
        )
        .await
    }

    pub(crate) async fn disconnect_slow_login(&self) {
        let reason =
            TextComponent::translated(translations::MULTIPLAYER_DISCONNECT_SLOW_LOGIN.msg());
        let mut deadline = Box::pin(sleep(GRACEFUL_CLOSE_TIMEOUT));
        if self.outbound_encryption.is_pending()
            && matches!(
                complete_before_deadline(
                    self.outbound_encryption.wait_until_enabled(),
                    deadline.as_mut(),
                )
                .await,
                CloseDeadlineResult::Elapsed
            )
        {
            self.log_slow_login_close_timeout();
            self.close();
            return;
        }

        if matches!(
            complete_before_deadline(self.kick(reason), deadline.as_mut()).await,
            CloseDeadlineResult::Elapsed
        ) {
            self.log_slow_login_close_timeout();
            self.close();
        }
    }

    fn log_slow_login_close_timeout(&self) {
        log::debug!(
            "Best-effort slow-login disconnect write for client {} timed out",
            self.id
        );
    }

    async fn process_packet(&self, packet: RawPacket) -> Result<ConnectionAction, PacketError> {
        match self.protocol.load() {
            ConnectionProtocol::Handshake => {
                self.handle_handshake(packet).await?;
                Ok(ConnectionAction::none())
            }
            ConnectionProtocol::Status => {
                self.handle_status(packet).await?;
                Ok(ConnectionAction::none())
            }
            ConnectionProtocol::Login => self.handle_login(packet).await,
            ConnectionProtocol::Config => self.handle_config(packet).await,
            ConnectionProtocol::Play => Err(PacketError::InvalidProtocol("Play".to_string())),
        }
    }

    /// Handles a handshake packet.
    pub async fn handle_handshake(&self, packet: RawPacket) -> Result<(), PacketError> {
        let data = &mut Cursor::new(packet.payload());

        match packet.id {
            handshake::S_INTENTION => {
                let packet = SClientIntention::read_packet(data)?;
                let intent = match packet.intention {
                    ClientIntent::Status => ConnectionProtocol::Status,
                    ClientIntent::Login | ClientIntent::Transfer => ConnectionProtocol::Login,
                };
                let sequence_result = self.pre_play_state.lock().select_protocol(intent);
                if let Err(error) = sequence_result {
                    log::warn!("Client {} {error}", self.id);
                    self.kick(TextComponent::translated(
                        translations::MULTIPLAYER_DISCONNECT_INVALID_PACKET.msg(),
                    ))
                    .await;
                    return Ok(());
                }
                if intent == ConnectionProtocol::Status {
                    self.protocol.store(intent);
                } else {
                    let reason = match packet.protocol_version.cmp(&CURRENT_MC_PROTOCOL) {
                        Ordering::Equal => {
                            let tick_manager = self.server.tick_rate_manager.read();
                            self.login_deadline
                                .store(Some(LoginDeadline::from_start_tick(
                                    tick_manager.tick_count,
                                )));
                            self.protocol.store(intent);
                            return Ok(());
                        }
                        Ordering::Less => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_OUTDATED_CLIENT
                                .message([MC_VERSION]),
                        ),
                        Ordering::Greater => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_INCOMPATIBLE.message([MC_VERSION]),
                        ),
                    };
                    self.protocol.store(intent);
                    self.kick(reason).await;
                    return Ok(());
                }
            }
            id => {
                log::error!("Received unexpected packet id: {id}");
                return Err(PacketError::InvalidProtocol(id.to_string()));
            }
        }
        Ok(())
    }

    /// Handles a status packet.
    pub async fn handle_status(&self, packet: RawPacket) -> Result<(), PacketError> {
        let data = &mut Cursor::new(packet.payload());

        match packet.id {
            status::S_STATUS_REQUEST => {
                self.handle_status_request().await;
            }
            status::S_PING_REQUEST => {
                self.handle_ping_request(SPingRequest::read_packet(data)?)
                    .await;
            }
            _ => return Err(PacketError::InvalidProtocol("Status".to_string())),
        }
        Ok(())
    }

    /// Handles a login packet.
    pub(crate) async fn handle_login(
        &self,
        packet: RawPacket,
    ) -> Result<ConnectionAction, PacketError> {
        let data = &mut Cursor::new(packet.payload());

        match packet.id {
            login_packets::S_HELLO => {
                if let Err(error) = self.expect_pre_play_packet(PrePlayPacket::Hello) {
                    return Ok(self.reject_unexpected_packet(error).await);
                }
                Ok(self.handle_hello(SHello::read_packet(data)?).await)
            }
            login_packets::S_KEY => {
                if let Err(error) = self.expect_pre_play_packet(PrePlayPacket::Key) {
                    return Ok(self.reject_unexpected_packet(error).await);
                }
                Ok(self.handle_key(SKey::read_packet(data)?).await)
            }
            login_packets::S_LOGIN_ACKNOWLEDGED => {
                if let Err(error) = self.expect_pre_play_packet(PrePlayPacket::LoginAcknowledged) {
                    return Ok(self.reject_unexpected_packet(error).await);
                }
                Ok(self.handle_login_acknowledged().await)
            }
            _ => Err(PacketError::InvalidProtocol("Login".to_string())),
        }
    }

    /// Handles a configuration packet.
    pub(crate) async fn handle_config(
        &self,
        packet: RawPacket,
    ) -> Result<ConnectionAction, PacketError> {
        let data = &mut Cursor::new(packet.payload());

        match packet.id {
            config::S_CUSTOM_PAYLOAD => {
                self.handle_config_custom_payload(SCustomPayload::read_packet(data)?);
                Ok(ConnectionAction::none())
            }
            config::S_CLIENT_INFORMATION => {
                self.handle_client_information(SClientInformation::read_packet(data)?)
                    .await;
                Ok(ConnectionAction::none())
            }
            config::S_SELECT_KNOWN_PACKS => {
                if let Err(error) = self.expect_pre_play_packet(PrePlayPacket::SelectKnownPacks) {
                    return Ok(self.reject_unexpected_packet(error).await);
                }
                self.handle_select_known_packs(SSelectKnownPacks::read_packet(data)?)
                    .await;
                Ok(ConnectionAction::none())
            }
            config::S_FINISH_CONFIGURATION => {
                if let Err(error) = self.expect_pre_play_packet(PrePlayPacket::FinishConfiguration)
                {
                    return Ok(self.reject_unexpected_packet(error).await);
                }
                Ok(self.finish_configuration().await)
            }
            _ => Err(PacketError::InvalidProtocol("Config".to_string())),
        }
    }

    fn expect_pre_play_packet(&self, packet: PrePlayPacket) -> Result<(), PacketSequenceError> {
        self.pre_play_state.lock().expect(packet)
    }

    pub(crate) async fn reject_unexpected_packet(
        &self,
        error: PacketSequenceError,
    ) -> ConnectionAction {
        log::warn!("Client {} {error}", self.id);
        self.kick(TextComponent::translated(
            translations::MULTIPLAYER_DISCONNECT_INVALID_PACKET.msg(),
        ))
        .await;
        ConnectionAction::none()
    }

    /// Kicks the client with a given reason.
    pub async fn kick(&self, reason: TextComponent) {
        log::info!("Kicking client {}: {:p}", self.id, reason);
        match self.protocol.load() {
            ConnectionProtocol::Login => {
                let packet = CLoginDisconnect::new(&reason, self);
                self.send_bare_packet_now(packet).await;
            }
            ConnectionProtocol::Play | ConnectionProtocol::Config => {
                let packet = CDisconnect::new(&reason, self);
                self.send_bare_packet_now(packet).await;
            }
            ConnectionProtocol::Handshake | ConnectionProtocol::Status => (),
        }
        log::debug!("Closing connection for {}", self.id);
        self.close();
    }
}

impl TextResolutor for JavaTcpClient {
    fn resolve_content(&self, _resolvable: &Resolvable) -> TextComponent {
        TextComponent::new()
    }

    fn resolve_custom(&self, _data: &CustomData) -> Option<TextComponent> {
        None
    }

    fn translate(&self, _key: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests;
