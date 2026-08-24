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
    CBundleDelimiter, CCommandSuggestions, ClientCommandAction, PlayerAction, PlayerCommandAction,
    SAcceptTeleportation, SAttack, SChangeDifficulty, SChangeGameMode, SChat, SChatAck,
    SChatCommand, SChatSessionUpdate, SChunkBatchReceived, SClientCommand, SClientTickEnd,
    SCommandSuggestion, SContainerButtonClick, SContainerClick, SContainerClose,
    SContainerSlotStateChanged, SInteract, SMovePlayer, SMovePlayerPos, SMovePlayerPosRot,
    SMovePlayerRot, SMovePlayerStatusOnly, SMoveVehicle, SPickItemFromBlock, SPlayerAbilities,
    SPlayerAction, SPlayerCommand, SPlayerInput, SPlayerLoad, SRenameItem, SSetCarriedItem,
    SSetCreativeModeSlot, SSignUpdate, SSpectatorAction, SSwing, SUseItem, SUseItemOn,
};

use steel_protocol::utils::{ConnectionProtocol, PacketError, RawPacket};
use steel_registry::packets::play;
use steel_utils::locks::{AsyncMutex, SyncMutex};
use steel_utils::translations;
use text_components::content::Resolvable;
use text_components::custom::CustomData;
use text_components::resolving::TextResolutor;
use text_components::{Modifier, TextComponent, format::Color};
use tokio::io::{AsyncWrite, BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio::task::yield_now;
use tokio_util::sync::CancellationToken;

use crate::command::{handle_client_request, sender::CommandSender};
use crate::player::Player;
use crate::player::connection::NetworkConnection;
use crate::server::Server;

type JavaPacketWriter = TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>;
/// Shared Java socket writer during pre-play states.
pub type JavaNetworkWriter = Arc<AsyncMutex<Option<JavaPacketWriter>>>;

// Tuned by the release `outbound_transport` A/B; these bound cooperative work, not protocol size.
const MAX_PACKETS_PER_WRITE_QUANTUM: usize = 32;
const MAX_BYTES_PER_WRITE_QUANTUM: usize = 256 * 1_024;

/// Outbound packet queue message for Java connections.
pub enum OutboundPacket {
    /// One packet that is flushed immediately.
    Packet(EncodedPacket),
    /// Separate protocol packets kept contiguous and flushed in bounded quanta.
    /// Boxing keeps ordinary per-packet queue entries compact.
    #[expect(
        clippy::box_collection,
        reason = "boxing the rare batch keeps every common per-packet channel slot compact"
    )]
    PacketBatch(Box<Vec<EncodedPacket>>),
    /// Final disconnect packet that must be flushed before closing the socket.
    Disconnect(EncodedPacket),
}

enum QueuedWrite {
    Packet(EncodedPacket),
    Batch(Vec<EncodedPacket>),
}

struct ClosingWrites {
    queued: Vec<QueuedWrite>,
    disconnect: EncodedPacket,
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
    RenameItem(SRenameItem),
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
    /// Returns whether this packet acknowledges target-world synchronization.
    pub(crate) const fn is_domain_handshake_packet(&self) -> bool {
        matches!(
            self.0,
            ScheduledPlayPacketKind::AcceptTeleportation(_) | ScheduledPlayPacketKind::PlayerLoaded
        )
    }

    /// Returns whether this is the death screen's one-shot respawn request.
    pub(crate) const fn is_perform_respawn(&self) -> bool {
        matches!(
            self.0,
            ScheduledPlayPacketKind::ClientCommand(SClientCommand {
                action: ClientCommandAction::PerformRespawn,
            })
        )
    }

    #[cfg(test)]
    pub(crate) const fn perform_respawn_for_test() -> Self {
        Self(ScheduledPlayPacketKind::ClientCommand(SClientCommand {
            action: ClientCommandAction::PerformRespawn,
        }))
    }

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
                // Active-use release may invoke item behavior and mutate inventory, while stab
                // spans independently locked targets; neither can overlap player-local work.
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
            | ScheduledPlayPacketKind::RenameItem(_)
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
            ScheduledPlayPacketKind::RenameItem(packet) => player.handle_rename_item(packet),
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

    fn enqueue_packet_batch(&self, packets: Vec<EncodedPacket>) {
        if packets.is_empty() {
            return;
        }
        if self
            .outgoing_packets
            .send(OutboundPacket::PacketBatch(Box::new(packets)))
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

    const fn can_process_during_domain_handshake(packet_id: i32) -> bool {
        matches!(
            packet_id,
            play::S_ACCEPT_TELEPORTATION | play::S_CHUNK_BATCH_RECEIVED | play::S_PLAYER_LOADED
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

        let payload_bytes = packet.payload().len();
        let Some(packet) = Self::decode_domain_gated_packet(packet, &player)? else {
            return Ok(());
        };

        match packet {
            DecodedPlayPacket::Scheduled(packet) => {
                server.schedule_play_packet(player, packet, payload_bytes);
            }
            DecodedPlayPacket::Immediate(packet) => {
                self.handle_immediate_packet(packet, &player);
            }
        }
        Ok(())
    }

    fn decode_domain_gated_packet(
        packet: RawPacket,
        player: &Player,
    ) -> Result<Option<DecodedPlayPacket>, PacketError> {
        let maintenance_packet = matches!(packet.id, play::S_KEEP_ALIVE | play::S_PING_REQUEST);
        let handshake_packet = Self::can_process_during_domain_handshake(packet.id);
        if packet.id == play::S_CLIENT_COMMAND {
            let decoded = Self::decode_play_packet(packet)?;
            let perform_respawn = matches!(
                &decoded,
                DecodedPlayPacket::Scheduled(packet) if packet.is_perform_respawn()
            );
            if player.gate_domain_switch_packet(false, perform_respawn) {
                return Ok(Some(decoded));
            }
            return Ok(None);
        }

        if !maintenance_packet && !player.gate_domain_switch_packet(handshake_packet, false) {
            return Ok(None);
        }

        Self::decode_play_packet(packet).map(Some)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "single match decode over all implemented play packets keeps protocol routing auditable"
    )]
    fn decode_play_packet(packet: RawPacket) -> Result<DecodedPlayPacket, PacketError> {
        let data = &mut Cursor::new(packet.payload());
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
            play::S_RENAME_ITEM => scheduled(ScheduledPlayPacketKind::RenameItem(
                SRenameItem::read_packet(data)?,
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
        let mut shared_writer = self.network_writer.lock().await;
        let Some(mut network_writer) = shared_writer.take() else {
            return;
        };
        drop(shared_writer);

        self.send_packets(&mut network_writer, &mut sender_recv)
            .await;
        drop(network_writer);

        let Some(player) = self.player.upgrade() else {
            return;
        };
        if !player.has_joined_world() || player.server().cancel_token.is_cancelled() {
            return;
        }
        player.server().queue_player_disconnect(player);
    }

    async fn send_packets<W: AsyncWrite + Unpin>(
        &self,
        network_writer: &mut TCPNetworkEncoder<W>,
        sender_recv: &mut UnboundedReceiver<OutboundPacket>,
    ) {
        let close = self.wait_for_close();
        tokio::pin!(close);

        loop {
            select! {
                biased;
                () = close.as_mut() => {
                    if let Some(closing_writes) = Self::close_and_take_writes_through_disconnect(sender_recv) {
                        self.write_closing_queue(network_writer, closing_writes).await;
                    }
                    break;
                }
                outbound = sender_recv.recv() => {
                    if let Some(outbound) = outbound {
                        let queued_write = match outbound {
                            OutboundPacket::Packet(packet) => QueuedWrite::Packet(packet),
                            OutboundPacket::PacketBatch(packets) => QueuedWrite::Batch(*packets),
                            OutboundPacket::Disconnect(packet) => {
                                sender_recv.close();
                                if let Err(err) = network_writer.write_packet(&packet).await {
                                    log::warn!("Failed to send disconnect packet to client {}: {err}", self.id);
                                }
                                self.close();
                                break;
                            }
                        };

                        let closing_writes = {
                            let write_result = Self::write_queued(network_writer, queued_write);
                            tokio::pin!(write_result);
                            select! {
                                biased;
                                () = close.as_mut() => {
                                    let Some(closing_writes) = Self::close_and_take_writes_through_disconnect(sender_recv) else {
                                        break;
                                    };
                                    if let Err(err) = write_result.as_mut().await {
                                        log::warn!("Failed to finish packet before disconnecting client {}: {err}", self.id);
                                        break;
                                    }
                                    Some(closing_writes)
                                },
                                result = write_result.as_mut() => {
                                    if let Err(err) = result {
                                        log::warn!("Failed to send packet to client {}: {err}", self.id);
                                        self.close();
                                        break;
                                    }
                                    None
                                }
                            }
                        };
                        if let Some(closing_writes) = closing_writes {
                            self.write_closing_queue(network_writer, closing_writes).await;
                            break;
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
    }

    async fn write_queued<W: AsyncWrite + Unpin>(
        network_writer: &mut TCPNetworkEncoder<W>,
        queued_write: QueuedWrite,
    ) -> Result<(), PacketError> {
        match queued_write {
            QueuedWrite::Packet(packet) => network_writer.write_packet(&packet).await,
            QueuedWrite::Batch(packets) => Self::write_packet_batch(network_writer, &packets).await,
        }
    }

    async fn write_packet_batch<W: AsyncWrite + Unpin>(
        network_writer: &mut TCPNetworkEncoder<W>,
        packets: &[EncodedPacket],
    ) -> Result<(), PacketError> {
        Self::write_packet_batch_with_limits(
            network_writer,
            packets,
            MAX_PACKETS_PER_WRITE_QUANTUM,
            MAX_BYTES_PER_WRITE_QUANTUM,
        )
        .await
    }

    async fn write_packet_batch_with_limits<W: AsyncWrite + Unpin>(
        network_writer: &mut TCPNetworkEncoder<W>,
        packets: &[EncodedPacket],
        max_packets: usize,
        max_bytes: usize,
    ) -> Result<(), PacketError> {
        let mut pending_packets = 0usize;
        let mut pending_bytes = 0usize;

        for packet in packets {
            let next_bytes = pending_bytes.saturating_add(packet.encoded_data.len());
            if pending_packets != 0 && next_bytes > max_bytes {
                network_writer.flush().await?;
                pending_packets = 0;
                pending_bytes = 0;
                yield_now().await;
            }

            if let Err(write_error) = network_writer.write_packet_unflushed(packet).await {
                // Drain already accepted ciphertext once without retrying plaintext.
                let flush_result = network_writer.flush().await;
                drop(flush_result);
                return Err(write_error);
            }
            pending_packets += 1;
            pending_bytes = pending_bytes.saturating_add(packet.encoded_data.len());

            if pending_packets >= max_packets || pending_bytes >= max_bytes {
                network_writer.flush().await?;
                pending_packets = 0;
                pending_bytes = 0;
                yield_now().await;
            }
        }

        if pending_packets != 0 {
            network_writer.flush().await?;
        }
        Ok(())
    }

    fn close_and_take_writes_through_disconnect(
        sender_recv: &mut UnboundedReceiver<OutboundPacket>,
    ) -> Option<ClosingWrites> {
        sender_recv.close();
        let mut queued = Vec::new();
        loop {
            match sender_recv.try_recv() {
                Ok(OutboundPacket::Packet(packet)) => queued.push(QueuedWrite::Packet(packet)),
                Ok(OutboundPacket::PacketBatch(packets)) => {
                    queued.push(QueuedWrite::Batch(*packets));
                }
                Ok(OutboundPacket::Disconnect(disconnect)) => {
                    return Some(ClosingWrites { queued, disconnect });
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
            }
        }
    }

    async fn write_closing_queue<W: AsyncWrite + Unpin>(
        &self,
        network_writer: &mut TCPNetworkEncoder<W>,
        closing: ClosingWrites,
    ) {
        for queued_write in closing.queued {
            if let Err(err) = Self::write_queued(network_writer, queued_write).await {
                log::warn!(
                    "Failed to finish the outbound queue for client {} during disconnect: {err}",
                    self.id
                );
                return;
            }
        }

        if let Err(err) = network_writer.write_packet(&closing.disconnect).await {
            log::warn!(
                "Failed to send the disconnect packet to client {}: {err}",
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

    fn send_encoded_batch(&self, packets: Vec<EncodedPacket>) {
        self.enqueue_packet_batch(packets);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        let delimiter = match EncodedPacket::from_bare(
            CBundleDelimiter,
            self.compression,
            ConnectionProtocol::Play,
        ) {
            Ok(packet) => packet,
            Err(err) => {
                log::warn!(
                    "Failed to encode bundle delimiter for client {}: {err}",
                    self.id
                );
                self.close();
                return;
            }
        };
        let mut batch = Vec::with_capacity(packets.len().saturating_add(2));
        batch.push(delimiter.clone());
        batch.extend(packets);
        batch.push(delimiter);
        self.enqueue_packet_batch(batch);
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
    use std::{
        array,
        future::Future,
        io,
        pin::Pin,
        sync::Arc,
        task::{Context, Poll, Waker},
    };

    use crate::{
        entity::{Entity as _, LivingEntity as _},
        test_support::{TestPlayerBuilder, fresh_test_world},
    };
    use cfb8::cipher::KeyIvInit;
    use rustc_hash::FxHashMap;
    use steel_protocol::packets::common::{ChatVisibility, HumanoidArm, ParticleStatus};
    use steel_protocol::packets::game::{ClickType, ClientCommandAction, HashedStack};
    use steel_registry::{blocks::properties::Direction, item_stack::ItemStack};
    use steel_utils::{BlockPos, FrontVec, codec::VarInt, types::InteractionHand};
    use tokio::{
        io::{AsyncReadExt, AsyncWrite},
        net::{TcpListener, TcpStream},
        sync::{Notify, mpsc},
    };
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

    fn encoded_bytes(bytes: &[u8]) -> EncodedPacket {
        let mut data = FrontVec::new(0);
        data.extend_from_slice(bytes);
        EncodedPacket {
            encoded_data: Arc::new(data),
        }
    }

    fn encoded_marker(marker: u8) -> EncodedPacket {
        encoded_bytes(&[marker])
    }

    #[derive(Default)]
    struct RecordingWriterState {
        bytes: Vec<u8>,
        flush_offsets: Vec<usize>,
    }

    struct RecordingWriter {
        state: Arc<SyncMutex<RecordingWriterState>>,
    }

    impl AsyncWrite for RecordingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.state.lock().bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            let mut state = self.state.lock();
            let offset = state.bytes.len();
            state.flush_offsets.push(offset);
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[derive(Default)]
    struct PausingWriterState {
        bytes: Vec<u8>,
        released: bool,
        waker: Option<Waker>,
    }

    struct PausingWriter {
        state: Arc<SyncMutex<PausingWriterState>>,
        paused: Arc<Notify>,
        pause_after: usize,
    }

    impl AsyncWrite for PausingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            let mut state = self.state.lock();
            if !state.released && state.bytes.len() >= self.pause_after {
                state.waker = Some(cx.waker().clone());
                drop(state);
                self.paused.notify_one();
                return Poll::Pending;
            }

            let write_len = if state.released {
                bytes.len()
            } else {
                bytes.len().min(self.pause_after - state.bytes.len())
            };
            state.bytes.extend_from_slice(&bytes[..write_len]);
            Poll::Ready(Ok(write_len))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl PausingWriter {
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

    struct PausingFlushWriter {
        state: Arc<SyncMutex<PausingWriterState>>,
        paused: Arc<Notify>,
    }

    impl AsyncWrite for PausingFlushWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.state.lock().bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            let mut state = self.state.lock();
            if state.released {
                return Poll::Ready(Ok(()));
            }
            state.waker = Some(cx.waker().clone());
            drop(state);
            self.paused.notify_one();
            Poll::Pending
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl PausingFlushWriter {
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

    #[test]
    fn disconnect_close_preserves_queued_packet_order() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        assert!(
            sender
                .send(OutboundPacket::Packet(encoded_marker(1)))
                .is_ok()
        );
        assert!(
            sender
                .send(OutboundPacket::PacketBatch(Box::new(vec![
                    encoded_marker(2),
                    encoded_marker(3),
                ])))
                .is_ok()
        );
        assert!(
            sender
                .send(OutboundPacket::Disconnect(encoded_marker(4)))
                .is_ok()
        );
        assert!(
            sender
                .send(OutboundPacket::Packet(encoded_marker(5)))
                .is_ok()
        );

        let Some(closing) = JavaConnection::close_and_take_writes_through_disconnect(&mut receiver)
        else {
            panic!("disconnect packet should be present");
        };
        let mut markers = Vec::new();
        for queued_write in closing.queued {
            match queued_write {
                QueuedWrite::Packet(packet) => markers.push(packet.encoded_data[0]),
                QueuedWrite::Batch(packets) => {
                    markers.extend(packets.iter().map(|packet| packet.encoded_data[0]));
                }
            }
        }
        markers.push(closing.disconnect.encoded_data[0]);

        assert_eq!(markers, [1, 2, 3, 4]);
        assert!(matches!(
            receiver.try_recv(),
            Ok(OutboundPacket::Packet(packet)) if packet.encoded_data[0] == 5
        ));
    }

    #[test]
    fn hard_close_discards_queue_without_disconnect_packet() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        assert!(
            sender
                .send(OutboundPacket::Packet(encoded_marker(1)))
                .is_ok()
        );

        assert!(JavaConnection::close_and_take_writes_through_disconnect(&mut receiver).is_none());
        assert!(matches!(
            receiver.try_recv(),
            Err(TryRecvError::Empty | TryRecvError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn packet_batch_flushes_at_packet_and_byte_limits() {
        let packet_limited_state = Arc::new(SyncMutex::new(RecordingWriterState::default()));
        let mut packet_limited_encoder = TCPNetworkEncoder::new(RecordingWriter {
            state: Arc::clone(&packet_limited_state),
        });
        let packet_limited = (1..=5).map(encoded_marker).collect::<Vec<_>>();

        JavaConnection::write_packet_batch_with_limits(
            &mut packet_limited_encoder,
            &packet_limited,
            2,
            usize::MAX,
        )
        .await
        .expect("packet-limited batch should write");

        {
            let packet_limited_result = packet_limited_state.lock();
            assert_eq!(packet_limited_result.bytes, [1, 2, 3, 4, 5]);
            assert_eq!(packet_limited_result.flush_offsets, [2, 4, 5]);
        }

        let byte_limited_state = Arc::new(SyncMutex::new(RecordingWriterState::default()));
        let mut byte_limited_encoder = TCPNetworkEncoder::new(RecordingWriter {
            state: Arc::clone(&byte_limited_state),
        });
        let byte_limited = vec![
            encoded_bytes(&[1, 2]),
            encoded_bytes(&[3, 4]),
            encoded_bytes(&[5, 6, 7, 8, 9]),
            encoded_marker(10),
        ];

        JavaConnection::write_packet_batch_with_limits(
            &mut byte_limited_encoder,
            &byte_limited,
            usize::MAX,
            4,
        )
        .await
        .expect("byte-limited batch should write");

        let byte_limited_result = byte_limited_state.lock();
        assert_eq!(byte_limited_result.bytes, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(byte_limited_result.flush_offsets, [4, 9, 10]);
    }

    #[tokio::test]
    async fn packet_batch_yields_between_bounded_quanta() {
        let state = Arc::new(SyncMutex::new(RecordingWriterState::default()));
        let mut encoder = TCPNetworkEncoder::new(RecordingWriter {
            state: Arc::clone(&state),
        });
        let packets = [encoded_marker(1), encoded_marker(2)];
        let mut write = Box::pin(JavaConnection::write_packet_batch_with_limits(
            &mut encoder,
            &packets,
            1,
            usize::MAX,
        ));
        let mut context = Context::from_waker(Waker::noop());

        assert!(write.as_mut().poll(&mut context).is_pending());
        assert_eq!(state.lock().bytes, [1]);
        assert_eq!(state.lock().flush_offsets, [1]);

        assert!(write.as_mut().poll(&mut context).is_pending());
        assert_eq!(state.lock().bytes, [1, 2]);
        assert_eq!(state.lock().flush_offsets, [1, 2]);

        write.await.expect("second write quantum should finish");
        let result = state.lock();
        assert_eq!(result.bytes, [1, 2]);
        assert_eq!(result.flush_offsets, [1, 2]);
    }

    #[tokio::test]
    async fn sender_keeps_batch_contiguous_across_write_quanta() {
        let state = Arc::new(SyncMutex::new(RecordingWriterState::default()));
        let writer = RecordingWriter {
            state: Arc::clone(&state),
        };
        let (outgoing, mut receiver) = mpsc::unbounded_channel();
        let connection = Arc::new(JavaConnection::new(
            outgoing.clone(),
            CancellationToken::new(),
            None,
            Arc::new(AsyncMutex::new(None)),
            1,
            Weak::new(),
        ));
        let sender_connection = Arc::clone(&connection);
        let sender = tokio::spawn(async move {
            let mut network_writer = TCPNetworkEncoder::new(writer);
            sender_connection
                .send_packets(&mut network_writer, &mut receiver)
                .await;
        });
        let marker = |value| u8::try_from(value).expect("write quantum should fit a test marker");
        let batch_end = MAX_PACKETS_PER_WRITE_QUANTUM + 1;
        let normal_marker = marker(batch_end + 1);
        let disconnect_marker = marker(batch_end + 2);
        let batch = (1..=batch_end)
            .map(|value| encoded_marker(marker(value)))
            .collect::<Vec<_>>();

        assert!(
            outgoing
                .send(OutboundPacket::PacketBatch(Box::new(batch)))
                .is_ok()
        );
        assert!(
            outgoing
                .send(OutboundPacket::Packet(encoded_marker(normal_marker)))
                .is_ok()
        );
        assert!(
            outgoing
                .send(OutboundPacket::Disconnect(encoded_marker(
                    disconnect_marker,
                )))
                .is_ok()
        );
        sender.await.expect("sender task should finish");

        let result = state.lock();
        assert_eq!(result.bytes, (1..=disconnect_marker).collect::<Vec<_>>());
        assert_eq!(
            result.flush_offsets,
            [
                MAX_PACKETS_PER_WRITE_QUANTUM,
                batch_end,
                batch_end + 1,
                batch_end + 2,
            ]
        );
    }

    #[tokio::test]
    async fn encrypted_packet_batch_matches_one_continuous_cfb8_stream() {
        const KEY: [u8; 16] = *b"batch-cipher-key";

        let state = Arc::new(SyncMutex::new(RecordingWriterState::default()));
        let mut encoder = TCPNetworkEncoder::new(RecordingWriter {
            state: Arc::clone(&state),
        });
        encoder.set_encryption(&KEY);
        let payloads = [
            vec![0x11],
            vec![0x22; 255],
            vec![0x33; 257],
            vec![0x44; 1_025],
        ];
        let packets = payloads
            .iter()
            .map(|payload| encoded_bytes(payload))
            .collect::<Vec<_>>();

        JavaConnection::write_packet_batch_with_limits(&mut encoder, &packets, 2, 300)
            .await
            .expect("encrypted batch should write");

        let mut expected = payloads.concat();
        cfb8::Encryptor::<aes::Aes128>::new(&KEY.into(), &KEY.into()).encrypt(&mut expected);
        let result = state.lock();
        assert_eq!(result.bytes, expected);
        assert_eq!(result.flush_offsets.len(), 3);
    }

    #[test]
    fn bundle_is_enqueued_as_one_ordered_transport_batch() {
        let (outgoing, mut receiver) = mpsc::unbounded_channel();
        let connection = JavaConnection::new(
            outgoing,
            CancellationToken::new(),
            None,
            Arc::new(AsyncMutex::new(None)),
            1,
            Weak::new(),
        );

        connection.send_encoded_bundle(vec![encoded_marker(7)]);

        let expected_delimiter =
            EncodedPacket::from_bare(CBundleDelimiter, None, ConnectionProtocol::Play)
                .expect("bundle delimiter should encode");
        let Ok(OutboundPacket::PacketBatch(packets)) = receiver.try_recv() else {
            panic!("bundle should occupy one outbound queue entry");
        };
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[1].encoded_data.as_slice(), [7]);
        assert_eq!(packets[0].encoded_data, expected_delimiter.encoded_data);
        assert_eq!(packets[2].encoded_data, expected_delimiter.encoded_data);
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn play_sender_takes_writer_and_writes_fifo() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener should bind");
        let address = listener
            .local_addr()
            .expect("loopback listener should have an address");
        let connect = TcpStream::connect(address);
        let accept = listener.accept();
        let (client_result, server_result) = tokio::join!(connect, accept);
        let mut client = client_result.expect("loopback client should connect");
        let (server, _) = server_result.expect("loopback server should accept");
        let (_, server_write) = server.into_split();

        let network_writer: JavaNetworkWriter = Arc::new(AsyncMutex::new(Some(
            TCPNetworkEncoder::new(BufWriter::new(server_write)),
        )));
        let (outgoing, receiver) = mpsc::unbounded_channel();
        let connection = Arc::new(JavaConnection::new(
            outgoing.clone(),
            CancellationToken::new(),
            None,
            Arc::clone(&network_writer),
            1,
            Weak::new(),
        ));
        let sender_connection = Arc::clone(&connection);
        let sender = tokio::spawn(async move {
            sender_connection.sender(receiver).await;
        });

        assert!(
            outgoing
                .send(OutboundPacket::Packet(encoded_marker(1)))
                .is_ok()
        );
        assert!(
            outgoing
                .send(OutboundPacket::Disconnect(encoded_marker(2)))
                .is_ok()
        );

        let mut wire_bytes = [0; 2];
        client
            .read_exact(&mut wire_bytes)
            .await
            .expect("loopback client should receive both packets");
        sender.await.expect("sender task should finish");

        assert_eq!(wire_bytes, [1, 2]);
        assert!(network_writer.lock().await.is_none());
    }

    #[tokio::test]
    async fn disconnect_finishes_partial_batch_before_terminal_packet() {
        let state = Arc::new(SyncMutex::new(PausingWriterState::default()));
        let paused = Arc::new(Notify::new());
        let writer = PausingWriter {
            state: Arc::clone(&state),
            paused: Arc::clone(&paused),
            pause_after: 1,
        };
        let writer_control = PausingWriter {
            state: Arc::clone(&state),
            paused: Arc::clone(&paused),
            pause_after: 1,
        };
        let (outgoing, mut receiver) = mpsc::unbounded_channel();
        let connection = Arc::new(JavaConnection::new(
            outgoing.clone(),
            CancellationToken::new(),
            None,
            Arc::new(AsyncMutex::new(None)),
            1,
            Weak::new(),
        ));
        let sender_connection = Arc::clone(&connection);
        let sender = tokio::spawn(async move {
            let mut network_writer = TCPNetworkEncoder::new(writer);
            sender_connection
                .send_packets(&mut network_writer, &mut receiver)
                .await;
        });

        assert!(
            outgoing
                .send(OutboundPacket::PacketBatch(Box::new(vec![
                    encoded_bytes(&[1, 2]),
                    encoded_marker(3),
                ])))
                .is_ok()
        );
        paused.notified().await;
        assert!(
            outgoing
                .send(OutboundPacket::PacketBatch(Box::new(vec![
                    encoded_marker(4),
                    encoded_marker(5),
                ])))
                .is_ok()
        );
        assert!(
            outgoing
                .send(OutboundPacket::Disconnect(encoded_marker(6)))
                .is_ok()
        );
        assert!(
            outgoing
                .send(OutboundPacket::Packet(encoded_marker(7)))
                .is_ok()
        );
        connection.close();
        outgoing.closed().await;

        assert!(!sender.is_finished());
        assert!(
            outgoing
                .send(OutboundPacket::Packet(encoded_marker(8)))
                .is_err()
        );
        writer_control.release();
        sender.await.expect("sender task should finish");

        assert_eq!(state.lock().bytes, [1, 2, 3, 4, 5, 6]);
    }

    #[tokio::test]
    async fn disconnect_finishes_pending_batch_flush_before_terminal_packet() {
        let state = Arc::new(SyncMutex::new(PausingWriterState::default()));
        let paused = Arc::new(Notify::new());
        let writer = PausingFlushWriter {
            state: Arc::clone(&state),
            paused: Arc::clone(&paused),
        };
        let writer_control = PausingFlushWriter {
            state: Arc::clone(&state),
            paused: Arc::clone(&paused),
        };
        let (outgoing, mut receiver) = mpsc::unbounded_channel();
        let connection = Arc::new(JavaConnection::new(
            outgoing.clone(),
            CancellationToken::new(),
            None,
            Arc::new(AsyncMutex::new(None)),
            1,
            Weak::new(),
        ));
        let sender_connection = Arc::clone(&connection);
        let sender = tokio::spawn(async move {
            let mut network_writer = TCPNetworkEncoder::new(writer);
            sender_connection
                .send_packets(&mut network_writer, &mut receiver)
                .await;
        });

        assert!(
            outgoing
                .send(OutboundPacket::PacketBatch(Box::new(vec![encoded_marker(
                    1
                )])))
                .is_ok()
        );
        paused.notified().await;
        assert!(
            outgoing
                .send(OutboundPacket::Packet(encoded_marker(2)))
                .is_ok()
        );
        assert!(
            outgoing
                .send(OutboundPacket::Disconnect(encoded_marker(3)))
                .is_ok()
        );
        assert!(
            outgoing
                .send(OutboundPacket::Packet(encoded_marker(4)))
                .is_ok()
        );
        connection.close();
        outgoing.closed().await;

        assert!(!sender.is_finished());
        writer_control.release();
        sender.await.expect("sender task should finish");

        assert_eq!(state.lock().bytes, [1, 2, 3]);
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
    fn queued_domain_switch_records_only_perform_respawn_at_connection_gate() {
        let world = fresh_test_world("queued_domain_switch_respawn_packet");
        let player = TestPlayerBuilder::new(world, "RespawnTester", 1).build();
        let Some(token) = player.begin_pending_world_change() else {
            panic!("test player should acquire a world-change token");
        };
        assert!(player.begin_domain_switch(token));
        player.set_health(0.0);

        let request_stats = JavaConnection::decode_domain_gated_packet(
            RawPacket::new(
                play::S_CLIENT_COMMAND,
                vec![ClientCommandAction::RequestStats as u8],
            ),
            &player,
        );
        assert!(matches!(request_stats, Ok(None)));
        assert!(!player.has_deferred_death_respawn_for_test());

        let perform_respawn = JavaConnection::decode_domain_gated_packet(
            RawPacket::new(
                play::S_CLIENT_COMMAND,
                vec![ClientCommandAction::PerformRespawn as u8],
            ),
            &player,
        );
        assert!(matches!(perform_respawn, Ok(None)));
        assert!(player.has_deferred_death_respawn_for_test());

        assert!(player.finish_domain_switch(token));
        assert!(player.finish_pending_world_change(token));
    }

    #[test]
    fn custom_payload_defaults_to_global_exclusive_scheduling() {
        let channel = b"minecraft:brand";
        let mut payload = vec![channel.len() as u8];
        payload.extend_from_slice(channel);
        payload.extend_from_slice(b"steel");
        let decoded = decode(RawPacket::new(play::S_CUSTOM_PAYLOAD, payload));
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
        assert!(JavaConnection::can_process_during_domain_handshake(
            play::S_ACCEPT_TELEPORTATION
        ));
        assert!(JavaConnection::can_process_during_domain_handshake(
            play::S_CHUNK_BATCH_RECEIVED
        ));
        assert!(JavaConnection::can_process_during_domain_handshake(
            play::S_PLAYER_LOADED
        ));
        assert!(!JavaConnection::can_process_during_domain_handshake(
            play::S_MOVE_PLAYER_POS
        ));
    }

    #[test]
    fn scheduled_domain_handshake_classification_is_narrow() {
        let accept = decode(RawPacket::new(play::S_ACCEPT_TELEPORTATION, vec![0]));
        let DecodedPlayPacket::Scheduled(accept) = accept else {
            panic!("teleport acknowledgement should be scheduled");
        };
        assert!(accept.is_domain_handshake_packet());

        let client_tick_end = decode(RawPacket::new(play::S_CLIENT_TICK_END, Vec::new()));
        let DecodedPlayPacket::Scheduled(client_tick_end) = client_tick_end else {
            panic!("client tick end should be scheduled");
        };
        assert!(!client_tick_end.is_domain_handshake_packet());
    }

    #[test]
    fn client_tick_end_is_scheduled_for_the_inter_tick_phase() {
        let decoded = decode(RawPacket::new(play::S_CLIENT_TICK_END, Vec::new()));

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
        let decoded = decode(RawPacket::new(
            play::S_KEEP_ALIVE,
            42_i64.to_be_bytes().to_vec(),
        ));

        assert!(matches!(
            decoded,
            DecodedPlayPacket::Immediate(ImmediatePlayPacket::KeepAlive(SKeepAlive { id: 42 }))
        ));
    }

    #[test]
    fn chunk_batch_ack_uses_the_immediate_connection_path() {
        let decoded = decode(RawPacket::new(
            play::S_CHUNK_BATCH_RECEIVED,
            12.5_f32.to_be_bytes().to_vec(),
        ));

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
