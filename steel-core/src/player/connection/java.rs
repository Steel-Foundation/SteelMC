//! This module contains the `JavaConnection` struct, which is used to represent a connection to a Java client.
use std::io::Cursor;
use std::sync::{Arc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use steel_protocol::packet_reader::TCPNetworkDecoder;
use steel_protocol::packet_traits::{ClientPacket, CompressionInfo, EncodedPacket, ServerPacket};
use steel_protocol::packet_writer::TCPNetworkEncoder;
use steel_protocol::packets::common::{
    CDisconnect, CKeepAlive, CPongResponse, SClientInformation, SCustomPayload, SKeepAlive,
    SPingRequest,
};
use steel_protocol::packets::game::{
    CBundleDelimiter, CCommandSuggestions, PlayerAction, PlayerCommandAction, SAcceptTeleportation,
    SAttack, SChangeDifficulty, SChangeGameMode, SChat, SChatAck, SChatCommand, SChatSessionUpdate,
    SChunkBatchReceived, SClientCommand, SClientTickEnd, SCommandSuggestion, SContainerButtonClick,
    SContainerClick, SContainerClose, SContainerSlotStateChanged, SInteract, SMovePlayer,
    SMovePlayerPos, SMovePlayerPosRot, SMovePlayerRot, SMovePlayerStatusOnly, SMoveVehicle,
    SPickItemFromBlock, SPlayerAbilities, SPlayerAction, SPlayerCommand, SPlayerInput, SPlayerLoad,
    SSetCarriedItem, SSetCreativeModeSlot, SSignUpdate, SSpectatorAction, SSwing, SUseItem,
    SUseItemOn,
};

use steel_protocol::utils::{ConnectionProtocol, PacketError, RawPacket};
use steel_registry::packets::play;
use steel_utils::locks::{AsyncMutex, SyncMutex};
use steel_utils::translations;
use text_components::content::Resolvable;
use text_components::custom::CustomData;
use text_components::resolving::TextResolutor;
use text_components::{Modifier, TextComponent, format::Color};
use tokio::io::{BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio_util::sync::CancellationToken;

use crate::command::{handle_client_request, sender::CommandSender};
use crate::player::Player;
use crate::player::connection::NetworkConnection;
use crate::server::Server;

/// Shared Java socket writer.
pub type JavaNetworkWriter = Arc<AsyncMutex<Option<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>>;

/// Outbound packet queue message for Java connections.
pub enum OutboundPacket {
    /// Normal packet write that may be interrupted by connection shutdown.
    Packet(EncodedPacket),
    /// Final disconnect packet that must be flushed before closing the socket.
    Disconnect(EncodedPacket),
}

/// A decoded play packet whose handler runs in the server's inter-tick packet phase.
pub(crate) struct ScheduledPlayPacket(ScheduledPlayPacketKind);

/// Cross-player concurrency permitted for a scheduled packet handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScheduledPacketExecution {
    /// The handler may overlap handlers for other players, but never its own player lane.
    ///
    /// Shared mutations must be fully linearized by their resource locks, and the handler must
    /// tolerate cross-player execution order differing from packet submission order.
    PlayerLocal,
    /// The handler may overlap player-local work, but not another serialized handler. Serialized
    /// handlers start in global packet submission order.
    Serialized,
    /// The handler is a global submission-order barrier and must not overlap scheduled work.
    Exclusive,
}

enum ScheduledPlayPacketKind {
    AcceptTeleportation(SAcceptTeleportation),
    Attack(SAttack),
    Interact(SInteract),
    CustomPayload(SCustomPayload),
    Chat(Box<SChat>),
    ChatAck(SChatAck),
    ChatSessionUpdate(SChatSessionUpdate),
    ClientInformation(SClientInformation),
    ClientTickEnd,
    MovePlayer(SMovePlayer),
    MoveVehicle(SMoveVehicle),
    PlayerLoaded,
    ChatCommand(SChatCommand),
    CommandSuggestion(SCommandSuggestion),
    ContainerButtonClick(SContainerButtonClick),
    ContainerClick(SContainerClick),
    ContainerClose(SContainerClose),
    ContainerSlotStateChanged(SContainerSlotStateChanged),
    SetCreativeModeSlot(SSetCreativeModeSlot),
    PlayerInput(SPlayerInput),
    PlayerCommand(SPlayerCommand),
    PlayerAbilities(SPlayerAbilities),
    UseItemOn(SUseItemOn),
    UseItem(SUseItem),
    SetCarriedItem(SSetCarriedItem),
    Swing(SSwing),
    PlayerAction(SPlayerAction),
    PickItemFromBlock(SPickItemFromBlock),
    SignUpdate(SSignUpdate),
    SpectatorAction(SSpectatorAction),
    ClientCommand(SClientCommand),
    ChangeGameMode(SChangeGameMode),
    ChangeDifficulty(SChangeDifficulty),
}

enum ImmediatePlayPacket {
    KeepAlive(SKeepAlive),
    PingRequest(SPingRequest),
    ChunkBatchReceived(SChunkBatchReceived),
    Unknown(i32),
}

enum DecodedPlayPacket {
    Scheduled(ScheduledPlayPacket),
    Immediate(ImmediatePlayPacket),
}

impl ScheduledPlayPacket {
    /// Returns the handler's audited cross-player concurrency class.
    ///
    /// This match is intentionally exhaustive so every newly implemented packet requires an
    /// explicit concurrency decision.
    pub(crate) const fn execution(&self) -> ScheduledPacketExecution {
        match &self.0 {
            // These handlers touch player-owned state or use an individually linearizable shared
            // operation. The player lane preserves same-player order.
            ScheduledPlayPacketKind::ChatSessionUpdate(_)
            | ScheduledPlayPacketKind::ClientInformation(_)
            | ScheduledPlayPacketKind::ClientTickEnd
            | ScheduledPlayPacketKind::PlayerLoaded
            | ScheduledPlayPacketKind::ChatCommand(_)
            | ScheduledPlayPacketKind::CommandSuggestion(_)
            | ScheduledPlayPacketKind::ContainerClose(_)
            | ScheduledPlayPacketKind::SetCreativeModeSlot(_)
            | ScheduledPlayPacketKind::PlayerInput(_)
            | ScheduledPlayPacketKind::PlayerAbilities(_)
            | ScheduledPlayPacketKind::SetCarriedItem(_)
            | ScheduledPlayPacketKind::Swing(_)
            | ScheduledPlayPacketKind::PickItemFromBlock(_)
            | ScheduledPlayPacketKind::ClientCommand(_) => ScheduledPacketExecution::PlayerLocal,
            ScheduledPlayPacketKind::PlayerCommand(packet) => match packet.action {
                PlayerCommandAction::StartSprinting
                | PlayerCommandAction::StopSprinting
                | PlayerCommandAction::StartFallFlying => ScheduledPacketExecution::PlayerLocal,
                PlayerCommandAction::LeaveBed => ScheduledPacketExecution::Serialized,
                // These handlers are not implemented, so their eventual vehicle transaction
                // cannot yet be audited against concurrently player-local work.
                PlayerCommandAction::StartRidingJump
                | PlayerCommandAction::StopRidingJump
                | PlayerCommandAction::OpenVehicleInventory => ScheduledPacketExecution::Exclusive,
            },
            ScheduledPlayPacketKind::PlayerAction(packet) => match packet.action {
                PlayerAction::AbortDestroyBlock | PlayerAction::SwapItemWithOffhand => {
                    ScheduledPacketExecution::PlayerLocal
                }
                PlayerAction::StartDestroyBlock
                | PlayerAction::StopDestroyBlock
                | PlayerAction::DropAllItems
                | PlayerAction::DropItem => ScheduledPacketExecution::Serialized,
                // Release-use is not implemented, while stab spans independently locked targets;
                // neither can yet overlap player-local work safely.
                PlayerAction::ReleaseUseItem | PlayerAction::Stab => {
                    ScheduledPacketExecution::Exclusive
                }
            },
            // Position, world, menu, chat, and domain mutations may overlap player-local work but
            // retain one global mutation order matching the packet submission order.
            ScheduledPlayPacketKind::AcceptTeleportation(_)
            | ScheduledPlayPacketKind::Chat(_)
            | ScheduledPlayPacketKind::ChatAck(_)
            | ScheduledPlayPacketKind::MovePlayer(_)
            | ScheduledPlayPacketKind::MoveVehicle(_)
            | ScheduledPlayPacketKind::ContainerClick(_)
            | ScheduledPlayPacketKind::UseItemOn(_)
            | ScheduledPlayPacketKind::UseItem(_)
            | ScheduledPlayPacketKind::SignUpdate(_)
            | ScheduledPlayPacketKind::SpectatorAction(_)
            | ScheduledPlayPacketKind::ChangeGameMode(_)
            | ScheduledPlayPacketKind::ChangeDifficulty(_) => ScheduledPacketExecution::Serialized,
            // Combat spans source and target state, custom payloads have no constrained resource
            // contract, and the unimplemented menu handlers have no auditable transaction yet.
            ScheduledPlayPacketKind::Attack(_)
            | ScheduledPlayPacketKind::Interact(_)
            | ScheduledPlayPacketKind::CustomPayload(_)
            | ScheduledPlayPacketKind::ContainerButtonClick(_)
            | ScheduledPlayPacketKind::ContainerSlotStateChanged(_) => {
                ScheduledPacketExecution::Exclusive
            }
        }
    }

    pub(crate) const fn can_process_before_join(&self) -> bool {
        matches!(
            &self.0,
            ScheduledPlayPacketKind::AcceptTeleportation(_)
                | ScheduledPlayPacketKind::ClientInformation(_)
                | ScheduledPlayPacketKind::ClientTickEnd
                | ScheduledPlayPacketKind::CustomPayload(_)
                | ScheduledPlayPacketKind::ChatAck(_)
                | ScheduledPlayPacketKind::ChatSessionUpdate(_)
                | ScheduledPlayPacketKind::PlayerLoaded
        )
    }

    pub(crate) fn handle(self, player: Arc<Player>, server: &Arc<Server>) {
        if !player.has_joined_world() && !self.can_process_before_join() {
            return;
        }

        match self.0 {
            ScheduledPlayPacketKind::AcceptTeleportation(packet) => {
                player.handle_accept_teleportation(packet);
            }
            ScheduledPlayPacketKind::Attack(packet) => player.handle_attack(packet),
            ScheduledPlayPacketKind::Interact(packet) => player.handle_interact(packet),
            ScheduledPlayPacketKind::CustomPayload(packet) => {
                player.handle_custom_payload(packet);
            }
            ScheduledPlayPacketKind::Chat(packet) => {
                player.handle_chat(*packet, Arc::clone(&player));
            }
            ScheduledPlayPacketKind::ChatAck(packet) => player.handle_chat_ack(packet),
            ScheduledPlayPacketKind::ChatSessionUpdate(packet) => {
                player.handle_chat_session_update(packet);
            }
            ScheduledPlayPacketKind::ClientInformation(packet) => {
                player.handle_client_information(packet);
            }
            ScheduledPlayPacketKind::ClientTickEnd => player.handle_client_tick_end(),
            ScheduledPlayPacketKind::MovePlayer(packet) => player.handle_move_player(packet),
            ScheduledPlayPacketKind::MoveVehicle(packet) => player.handle_move_vehicle(packet),
            ScheduledPlayPacketKind::PlayerLoaded => {
                if player.mark_client_loaded_from_network() {
                    player.send_inventory_to_remote();
                }
            }
            ScheduledPlayPacketKind::ChatCommand(packet) => {
                if server
                    .submit_command(CommandSender::Player(Arc::clone(&player)), packet.command)
                    .is_err()
                {
                    player.send_message(
                        &TextComponent::const_plain("Command queue is full").color(Color::Red),
                    );
                }
                player.detect_command_rate_spam();
            }
            ScheduledPlayPacketKind::CommandSuggestion(packet) => {
                if server
                    .submit_command_suggestions(Arc::clone(&player), packet.id, packet.command)
                    .is_err()
                {
                    player.send_packet(CCommandSuggestions::new(packet.id, 0, 0, Vec::new()));
                }
            }
            ScheduledPlayPacketKind::ContainerButtonClick(packet) => {
                player.handle_container_button_click(packet);
            }
            ScheduledPlayPacketKind::ContainerClick(packet) => {
                player.handle_container_click(packet);
            }
            ScheduledPlayPacketKind::ContainerClose(packet) => {
                player.handle_container_close(packet);
            }
            ScheduledPlayPacketKind::ContainerSlotStateChanged(packet) => {
                player.handle_container_slot_state_changed(packet);
            }
            ScheduledPlayPacketKind::SetCreativeModeSlot(packet) => {
                player.handle_set_creative_mode_slot(packet);
            }
            ScheduledPlayPacketKind::PlayerInput(packet) => player.handle_player_input(packet),
            ScheduledPlayPacketKind::PlayerCommand(packet) => {
                player.handle_player_command(packet);
            }
            ScheduledPlayPacketKind::PlayerAbilities(packet) => {
                player.handle_player_abilities(packet);
            }
            ScheduledPlayPacketKind::UseItemOn(packet) => player.handle_use_item_on(packet),
            ScheduledPlayPacketKind::UseItem(packet) => player.handle_use_item(packet),
            ScheduledPlayPacketKind::SetCarriedItem(packet) => {
                player.handle_set_carried_item(packet);
            }
            ScheduledPlayPacketKind::Swing(packet) => player.swing(packet.hand, false),
            ScheduledPlayPacketKind::PlayerAction(packet) => {
                player.handle_player_action(packet);
            }
            ScheduledPlayPacketKind::PickItemFromBlock(packet) => {
                player.handle_pick_item_from_block(packet);
            }
            ScheduledPlayPacketKind::SignUpdate(packet) => player.handle_sign_update(packet),
            ScheduledPlayPacketKind::SpectatorAction(packet) => {
                player.handle_spectator_action(packet);
            }
            ScheduledPlayPacketKind::ClientCommand(packet) => {
                player.handle_client_command(packet.action);
            }
            ScheduledPlayPacketKind::ChangeGameMode(packet) => {
                handle_client_request(&player, server, packet.gamemode);
            }
            ScheduledPlayPacketKind::ChangeDifficulty(packet) => {
                player.handle_change_difficulty(packet.difficulty);
            }
        }
    }
}

/// Builder for creating packet bundles.
///
/// Used with [`JavaConnection::send_bundle`] to send multiple packets atomically.
pub struct BundleBuilder {
    packets: Vec<EncodedPacket>,
    compression: Option<CompressionInfo>,
}

impl BundleBuilder {
    /// Creates a new `BundleBuilder` with the given compression settings.
    #[must_use]
    pub const fn new(compression: Option<CompressionInfo>) -> Self {
        Self {
            packets: Vec::new(),
            compression,
        }
    }

    /// Adds a packet to the bundle.
    ///
    /// # Panics
    /// Panics if the packet fails to encode.
    pub fn add<P: ClientPacket>(&mut self, packet: P) {
        let encoded = EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            .expect("Failed to encode packet");
        self.packets.push(encoded);
    }

    /// Consumes the builder and returns the collected encoded packets.
    #[must_use]
    pub fn into_packets(self) -> Vec<EncodedPacket> {
        self.packets
    }
}

#[expect(
    clippy::struct_field_names,
    reason = "alive_ prefix is intentional to group related keep-alive fields"
)]
struct KeepAliveTracker {
    alive_time: u64,
    alive_pending: bool,
    alive_id: u64,
}

/// A connection to a Java client.
pub struct JavaConnection {
    outgoing_packets: UnboundedSender<OutboundPacket>,
    cancel_token: CancellationToken,
    compression: Option<CompressionInfo>,
    network_writer: JavaNetworkWriter,
    id: u64,

    player: Weak<Player>,
    keep_alive_tracker: SyncMutex<KeepAliveTracker>,
    latency: SyncMutex<u32>,
}

impl JavaConnection {
    /// Creates a new `JavaConnection`.
    pub const fn new(
        outgoing_packets: UnboundedSender<OutboundPacket>,
        cancel_token: CancellationToken,
        compression: Option<CompressionInfo>,
        network_writer: JavaNetworkWriter,
        id: u64,
        player: Weak<Player>,
    ) -> Self {
        Self {
            outgoing_packets,
            cancel_token,
            compression,
            network_writer,
            id,
            player,
            keep_alive_tracker: SyncMutex::new(KeepAliveTracker {
                alive_time: 0,
                alive_pending: false,
                alive_id: 0,
            }),
            latency: SyncMutex::new(0),
        }
    }

    async fn write_packet_now(&self, packet: &EncodedPacket) -> Result<(), PacketError> {
        let mut network_writer = self.network_writer.lock().await;
        let Some(network_writer) = network_writer.as_mut() else {
            return Err(PacketError::ConnectionClosed);
        };
        network_writer.write_packet(packet).await
    }

    async fn release_network_writer(&self) {
        self.network_writer.lock().await.take();
    }

    /// Ticks the connection.
    pub fn tick(&self) {
        self.keep_connection_alive();
    }

    fn keep_connection_alive(&self) {
        let mut tracker = self.keep_alive_tracker.lock();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before UNIX EPOCH")
            .as_millis() as u64;

        if now - tracker.alive_time >= 15000 {
            if tracker.alive_pending {
                self.disconnect(translations::DISCONNECT_TIMEOUT.msg());
            } else {
                tracker.alive_pending = true;
                tracker.alive_id = now;
                tracker.alive_time = now;
                self.send_packet(CKeepAlive::new(tracker.alive_id as i64));
            }
        }
    }

    /// Handles a keep alive packet.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "latency saturates at u32::MAX ms (~49 days), which is unreachable in practice"
    )]
    fn handle_keep_alive(&self, packet: SKeepAlive) {
        let mut tracker = self.keep_alive_tracker.lock();
        if tracker.alive_pending && packet.id as u64 == tracker.alive_id {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("System time before UNIX EPOCH")
                .as_millis() as u64;

            let time = now.saturating_sub(tracker.alive_time) as u32;
            tracker.alive_pending = false;
            drop(tracker);
            let mut latency = self.latency.lock();
            *latency = (*latency * 3 + time) / 4;
        } else {
            self.disconnect(translations::DISCONNECT_TIMEOUT.msg());
        }
    }

    /// Returns the current latency in milliseconds.
    /// This is a smoothed average calculated from keep-alive round-trip times.
    #[must_use]
    pub fn latency(&self) -> i32 {
        *self.latency.lock() as i32
    }

    /// Disconnects the client.
    pub fn disconnect(&self, reason: impl Into<TextComponent>) {
        let packet = match EncodedPacket::from_bare(
            CDisconnect::new(&reason.into(), self),
            self.compression,
            ConnectionProtocol::Play,
        ) {
            Ok(packet) => packet,
            Err(err) => {
                log::warn!(
                    "Failed to encode disconnect packet for client {}: {err}",
                    self.id
                );
                self.close();
                return;
            }
        };
        if self
            .outgoing_packets
            .send(OutboundPacket::Disconnect(packet))
            .is_err()
        {
            self.close();
            return;
        }
        self.close();
    }

    /// Sends a packet to the client.
    ///
    /// # Panics
    /// - If the packet fails to be encoded.
    /// - If the packet fails to be sent through the channel.
    pub fn send_packet<P: ClientPacket>(&self, packet: P) {
        let packet = EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            .expect("Failed to encode packet");
        if self
            .outgoing_packets
            .send(OutboundPacket::Packet(packet))
            .is_err()
        {
            self.close();
        }
    }

    /// Sends an encoded packet to the client.
    ///
    /// # Panics
    /// - If the packet fails to be sent through the channel.
    pub fn send_encoded_packet(&self, packet: EncodedPacket) {
        if self
            .outgoing_packets
            .send(OutboundPacket::Packet(packet))
            .is_err()
        {
            self.close();
        }
    }

    /// Closes the connection.
    pub fn close(&self) {
        self.cancel_token.cancel();
    }

    /// Returns whether the connection is closed.
    #[must_use]
    pub fn closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Waits for the connection to be closed.
    pub async fn wait_for_close(&self) {
        self.cancel_token.cancelled().await;
    }

    const fn can_process_before_join(packet_id: i32) -> bool {
        matches!(
            packet_id,
            play::S_ACCEPT_TELEPORTATION
                | play::S_KEEP_ALIVE
                | play::S_PING_REQUEST
                | play::S_CLIENT_INFORMATION
                | play::S_CUSTOM_PAYLOAD
                | play::S_CHUNK_BATCH_RECEIVED
                | play::S_CHAT_SESSION_UPDATE
                | play::S_CHAT_ACK
                | play::S_CLIENT_TICK_END
                | play::S_PLAYER_LOADED
        )
    }

    /// Decodes and dispatches one packet received from the client.
    fn process_packet(
        &self,
        packet: RawPacket,
        player: Arc<Player>,
        server: &Server,
    ) -> Result<(), PacketError> {
        if !player.has_joined_world() && !Self::can_process_before_join(packet.id) {
            return Ok(());
        }

        if player.is_domain_switching()
            && !matches!(packet.id, play::S_KEEP_ALIVE | play::S_PING_REQUEST)
        {
            return Ok(());
        }

        let payload_bytes = packet.payload.len();
        match Self::decode_play_packet(packet)? {
            DecodedPlayPacket::Scheduled(packet) => {
                server.schedule_play_packet(player, packet, payload_bytes);
            }
            DecodedPlayPacket::Immediate(packet) => {
                self.handle_immediate_packet(packet, &player);
            }
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "single match decode over all implemented play packets keeps protocol routing auditable"
    )]
    fn decode_play_packet(packet: RawPacket) -> Result<DecodedPlayPacket, PacketError> {
        let data = &mut Cursor::new(packet.payload.as_slice());
        let scheduled = |packet| DecodedPlayPacket::Scheduled(ScheduledPlayPacket(packet));

        Ok(match packet.id {
            play::S_ACCEPT_TELEPORTATION => {
                scheduled(ScheduledPlayPacketKind::AcceptTeleportation(
                    SAcceptTeleportation::read_packet(data)?,
                ))
            }
            play::S_ATTACK => {
                scheduled(ScheduledPlayPacketKind::Attack(SAttack::read_packet(data)?))
            }
            play::S_INTERACT => scheduled(ScheduledPlayPacketKind::Interact(
                SInteract::read_packet(data)?,
            )),
            play::S_CUSTOM_PAYLOAD => scheduled(ScheduledPlayPacketKind::CustomPayload(
                SCustomPayload::read_packet(data)?,
            )),
            play::S_CHAT => scheduled(ScheduledPlayPacketKind::Chat(Box::new(SChat::read_packet(
                data,
            )?))),
            play::S_CHAT_SESSION_UPDATE => scheduled(ScheduledPlayPacketKind::ChatSessionUpdate(
                SChatSessionUpdate::read_packet(data)?,
            )),
            play::S_CHAT_ACK => scheduled(ScheduledPlayPacketKind::ChatAck(SChatAck::read_packet(
                data,
            )?)),
            play::S_CLIENT_INFORMATION => scheduled(ScheduledPlayPacketKind::ClientInformation(
                SClientInformation::read_packet(data)?,
            )),
            play::S_CLIENT_TICK_END => {
                let _ = SClientTickEnd::read_packet(data)?;
                scheduled(ScheduledPlayPacketKind::ClientTickEnd)
            }
            play::S_CHUNK_BATCH_RECEIVED => DecodedPlayPacket::Immediate(
                ImmediatePlayPacket::ChunkBatchReceived(SChunkBatchReceived::read_packet(data)?),
            ),
            play::S_KEEP_ALIVE => DecodedPlayPacket::Immediate(ImmediatePlayPacket::KeepAlive(
                SKeepAlive::read_packet(data)?,
            )),
            play::S_MOVE_PLAYER_POS => scheduled(ScheduledPlayPacketKind::MovePlayer(
                SMovePlayerPos::read_packet(data)?.into(),
            )),
            play::S_MOVE_PLAYER_POS_ROT => scheduled(ScheduledPlayPacketKind::MovePlayer(
                SMovePlayerPosRot::read_packet(data)?.into(),
            )),
            play::S_MOVE_PLAYER_ROT => scheduled(ScheduledPlayPacketKind::MovePlayer(
                SMovePlayerRot::read_packet(data)?.into(),
            )),
            play::S_MOVE_PLAYER_STATUS_ONLY => scheduled(ScheduledPlayPacketKind::MovePlayer(
                SMovePlayerStatusOnly::read_packet(data)?.into(),
            )),
            play::S_MOVE_VEHICLE => scheduled(ScheduledPlayPacketKind::MoveVehicle(
                SMoveVehicle::read_packet(data)?,
            )),
            play::S_PLAYER_LOADED => {
                let _ = SPlayerLoad::read_packet(data)?;
                scheduled(ScheduledPlayPacketKind::PlayerLoaded)
            }
            play::S_CHAT_COMMAND => scheduled(ScheduledPlayPacketKind::ChatCommand(
                SChatCommand::read_packet(data)?,
            )),
            play::S_COMMAND_SUGGESTION => scheduled(ScheduledPlayPacketKind::CommandSuggestion(
                SCommandSuggestion::read_packet(data)?,
            )),
            play::S_CONTAINER_BUTTON_CLICK => {
                scheduled(ScheduledPlayPacketKind::ContainerButtonClick(
                    SContainerButtonClick::read_packet(data)?,
                ))
            }
            play::S_CONTAINER_CLICK => scheduled(ScheduledPlayPacketKind::ContainerClick(
                SContainerClick::read_packet(data)?,
            )),
            play::S_CONTAINER_CLOSE => scheduled(ScheduledPlayPacketKind::ContainerClose(
                SContainerClose::read_packet(data)?,
            )),
            play::S_CONTAINER_SLOT_STATE_CHANGED => {
                scheduled(ScheduledPlayPacketKind::ContainerSlotStateChanged(
                    SContainerSlotStateChanged::read_packet(data)?,
                ))
            }
            play::S_SET_CREATIVE_MODE_SLOT => {
                scheduled(ScheduledPlayPacketKind::SetCreativeModeSlot(
                    SSetCreativeModeSlot::read_packet(data)?,
                ))
            }
            play::S_PLAYER_INPUT => scheduled(ScheduledPlayPacketKind::PlayerInput(
                SPlayerInput::read_packet(data)?,
            )),
            play::S_PLAYER_COMMAND => scheduled(ScheduledPlayPacketKind::PlayerCommand(
                SPlayerCommand::read_packet(data)?,
            )),
            play::S_PLAYER_ABILITIES => scheduled(ScheduledPlayPacketKind::PlayerAbilities(
                SPlayerAbilities::read_packet(data)?,
            )),
            play::S_USE_ITEM_ON => scheduled(ScheduledPlayPacketKind::UseItemOn(
                SUseItemOn::read_packet(data)?,
            )),
            play::S_USE_ITEM => scheduled(ScheduledPlayPacketKind::UseItem(SUseItem::read_packet(
                data,
            )?)),
            play::S_SET_CARRIED_ITEM => scheduled(ScheduledPlayPacketKind::SetCarriedItem(
                SSetCarriedItem::read_packet(data)?,
            )),
            play::S_SWING => scheduled(ScheduledPlayPacketKind::Swing(SSwing::read_packet(data)?)),
            play::S_PLAYER_ACTION => scheduled(ScheduledPlayPacketKind::PlayerAction(
                SPlayerAction::read_packet(data)?,
            )),
            play::S_PICK_ITEM_FROM_BLOCK => scheduled(ScheduledPlayPacketKind::PickItemFromBlock(
                SPickItemFromBlock::read_packet(data)?,
            )),
            play::S_SIGN_UPDATE => scheduled(ScheduledPlayPacketKind::SignUpdate(
                SSignUpdate::read_packet(data)?,
            )),
            play::S_SPECTATOR_ACTION => scheduled(ScheduledPlayPacketKind::SpectatorAction(
                SSpectatorAction::read_packet(data)?,
            )),
            play::S_CLIENT_COMMAND => scheduled(ScheduledPlayPacketKind::ClientCommand(
                SClientCommand::read_packet(data)?,
            )),
            play::S_PING_REQUEST => DecodedPlayPacket::Immediate(ImmediatePlayPacket::PingRequest(
                SPingRequest::read_packet(data)?,
            )),
            play::S_CHANGE_GAME_MODE => scheduled(ScheduledPlayPacketKind::ChangeGameMode(
                SChangeGameMode::read_packet(data)?,
            )),
            play::S_CHANGE_DIFFICULTY => scheduled(ScheduledPlayPacketKind::ChangeDifficulty(
                SChangeDifficulty::read_packet(data)?,
            )),
            id => DecodedPlayPacket::Immediate(ImmediatePlayPacket::Unknown(id)),
        })
    }

    fn handle_immediate_packet(&self, packet: ImmediatePlayPacket, player: &Player) {
        match packet {
            ImmediatePlayPacket::KeepAlive(packet) => self.handle_keep_alive(packet),
            ImmediatePlayPacket::PingRequest(packet) => {
                player.send_packet(CPongResponse::new(packet.time));
            }
            ImmediatePlayPacket::ChunkBatchReceived(packet) => {
                player
                    .chunk_sender
                    .lock()
                    .on_chunk_batch_received_by_client(packet.desired_chunks_per_tick);
            }
            ImmediatePlayPacket::Unknown(id) => log::info!("play packet id {id} is not known"),
        }
    }

    /// Listens for packets from the client.
    pub async fn listener(
        &self,
        mut reader: TCPNetworkDecoder<BufReader<OwnedReadHalf>>,
        server: Arc<Server>,
    ) {
        loop {
            select! {
                () = self.wait_for_close() => {
                    break;
                }
                packet = reader.get_raw_packet() => {
                    match packet {
                        Ok(packet) => {
                            if let Some(player) = self.player.upgrade()
                                && let Err(err) = self.process_packet(packet, player, &server) {
                                log::warn!(
                                    "Failed to get packet from client {}: {err}",
                                    self.id
                                );
                            }
                        }
                        Err(err) => {
                            log::debug!("Failed to get raw packet from client {}: {err}", self.id);
                            self.close();
                        }
                    }
                }
            }
        }
    }

    /// Sends packets to the client.
    ///
    pub async fn sender(&self, mut sender_recv: UnboundedReceiver<OutboundPacket>) {
        loop {
            select! {
                biased;
                () = self.wait_for_close() => {
                    self.write_queued_disconnect(&mut sender_recv).await;
                    break;
                }
                outbound = sender_recv.recv() => {
                    if let Some(outbound) = outbound {
                        let (packet, close_after_write) = match outbound {
                            OutboundPacket::Packet(packet) => (packet, false),
                            OutboundPacket::Disconnect(packet) => (packet, true),
                        };

                        if close_after_write {
                            if let Err(err) = self.write_packet_now(&packet).await {
                                log::warn!("Failed to send disconnect packet to client {}: {err}", self.id);
                            }
                            self.close();
                            break;
                        }

                        let write_result = self.write_packet_now(&packet);
                        select! {
                            biased;
                            () = self.wait_for_close() => {
                                self.write_queued_disconnect(&mut sender_recv).await;
                                break;
                            },
                            result = write_result => {
                                if let Err(err) = result {
                                    log::warn!("Failed to send packet to client {}: {err}", self.id);
                                    self.close();
                                    break;
                                }
                            }
                        }
                    } else {
                        //log::warn!(
                        //    "Internal packet_sender_recv channel closed for client {}",
                        //    self.id
                        //);
                        self.close();
                    }
                }
            }
        }

        self.release_network_writer().await;

        let Some(player) = self.player.upgrade() else {
            return;
        };
        if !player.has_joined_world() || player.server().cancel_token.is_cancelled() {
            return;
        }
        player.server().queue_player_disconnect(player);
    }

    async fn write_queued_disconnect(&self, sender_recv: &mut UnboundedReceiver<OutboundPacket>) {
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
        if let Err(err) = self.write_packet_now(&packet).await {
            log::warn!(
                "Failed to send disconnect packet to client {} during close: {err}",
                self.id
            );
        }
    }
}

impl TextResolutor for JavaConnection {
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

impl NetworkConnection for JavaConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        self.compression
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.send_encoded_packet(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.send_packet(CBundleDelimiter);
        for packet in packets {
            self.send_encoded_packet(packet);
        }
        self.send_packet(CBundleDelimiter);
    }

    fn disconnect_with_reason(&self, reason: TextComponent) {
        self.disconnect(reason);
    }

    fn tick(&self) {
        self.keep_connection_alive();
    }

    fn latency(&self) -> i32 {
        *self.latency.lock() as i32
    }

    fn close(&self) {
        self.cancel_token.cancel();
    }

    fn closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use std::array;

    use rustc_hash::FxHashMap;
    use steel_protocol::packets::common::{ChatVisibility, HumanoidArm, ParticleStatus};
    use steel_protocol::packets::game::{ClickType, ClientCommandAction, HashedStack};
    use steel_registry::{blocks::properties::Direction, item_stack::ItemStack};
    use steel_utils::{BlockPos, codec::VarInt, types::InteractionHand};
    use uuid::Uuid;

    use super::*;

    fn decode(packet: RawPacket) -> DecodedPlayPacket {
        let Ok(decoded) = JavaConnection::decode_play_packet(packet) else {
            panic!("test play packet should decode");
        };
        decoded
    }

    fn execution(kind: ScheduledPlayPacketKind) -> ScheduledPacketExecution {
        ScheduledPlayPacket(kind).execution()
    }

    #[test]
    fn pre_join_custom_payload_uses_serverbound_play_packet_id() {
        assert!(JavaConnection::can_process_before_join(
            play::S_CUSTOM_PAYLOAD
        ));
        assert!(!JavaConnection::can_process_before_join(
            play::C_CUSTOM_PAYLOAD
        ));
    }

    #[test]
    fn custom_payload_defaults_to_global_exclusive_scheduling() {
        let channel = b"minecraft:brand";
        let mut payload = vec![channel.len() as u8];
        payload.extend_from_slice(channel);
        payload.extend_from_slice(b"steel");
        let decoded = decode(RawPacket {
            id: play::S_CUSTOM_PAYLOAD,
            payload,
        });
        let DecodedPlayPacket::Scheduled(
            packet @ ScheduledPlayPacket(ScheduledPlayPacketKind::CustomPayload(_)),
        ) = decoded
        else {
            panic!("custom payload should use the scheduled packet path");
        };

        assert_eq!(packet.execution(), ScheduledPacketExecution::Exclusive);
    }

    #[test]
    fn pre_join_allows_initial_play_acknowledgements() {
        assert!(JavaConnection::can_process_before_join(
            play::S_ACCEPT_TELEPORTATION
        ));
        assert!(JavaConnection::can_process_before_join(
            play::S_CHUNK_BATCH_RECEIVED
        ));
        assert!(JavaConnection::can_process_before_join(
            play::S_PLAYER_LOADED
        ));
    }

    #[test]
    fn client_tick_end_is_scheduled_for_the_inter_tick_phase() {
        let decoded = decode(RawPacket {
            id: play::S_CLIENT_TICK_END,
            payload: Vec::new(),
        });

        assert!(matches!(
            decoded,
            DecodedPlayPacket::Scheduled(ScheduledPlayPacket(
                ScheduledPlayPacketKind::ClientTickEnd
            ))
        ));
    }

    #[test]
    fn packet_execution_classification_separates_local_and_serialized_work() {
        assert_eq!(
            execution(ScheduledPlayPacketKind::PlayerAbilities(SPlayerAbilities {
                flags: 0
            },)),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::MovePlayer(
                SMovePlayerStatusOnly { packed_byte: 0 }.into(),
            )),
            ScheduledPacketExecution::Serialized
        );
    }

    #[test]
    fn inventory_execution_reflects_complete_transaction_boundaries() {
        let click = SContainerClick {
            container_id: 0,
            state_id: 0,
            slot_num: 0,
            button_num: 0,
            click_type: ClickType::Pickup,
            changed_slots: FxHashMap::default(),
            carried_item: HashedStack::Empty,
        };

        assert_eq!(
            execution(ScheduledPlayPacketKind::ContainerClick(click)),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::ContainerClose(SContainerClose {
                container_id: 0,
            })),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::SetCreativeModeSlot(
                SSetCreativeModeSlot {
                    slot_num: 1,
                    item_stack: ItemStack::empty(),
                },
            )),
            ScheduledPacketExecution::PlayerLocal
        );
    }

    #[test]
    fn player_command_execution_is_action_sensitive() {
        let command = |action| {
            execution(ScheduledPlayPacketKind::PlayerCommand(SPlayerCommand {
                entity_id: 1,
                action,
                data: 0,
            }))
        };

        assert_eq!(
            command(PlayerCommandAction::StartSprinting),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            command(PlayerCommandAction::StartFallFlying),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            command(PlayerCommandAction::LeaveBed),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            command(PlayerCommandAction::OpenVehicleInventory),
            ScheduledPacketExecution::Exclusive
        );
    }

    #[test]
    fn player_action_execution_is_action_sensitive() {
        let action = |action| {
            execution(ScheduledPlayPacketKind::PlayerAction(SPlayerAction {
                action,
                pos: BlockPos::new(0, 64, 0),
                direction: Direction::Down,
                sequence: 0,
            }))
        };

        assert_eq!(
            action(PlayerAction::AbortDestroyBlock),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            action(PlayerAction::SwapItemWithOffhand),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            action(PlayerAction::StartDestroyBlock),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            action(PlayerAction::Stab),
            ScheduledPacketExecution::Exclusive
        );
    }

    #[test]
    fn chat_message_and_ack_share_the_serialized_commit_lane() {
        assert_eq!(
            execution(ScheduledPlayPacketKind::Chat(Box::new(SChat {
                message: "hello".to_owned(),
                timestamp: 0,
                salt: 0,
                signature: None,
                offset: 0,
                acknowledged: [0; 3],
                checksum: 0,
            }))),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::ChatAck(SChatAck {
                offset: VarInt(0),
            })),
            ScheduledPacketExecution::Serialized
        );
    }

    #[test]
    fn cross_player_and_unimplemented_handlers_remain_global_barriers() {
        assert_eq!(
            execution(ScheduledPlayPacketKind::Attack(SAttack { entity_id: 1 })),
            ScheduledPacketExecution::Exclusive
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::ContainerButtonClick(
                SContainerButtonClick {
                    container_id: 1,
                    button_id: 0,
                },
            )),
            ScheduledPacketExecution::Exclusive
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::PlayerAction(SPlayerAction {
                action: PlayerAction::ReleaseUseItem,
                pos: BlockPos::new(0, 64, 0),
                direction: Direction::Down,
                sequence: 0,
            })),
            ScheduledPacketExecution::Exclusive
        );
    }

    #[test]
    fn audited_handlers_use_the_narrowest_safe_execution_class() {
        assert_eq!(
            execution(ScheduledPlayPacketKind::AcceptTeleportation(
                SAcceptTeleportation { teleport_id: 1 },
            )),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::PlayerInput(SPlayerInput {
                flags: 0,
            })),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::ChatSessionUpdate(
                SChatSessionUpdate {
                    session_id: Uuid::nil(),
                    expires_at: 0,
                    public_key: Vec::new(),
                    key_signature: Vec::new(),
                },
            )),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::ClientInformation(
                SClientInformation {
                    language: "en_us".to_owned(),
                    view_distance: 8,
                    chat_visibility: ChatVisibility::Full,
                    chat_colors: true,
                    model_customization: 0,
                    main_hand: HumanoidArm::Right,
                    text_filtering_enabled: false,
                    allows_listing: true,
                    particle_status: ParticleStatus::All,
                },
            )),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::ChatCommand(SChatCommand {
                command: "help".to_owned(),
            })),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::PickItemFromBlock(
                SPickItemFromBlock {
                    pos: BlockPos::new(0, 64, 0),
                    include_data: false,
                },
            )),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::SignUpdate(SSignUpdate {
                pos: BlockPos::new(0, 64, 0),
                is_front_text: true,
                lines: array::from_fn(|_| String::new()),
            })),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::Swing(SSwing {
                hand: InteractionHand::MainHand,
            })),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(ScheduledPlayPacketKind::ClientCommand(SClientCommand {
                action: ClientCommandAction::PerformRespawn,
            })),
            ScheduledPacketExecution::PlayerLocal
        );
    }

    #[test]
    fn keep_alive_remains_on_the_immediate_connection_path() {
        let decoded = decode(RawPacket {
            id: play::S_KEEP_ALIVE,
            payload: 42_i64.to_be_bytes().to_vec(),
        });

        assert!(matches!(
            decoded,
            DecodedPlayPacket::Immediate(ImmediatePlayPacket::KeepAlive(SKeepAlive { id: 42 }))
        ));
    }

    #[test]
    fn chunk_batch_ack_uses_the_immediate_connection_path() {
        let decoded = decode(RawPacket {
            id: play::S_CHUNK_BATCH_RECEIVED,
            payload: 12.5_f32.to_be_bytes().to_vec(),
        });

        assert!(matches!(
            decoded,
            DecodedPlayPacket::Immediate(ImmediatePlayPacket::ChunkBatchReceived(
                SChunkBatchReceived {
                    desired_chunks_per_tick: 12.5
                }
            ))
        ));
    }
}
