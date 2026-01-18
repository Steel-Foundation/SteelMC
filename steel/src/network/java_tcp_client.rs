use std::{
    fmt::{self, Debug, Formatter},
    io::Cursor,
    net::SocketAddr,
    sync::Arc,
};

use crossbeam::atomic::AtomicCell;
use steel_core::player::{ClientInformation, GameProfile, networking::JavaConnection};
use steel_protocol::{
    packet_reader::TCPNetworkDecoder,
    packet_traits::{ClientPacket, CompressionInfo, EncodedPacket, ServerPacket},
    packet_writer::TCPNetworkEncoder,
    packets::{
        common::{CDisconnect, SClientInformation, SCustomPayload},
        config::SSelectKnownPacks,
        handshake::{ClientIntent, SClientIntention},
        login::{CLoginDisconnect, SHello, SKey},
        status::SPingRequest,
    },
    utils::{ConnectionProtocol, PacketError, RawPacket},
};
use steel_registry::packets::{config, handshake, login, status};
use steel_utils::locks::AsyncMutex;
use steel_utils::text::TextComponent;
use tokio::{
    io::{BufReader, BufWriter},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    select,
    sync::{
        Notify,
        broadcast::{self, Sender, error::RecvError},
        mpsc::{self, UnboundedReceiver, UnboundedSender},
    },
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use steel_core::server::Server;

/// Represents updates to the connection state.
#[derive(Clone)]
pub enum ConnectionUpdate {
    /// Enable encryption on the connection.
    EnableEncryption([u8; 16]),
    /// Enable compression on the connection.
    EnableCompression(CompressionInfo),
    /// Upgrade the connection to the play state.
    Upgrade(Arc<JavaConnection>),
}

impl Debug for ConnectionUpdate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnableEncryption(arg0) => f.debug_tuple("EnableEncryption").field(arg0).finish(),
            Self::EnableCompression(arg0) => {
                f.debug_tuple("EnableCompression").field(arg0).finish()
            }
            Self::Upgrade(_) => f.debug_tuple("Upgrade").finish(),
        }
    }
}

/// Connection for pre play packets
/// Gets dropped by `incoming_packet_task` if closed or upgradet to play connection
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
    /// A token to cancel the client's operations. Called when the connection is closed. Or client is removed.
    pub cancel_token: CancellationToken,

    /// A queue of encoded packets to send to the network
    pub outgoing_queue: UnboundedSender<EncodedPacket>,
    /// The packet encoder for outgoing packets.
    pub network_writer: Arc<AsyncMutex<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>,
    pub(crate) compression: Arc<AtomicCell<Option<CompressionInfo>>>,

    /// The shared server state.
    pub server: Arc<Server>,
    /// The challenge sent to the client during login.
    pub challenge: AtomicCell<[u8; 4]>,

    pub(crate) connection_updates: Sender<ConnectionUpdate>,
    pub(crate) connection_updated: Arc<Notify>,

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
        task_tracker: TaskTracker,
    ) -> (
        Self,
        UnboundedReceiver<EncodedPacket>,
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
            network_writer: Arc::new(AsyncMutex::new(TCPNetworkEncoder::new(BufWriter::new(
                write,
            )))),
            compression: Arc::new(AtomicCell::new(None)),
            server,
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

    /// Sends a packet immediately, without queueing.
    ///
    /// # Panics
    /// This function will panic if the packet cannot be encoded. Should never happen.
    pub async fn send_bare_packet_now<P: ClientPacket>(&self, packet: P) {
        let compression = self.compression.load();
        let protocol = self.protocol.load();
        let packet = EncodedPacket::from_bare(packet, compression, protocol)
            .expect("Failed to encode packet");

        if let Err(err) = self.network_writer.lock().await.write_packet(&packet).await
            && !self.cancel_token.is_cancelled()
        {
            log::warn!("Failed to send packet to client {}: {}", self.id, err);
            self.close();
        }
    }

    /// Sends an already encoded packet immediately, without queueing.
    pub async fn send_packet_now(&self, packet: &EncodedPacket) {
        if let Err(err) = self.network_writer.lock().await.write_packet(packet).await
            && !self.cancel_token.is_cancelled()
        {
            log::warn!("Failed to send packet to client {}: {}", self.id, err);
            self.close();
        }
    }

    /// Encodes and queues a packet to be sent.
    pub fn send_bare_packet<P: ClientPacket>(&self, packet: P) -> Result<(), PacketError> {
        let compression = self.compression.load();
        let protocol = self.protocol.load();
        let packet = EncodedPacket::from_bare(packet, compression, protocol)?;
        self.outgoing_queue.send(packet).map_err(|e| {
            PacketError::SendError(format!(
                "Failed to send packet to client {}: {}",
                self.id, e
            ))
        })?;
        Ok(())
    }

    /// Queues an already encoded packet to be sent.
    pub fn send_packet(&self, packet: EncodedPacket) -> Result<(), PacketError> {
        self.outgoing_queue.send(packet).map_err(|e| {
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
        mut sender_recv: UnboundedReceiver<EncodedPacket>,
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
                    () = cancel_token.cancelled() => {
                        break;
                    }
                    packet = sender_recv.recv() => {
                        if let Some(packet) = packet {
                            if let Err(err) = network_writer.lock().await.write_packet(&packet).await
                            {
                                log::warn!("Failed to send packet to client {id}: {err}");
                                cancel_token.cancel();
                            }
                        } else {
                            //log::warn!(
                            //    "Internal packet_sender_recv channel closed for client {id}",
                            //);
                            cancel_token.cancel();
                        }
                    }
                    connection_update = connection_updates_recv.recv() => {
                        match connection_update {
                            Ok(connection_update) => {
                                match connection_update {
                                    ConnectionUpdate::EnableEncryption(key) => {
                                        network_writer.lock().await.set_encryption(&key);
                                        connection_updated.notify_waiters();
                                    },
                                    ConnectionUpdate::Upgrade(upgrade) => {
                                        connection = Some(upgrade);
                                        break;
                                    }
                                    ConnectionUpdate::EnableCompression(_) => ()
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
            drop(network_writer);
            drop(connection_updates_recv);
            drop(connection_updated);

            if let Some(connection) = connection {
                connection.sender(sender_recv).await;
            }
        });
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
        let connection_updated = self.connection_updated.clone();

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
                                //log::info!("Received packet: {:?}, protocol: {:?}", packet.id, connection_protocol.load());
                                if let Err(err) = self_clone.process_packet(packet).await {
                                    log::warn!(
                                        "Failed to get packet from client {id}: {err}",
                                    );
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
                            Ok(ConnectionUpdate::EnableEncryption(key)) => {
                                reader.set_encryption(&key);
                            }
                            Ok(ConnectionUpdate::EnableCompression(compression)) => {
                                reader.set_compression(compression.threshold);
                                connection_updated.notify_waiters();
                            },
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
            drop(connection_updated);

            if let Some(connection) = connection {
                let server = self_clone.server.clone();
                drop(self_clone);

                connection.listener(reader, server).await;
            }
        });
    }

    async fn process_packet(&self, packet: RawPacket) -> Result<(), PacketError> {
        match self.protocol.load() {
            ConnectionProtocol::Handshake => self.handle_handshake(packet),
            ConnectionProtocol::Status => self.handle_status(packet).await,
            ConnectionProtocol::Login => self.handle_login(packet).await,
            ConnectionProtocol::Config => self.handle_config(packet).await,
            ConnectionProtocol::Play => Err(PacketError::InvalidProtocol("Play".to_string())),
        }
    }

    /// Handles a handshake packet.
    pub fn handle_handshake(&self, packet: RawPacket) -> Result<(), PacketError> {
        let data = &mut Cursor::new(packet.payload);

        match packet.id {
            handshake::S_INTENTION => {
                let intent = match SClientIntention::read_packet(data)?.intention {
                    ClientIntent::STATUS => ConnectionProtocol::Status,
                    ClientIntent::LOGIN | ClientIntent::TRANSFER => ConnectionProtocol::Login,
                };
                self.protocol.store(intent);

                if intent != ConnectionProtocol::Status {
                    //TODO: Handle client version being too low or high
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
        let data = &mut Cursor::new(packet.payload);

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
    pub async fn handle_login(&self, packet: RawPacket) -> Result<(), PacketError> {
        let data = &mut Cursor::new(packet.payload);

        match packet.id {
            login::S_HELLO => self.handle_hello(SHello::read_packet(data)?).await,
            login::S_KEY => self.handle_key(SKey::read_packet(data)?).await,
            login::S_LOGIN_ACKNOWLEDGED => {
                self.handle_login_acknowledged().await;
            }
            _ => return Err(PacketError::InvalidProtocol("Login".to_string())),
        }
        Ok(())
    }

    /// Handles a configuration packet.
    pub async fn handle_config(&self, packet: RawPacket) -> Result<(), PacketError> {
        let data = &mut Cursor::new(packet.payload);

        match packet.id {
            config::S_CUSTOM_PAYLOAD => {
                self.handle_config_custom_payload(SCustomPayload::read_packet(data)?);
            }
            config::S_CLIENT_INFORMATION => {
                self.handle_client_information(SClientInformation::read_packet(data)?)
                    .await;
            }
            config::S_SELECT_KNOWN_PACKS => {
                self.handle_select_known_packs(SSelectKnownPacks::read_packet(data)?)
                    .await;
            }
            config::S_FINISH_CONFIGURATION => {
                self.finish_configuration().await;
            }
            _ => return Err(PacketError::InvalidProtocol("Config".to_string())),
        }
        Ok(())
    }
}

impl JavaTcpClient {
    /// Kicks the client with a given reason.
    pub async fn kick(&self, reason: TextComponent) {
        log::info!("Kicking client {}: {:?}", self.id, reason);
        match self.protocol.load() {
            ConnectionProtocol::Login => {
                let packet = CLoginDisconnect::new(reason);
                self.send_bare_packet_now(packet).await;
            }
            ConnectionProtocol::Play | ConnectionProtocol::Config => {
                let packet = CDisconnect::new(reason);
                self.send_bare_packet_now(packet).await;
            }
            _ => {}
        }
        log::debug!("Closing connection for {}", self.id);
        self.close();
    }
}
