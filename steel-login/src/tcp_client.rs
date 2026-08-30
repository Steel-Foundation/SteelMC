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
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam::atomic::AtomicCell;
use steel_core::player::{
    ClientInformation, PlayerConnection,
    connection::{JavaNetworkWriter, OutboundPacket},
};
use steel_core::server::Server;
use steel_protocol::{
    packet_reader::TCPNetworkDecoder,
    packet_traits::{ClientPacket, CompressionInfo, EncodedPacket, ServerPacket},
    packet_writer::TCPNetworkEncoder,
    packets::{
        common::{
            CDisconnect, CKeepAlive, SClientInformation, SCustomPayload, SKeepAlive, SPingRequest,
        },
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
use tokio::time::{Interval, MissedTickBehavior, interval};
use tokio::{
    io::{BufReader, BufWriter},
    net::{TcpStream, tcp::OwnedReadHalf},
    select,
    sync::{
        Notify,
        broadcast::{self, Sender, error::RecvError},
        mpsc::{self, UnboundedReceiver, UnboundedSender, error::TryRecvError},
    },
    time::timeout,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use uuid::Uuid;

use crate::pre_play_state::{PacketSequenceError, PrePlayPacket, PrePlayState};

const MAX_TICKS_BEFORE_LOGIN: u64 = 600;
const DISCONNECT_FLUSH_TIMEOUT: Duration = Duration::from_secs(1);
/// Bound for the periodic keep-alive write itself: a healthy client accepts it in
/// microseconds, so a stall means the socket is wedged and the connection must close
/// instead of the read loop blocking on the write forever.
const KEEP_ALIVE_WRITE_TIMEOUT: Duration = Duration::from_secs(1);

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

enum IncomingEvent {
    Packet(Result<RawPacket, PacketError>),
    ConnectionUpdate(Result<ConnectionUpdate, RecvError>),
    KeepAliveTick,
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

impl Debug for ConnectionUpdate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnableEncryption(arg0) => f.debug_tuple("EnableEncryption").field(arg0).finish(),
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

/// What a configuration keep-alive tick should do.
enum KeepAliveDecision {
    /// Nothing is due yet.
    None,
    /// Send a challenge with this id.
    Send(i64),
    /// The pending challenge went unanswered for a full interval; kick.
    Timeout,
}

/// Configuration keep-alive state, mirroring vanilla's `ServerCommonPacketListenerImpl`
/// keep-alive timing and latency smoothing.
pub(crate) struct PrePlayKeepAliveTracker {
    /// Last time a challenge was sent (or the phase was entered).
    pub(crate) last_sent: Instant,
    /// The challenge the client has not answered yet.
    pub(crate) pending: Option<i64>,
    /// Smoothed round-trip latency in milliseconds, seeded into the play connection like
    /// vanilla's `CommonListenerCookie.latency()`.
    pub(crate) latency: u32,
}

impl PrePlayKeepAliveTracker {
    /// Vanilla sends a keep-alive every 15 seconds since the last send and kicks a
    /// client whose challenge went unanswered for a full interval.
    const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

    /// Decides this tick's action, mirroring vanilla `keepConnectionAlive`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "milliseconds since the Unix epoch fit i64 for the next 292 million years"
    )]
    fn tick(&mut self, now: Instant) -> KeepAliveDecision {
        if now.duration_since(self.last_sent) < Self::KEEP_ALIVE_INTERVAL {
            return KeepAliveDecision::None;
        }
        if self.pending.is_some() {
            return KeepAliveDecision::Timeout;
        }

        // Vanilla uses `Util.getMillis()` as the challenge id.
        let challenge = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before UNIX EPOCH")
            .as_millis() as i64;
        self.pending = Some(challenge);
        self.last_sent = now;
        KeepAliveDecision::Send(challenge)
    }

    /// Records a client keep-alive response. Returns `true` when it answers the pending
    /// challenge, smoothing the latency like vanilla `handleKeepAlive`; out-of-order
    /// answers return `false` and are a timeout kick.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "an unanswered challenge times out after one interval, so the round trip stays far below u32::MAX ms"
    )]
    fn answer(&mut self, id: i64, now: Instant) -> bool {
        match self.pending {
            Some(pending) if pending == id => {
                let round_trip = now.duration_since(self.last_sent).as_millis() as u32;
                self.latency = (self.latency * 3 + round_trip) / 4;
                self.pending = None;
                true
            }
            _ => false,
        }
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
    pub network_writer: JavaNetworkWriter,
    /// Current compression settings.
    pub compression: Arc<AtomicCell<Option<CompressionInfo>>>,

    /// The shared server state.
    pub server: Arc<Server>,
    /// The session id state for the active server connection listener
    pub connection_session: Arc<ServerConnectionSession>,
    /// The challenge sent to the client during login.
    pub challenge: AtomicCell<[u8; 4]>,
    /// Outstanding configuration keep-alive challenge awaiting the client response.
    pub(crate) config_keepalive: SyncMutex<PrePlayKeepAliveTracker>,

    /// Channel for broadcasting connection state updates.
    pub connection_updates: Sender<ConnectionUpdate>,
    /// Notification for when connection updates are processed.
    pub connection_updated: Arc<Notify>,

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
            config_keepalive: SyncMutex::new(PrePlayKeepAliveTracker {
                last_sent: Instant::now(),
                pending: None,
                latency: 0,
            }),
            connection_updates,
            connection_updated: Arc::new(Notify::new()),
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

    async fn write_network_packet(
        network_writer: &JavaNetworkWriter,
        packet: &EncodedPacket,
    ) -> Result<(), PacketError> {
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

    async fn release_network_writer(network_writer: &JavaNetworkWriter) {
        network_writer.lock().await.take();
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
    /// This task will run until the client is closed or the cancellation token is cancelled.
    pub fn start_outgoing_packet_task(
        self: &Arc<Self>,
        mut sender_recv: UnboundedReceiver<OutboundPacket>,
    ) {
        let cancel_token = self.cancel_token.clone();
        let network_writer = self.network_writer.clone();
        let id = self.id;
        let mut connection_updates_recv = self.connection_updates.subscribe();
        let connection_updated = self.connection_updated.clone();

        self.task_tracker.spawn(async move {
            let mut connection = None;
            loop {
                select! {
                    biased;
                    () = cancel_token.cancelled() => {
                        Self::write_queued_disconnect(&network_writer, &mut sender_recv, id).await;
                        break;
                    }
                    outbound = sender_recv.recv() => {
                        if let Some(outbound) = outbound {
                            let (packet, close_after_write) = match outbound {
                                OutboundPacket::Packet(packet) => (packet, false),
                                OutboundPacket::Disconnect(packet) => (packet, true),
                            };

                            if close_after_write {
                                if let Err(err) = Self::write_network_packet(&network_writer, &packet).await {
                                    log::warn!("Failed to send disconnect packet to client {id}: {err}");
                                }
                                cancel_token.cancel();
                                break;
                            }

                            let write_result = Self::write_network_packet(&network_writer, &packet);
                            select! {
                                biased;
                                () = cancel_token.cancelled() => {
                                    Self::write_queued_disconnect(&network_writer, &mut sender_recv, id).await;
                                    break;
                                },
                                result = write_result => {
                                    if let Err(err) = result {
                                        log::warn!("Failed to send packet to client {id}: {err}");
                                        cancel_token.cancel();
                                    }
                                }
                            }
                        } else {
                            cancel_token.cancel();
                        }
                    }
                    connection_update = connection_updates_recv.recv() => {
                        match connection_update {
                            Ok(connection_update) => {
                                match connection_update {
                                    ConnectionUpdate::EnableEncryption(key) => {
                                        let mut writer = network_writer.lock().await;
                                        let Some(writer) = writer.as_mut() else {
                                            cancel_token.cancel();
                                            continue;
                                        };
                                        writer.set_encryption(&key);
                                        connection_updated.notify_one();
                                    },
                                    ConnectionUpdate::Upgrade(upgrade) => {
                                        connection = Some(upgrade);
                                        connection_updated.notify_one();
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
            drop(connection_updated);

            if let Some(connection) = connection {
                drop(network_writer);
                match &*connection {
                    PlayerConnection::Java(java) => java.sender(sender_recv).await,
                    PlayerConnection::Other(_) => unreachable!("Expected Java connection"),
                }
            } else {
                Self::release_network_writer(&network_writer).await;
                drop(network_writer);
            }
        });
    }

    async fn write_queued_disconnect(
        network_writer: &JavaNetworkWriter,
        sender_recv: &mut UnboundedReceiver<OutboundPacket>,
        id: u64,
    ) {
        let mut disconnect_packet = None;
        loop {
            match sender_recv.try_recv() {
                Ok(OutboundPacket::Packet(_)) => {}
                Ok(OutboundPacket::Disconnect(packet)) => disconnect_packet = Some(packet),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        let Some(packet) = disconnect_packet else {
            return;
        };
        if let Err(err) = Self::write_network_packet(network_writer, &packet).await {
            log::warn!("Failed to send disconnect packet to client {id} during close: {err}");
        }
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
            // One-second ticks give the vanilla 15-second send and timeout boundaries
            // one-second resolution on a single shared timer.
            let mut config_keepalive_tick = interval(Duration::from_secs(1));
            config_keepalive_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            // Consume the immediate first tick so the cadence starts at one second.
            config_keepalive_tick.tick().await;
            loop {
                let incoming_event = self_clone
                    .await_incoming_event(&mut reader, &mut connection_updates_recv, &mut config_keepalive_tick)
                    .await;

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
                                let reason = TextComponent::translated(
                                    translations::MULTIPLAYER_DISCONNECT_INVALID_PACKET.msg(),
                                );
                                self_clone.kick(reason).await;
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
                    LoginOperationResult::Completed(IncomingEvent::KeepAliveTick) => {
                        if self_clone.protocol.load() == ConnectionProtocol::Config {
                            self_clone.tick_config_keepalive().await;
                        }
                    }
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

    /// Awaits the next pre-play event: an inbound packet, a connection update, or a
    /// configuration keep-alive tick, bounded by the login deadline when one is set.
    async fn await_incoming_event(
        &self,
        reader: &mut TCPNetworkDecoder<BufReader<OwnedReadHalf>>,
        connection_updates_recv: &mut broadcast::Receiver<ConnectionUpdate>,
        config_keepalive_tick: &mut Interval,
    ) -> LoginOperationResult<IncomingEvent> {
        if self.login_deadline_expired() {
            return LoginOperationResult::TimedOut;
        }
        let incoming_event = async {
            select! {
                packet = reader.get_raw_packet() => IncomingEvent::Packet(packet),
                connection_update = connection_updates_recv.recv() => {
                    IncomingEvent::ConnectionUpdate(connection_update)
                }
                _ = config_keepalive_tick.tick() => IncomingEvent::KeepAliveTick,
            }
        };
        await_login_operation(
            &self.cancel_token,
            &self.login_deadline,
            incoming_event,
            self.wait_for_login_deadline(),
        )
        .await
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
        if timeout(DISCONNECT_FLUSH_TIMEOUT, self.kick(reason))
            .await
            .is_err()
        {
            log::debug!(
                "Best-effort slow-login disconnect write for client {} timed out",
                self.id
            );
            self.close();
        }
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
            config::S_KEEP_ALIVE => {
                let packet = SKeepAlive::read_packet(data)?;
                if self
                    .config_keepalive
                    .lock()
                    .answer(packet.id, Instant::now())
                {
                    Ok(ConnectionAction::none())
                } else {
                    // Vanilla disconnects clients answering out of order with a timeout.
                    self.kick(TextComponent::translated(
                        translations::DISCONNECT_TIMEOUT.msg(),
                    ))
                    .await;
                    Ok(ConnectionAction::none())
                }
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

    /// Ticks the configuration keep-alive, executing the tracker's vanilla-timed
    /// decision. Both writes are bounded: a wedged socket must close the connection
    /// instead of stalling the read loop.
    async fn tick_config_keepalive(&self) {
        let decision = self.config_keepalive.lock().tick(Instant::now());
        match decision {
            KeepAliveDecision::None => {}
            KeepAliveDecision::Send(challenge) => {
                if timeout(
                    KEEP_ALIVE_WRITE_TIMEOUT,
                    self.send_bare_packet_now(CKeepAlive::new(challenge)),
                )
                .await
                .is_err()
                {
                    self.close();
                }
            }
            KeepAliveDecision::Timeout => {
                let reason = TextComponent::translated(translations::DISCONNECT_TIMEOUT.msg());
                if timeout(DISCONNECT_FLUSH_TIMEOUT, self.kick(reason))
                    .await
                    .is_err()
                {
                    self.close();
                }
            }
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
