//! Pre-play TCP client connection handler.
//!
//! Handles the connection lifecycle from handshake through login and configuration,
//! until the connection is upgraded to play state.

use std::{
    cmp::Ordering,
    fmt::{self, Debug, Formatter},
    io::Cursor,
    net::SocketAddr,
    sync::Arc,
};

use crossbeam::atomic::AtomicCell;
use steel_core::player::{
    ClientInformation, GameProfile, PlayerConnection,
    networking::{JavaNetworkWriter, OutboundPacket},
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
    io::{BufReader, BufWriter},
    net::{TcpStream, tcp::OwnedReadHalf},
    select,
    sync::{
        Notify,
        broadcast::{self, Sender, error::RecvError},
        mpsc::{self, UnboundedReceiver, UnboundedSender, error::TryRecvError},
    },
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use uuid::Uuid;

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

/// Connection for pre-play packets.
///
/// Gets dropped by `incoming_packet_task` if closed or upgraded to play connection.
pub struct JavaTcpClient {
    /// The unique ID of the client.
    pub id: u64,
    /// The client's game profile information.
    pub gameprofile: AsyncMutex<Option<GameProfile>>,
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

    /// Channel for broadcasting connection state updates.
    pub connection_updates: Sender<ConnectionUpdate>,
    /// Notification for when connection updates are processed.
    pub connection_updated: Arc<Notify>,

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
            gameprofile: AsyncMutex::new(None),
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
            connection_updated: Arc::new(Notify::new()),
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
        let Some(network_writer) = network_writer.as_mut() else {
            return Err(PacketError::ConnectionClosed);
        };
        network_writer.write_packet(packet).await
    }

    async fn release_network_writer(network_writer: &JavaNetworkWriter) {
        network_writer.lock().await.take();
    }

    /// Encodes and queues a packet to be sent.
    pub fn send_bare_packet<P: ClientPacket>(&self, packet: P) -> Result<(), PacketError> {
        let compression = self.compression.load();
        let protocol = self.protocol.load();
        let packet = EncodedPacket::from_bare(packet, compression, protocol)?;
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
            loop {
                select! {
                    () = cancel_token.cancelled() => {
                        break;
                    }
                    packet = reader.get_raw_packet() => {
                        match packet {
                            Ok(packet) => {
                                match self_clone.process_packet(packet).await {
                                    Ok(action) => {
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
                                    Err(err) => {
                                        log::warn!(
                                            "Failed to get packet from client {id}: {err}",
                                        );
                                    }
                                }
                            }
                            Err(err) => {
                                log::info!("Failed to get raw packet from client {id}: {err}");
                                cancel_token.cancel();
                            }
                        }
                    }
                    connection_update = connection_updates_recv.recv() => {
                        match connection_update {
                            Ok(ConnectionUpdate::EnableEncryption(_)) => {}
                            Ok(ConnectionUpdate::Upgrade(upgrade)) => {
                                connection = Some(upgrade);
                                break;
                            }
                            Err(err) => {
                                if err != RecvError::Closed {
                                    log::info!("Internal connection_updates_recv channel closed for client {id}: {err}");
                                }
                                cancel_token.cancel();
                            }
                        }
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
        let data = &mut Cursor::new(packet.payload.as_slice());

        match packet.id {
            handshake::S_INTENTION => {
                let packet = SClientIntention::read_packet(data)?;
                let intent = match packet.intention {
                    ClientIntent::Status => ConnectionProtocol::Status,
                    ClientIntent::Login | ClientIntent::Transfer => ConnectionProtocol::Login,
                };
                self.protocol.store(intent);

                if intent != ConnectionProtocol::Status {
                    let reason = match packet.protocol_version.cmp(&CURRENT_MC_PROTOCOL) {
                        Ordering::Equal => return Ok(()),
                        Ordering::Less => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_OUTDATED_CLIENT
                                .message([MC_VERSION]),
                        ),
                        Ordering::Greater => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_INCOMPATIBLE.message([MC_VERSION]),
                        ),
                    };
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
        let data = &mut Cursor::new(packet.payload.as_slice());

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
        let data = &mut Cursor::new(packet.payload.as_slice());

        match packet.id {
            login_packets::S_HELLO => Ok(self.handle_hello(SHello::read_packet(data)?).await),
            login_packets::S_KEY => Ok(self.handle_key(SKey::read_packet(data)?).await),
            login_packets::S_LOGIN_ACKNOWLEDGED => {
                self.handle_login_acknowledged().await;
                Ok(ConnectionAction::none())
            }
            _ => Err(PacketError::InvalidProtocol("Login".to_string())),
        }
    }

    /// Handles a configuration packet.
    pub(crate) async fn handle_config(
        &self,
        packet: RawPacket,
    ) -> Result<ConnectionAction, PacketError> {
        let data = &mut Cursor::new(packet.payload.as_slice());

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
                self.handle_select_known_packs(SSelectKnownPacks::read_packet(data)?)
                    .await;
                Ok(ConnectionAction::none())
            }
            config::S_FINISH_CONFIGURATION => Ok(self.finish_configuration().await),
            _ => Err(PacketError::InvalidProtocol("Config".to_string())),
        }
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
