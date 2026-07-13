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
    CBundleDelimiter, CCommandSuggestions, SAcceptTeleportation, SAttack, SChangeDifficulty,
    SChangeGameMode, SChat, SChatAck, SChatCommand, SChatSessionUpdate, SChunkBatchReceived,
    SClientCommand, SClientTickEnd, SCommandSuggestion, SContainerButtonClick, SContainerClick,
    SContainerClose, SContainerSlotStateChanged, SInteract, SMovePlayerPos, SMovePlayerPosRot,
    SMovePlayerRot, SMovePlayerStatusOnly, SMoveVehicle, SPickItemFromBlock, SPlayerAbilities,
    SPlayerAction, SPlayerCommand, SPlayerInput, SPlayerLoad, SSetCarriedItem,
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

    /// Processes a packet from the client.
    #[expect(
        clippy::too_many_lines,
        reason = "single match dispatch over all play packets; splitting would hurt readability"
    )]
    pub fn process_packet(
        &self,
        packet: RawPacket,
        player: Arc<Player>,
        server: Arc<Server>,
    ) -> Result<(), PacketError> {
        let data = &mut Cursor::new(packet.payload.as_slice());

        if !player.has_joined_world() && !Self::can_process_before_join(packet.id) {
            return Ok(());
        }

        if player.is_domain_switching()
            && !matches!(packet.id, play::S_KEEP_ALIVE | play::S_PING_REQUEST)
        {
            return Ok(());
        }

        match packet.id {
            play::S_ACCEPT_TELEPORTATION => {
                player.handle_accept_teleportation(SAcceptTeleportation::read_packet(data)?);
            }
            play::S_ATTACK => {
                player.handle_attack(SAttack::read_packet(data)?);
            }
            play::S_INTERACT => {
                player.handle_interact(SInteract::read_packet(data)?);
            }
            play::S_CUSTOM_PAYLOAD => {
                player.handle_custom_payload(SCustomPayload::read_packet(data)?);
            }
            play::S_CHAT => {
                player.handle_chat(SChat::read_packet(data)?, Arc::clone(&player));
            }
            play::S_CHAT_SESSION_UPDATE => {
                player.handle_chat_session_update(SChatSessionUpdate::read_packet(data)?);
            }
            play::S_CHAT_ACK => {
                player.handle_chat_ack(SChatAck::read_packet(data)?);
            }
            play::S_CLIENT_INFORMATION => {
                player.handle_client_information(SClientInformation::read_packet(data)?);
            }
            play::S_CLIENT_TICK_END => {
                let _ = SClientTickEnd::read_packet(data)?;
                player.handle_client_tick_end();
            }
            play::S_CHUNK_BATCH_RECEIVED => {
                let packet = SChunkBatchReceived::read_packet(data)?;
                player
                    .chunk_sender
                    .lock()
                    .on_chunk_batch_received_by_client(packet.desired_chunks_per_tick);
            }
            play::S_KEEP_ALIVE => {
                self.handle_keep_alive(SKeepAlive::read_packet(data)?);
            }
            play::S_MOVE_PLAYER_POS => {
                player.handle_move_player(SMovePlayerPos::read_packet(data)?.into());
            }
            play::S_MOVE_PLAYER_POS_ROT => {
                player.handle_move_player(SMovePlayerPosRot::read_packet(data)?.into());
            }
            play::S_MOVE_PLAYER_ROT => {
                player.handle_move_player(SMovePlayerRot::read_packet(data)?.into());
            }
            play::S_MOVE_PLAYER_STATUS_ONLY => {
                player.handle_move_player(SMovePlayerStatusOnly::read_packet(data)?.into());
            }
            play::S_MOVE_VEHICLE => {
                player.handle_move_vehicle(SMoveVehicle::read_packet(data)?);
            }
            play::S_PLAYER_LOADED => {
                let _ = SPlayerLoad::read_packet(data)?;
                if player.mark_client_loaded_from_network() {
                    // Send initial inventory to client
                    player.send_inventory_to_remote();
                }
            }
            play::S_CHAT_COMMAND => {
                let command = SChatCommand::read_packet(data)?.command;
                if server
                    .submit_command(CommandSender::Player(Arc::clone(&player)), command)
                    .is_err()
                {
                    player.send_message(
                        &TextComponent::const_plain("Command queue is full").color(Color::Red),
                    );
                }
                player.detect_command_rate_spam();
            }
            play::S_COMMAND_SUGGESTION => {
                let packet = SCommandSuggestion::read_packet(data)?;
                if server
                    .submit_command_suggestions(Arc::clone(&player), packet.id, packet.command)
                    .is_err()
                {
                    player.send_packet(CCommandSuggestions::new(packet.id, 0, 0, Vec::new()));
                }
            }
            play::S_CONTAINER_BUTTON_CLICK => {
                player.handle_container_button_click(SContainerButtonClick::read_packet(data)?);
            }
            play::S_CONTAINER_CLICK => {
                player.handle_container_click(SContainerClick::read_packet(data)?);
            }
            play::S_CONTAINER_CLOSE => {
                player.handle_container_close(SContainerClose::read_packet(data)?);
            }
            play::S_CONTAINER_SLOT_STATE_CHANGED => {
                player.handle_container_slot_state_changed(
                    SContainerSlotStateChanged::read_packet(data)?,
                );
            }
            play::S_SET_CREATIVE_MODE_SLOT => {
                player.handle_set_creative_mode_slot(SSetCreativeModeSlot::read_packet(data)?);
            }
            play::S_PLAYER_INPUT => {
                player.handle_player_input(SPlayerInput::read_packet(data)?);
            }
            play::S_PLAYER_COMMAND => {
                player.handle_player_command(SPlayerCommand::read_packet(data)?);
            }
            play::S_PLAYER_ABILITIES => {
                player.handle_player_abilities(SPlayerAbilities::read_packet(data)?);
            }
            play::S_USE_ITEM_ON => {
                player.handle_use_item_on(SUseItemOn::read_packet(data)?);
            }
            play::S_USE_ITEM => {
                player.handle_use_item(SUseItem::read_packet(data)?);
            }
            play::S_SET_CARRIED_ITEM => {
                player.handle_set_carried_item(SSetCarriedItem::read_packet(data)?);
            }
            play::S_SWING => {
                let packet = SSwing::read_packet(data)?;
                player.swing(packet.hand, false);
            }
            play::S_PLAYER_ACTION => {
                let packet = SPlayerAction::read_packet(data)?;
                player.handle_player_action(packet);
            }
            play::S_PICK_ITEM_FROM_BLOCK => {
                let packet = SPickItemFromBlock::read_packet(data)?;
                player.handle_pick_item_from_block(packet);
            }
            play::S_SIGN_UPDATE => {
                let packet = SSignUpdate::read_packet(data)?;
                player.handle_sign_update(packet);
            }
            play::S_SPECTATOR_ACTION => {
                let packet = SSpectatorAction::read_packet(data)?;
                player.handle_spectator_action(packet);
            }
            play::S_CLIENT_COMMAND => {
                let packet = SClientCommand::read_packet(data)?;
                player.handle_client_command(packet.action);
            }
            play::S_PING_REQUEST => {
                let packet = SPingRequest::read_packet(data)?;
                player.send_packet(CPongResponse::new(packet.time));
            }
            play::S_CHANGE_GAME_MODE => {
                let packet = SChangeGameMode::read_packet(data)?;
                handle_client_request(&player, &server, packet.gamemode);
            }
            play::S_CHANGE_DIFFICULTY => {
                let packet = SChangeDifficulty::read_packet(data)?;
                player.handle_change_difficulty(packet.difficulty);
            }
            id => log::info!("play packet id {id} is not known"),
        }
        Ok(())
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
                                && let Err(err) = self.process_packet(packet, player, server.clone()) {
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
        let world = player.get_world();
        world.remove_player(player).await;
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
    use super::*;

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
}
