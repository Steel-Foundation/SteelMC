//! This module contains the `JavaConnection` struct, which is used to represent a connection to a Java client.
use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use steel_protocol::packet_reader::TCPNetworkDecoder;
use steel_protocol::packet_traits::{ClientPacket, CompressionInfo, EncodedPacket, ServerPacket};
use steel_protocol::packet_writer::TCPNetworkEncoder;
use steel_protocol::packets::common::{
    CDisconnect, CKeepAlive, SClientInformation, SCustomPayload, SKeepAlive,
};
use steel_protocol::packets::game::{
    SAcceptTeleportation, SChat, SChatAck, SChatCommand, SChatSessionUpdate, SChunkBatchReceived,
    SClientTickEnd, SCommandSuggestion, SContainerButtonClick, SContainerClick, SContainerClose,
    SContainerSlotStateChanged, SMovePlayerPos, SMovePlayerPosRot, SMovePlayerRot,
    SMovePlayerStatusOnly, SPickItemFromBlock, SPlayerAction, SPlayerInput, SPlayerLoad,
    SSetCarriedItem, SSetCreativeModeSlot, SSwing, SUseItem, SUseItemOn,
};
use steel_protocol::utils::{ConnectionProtocol, PacketError, RawPacket};
use steel_registry::packets::play;
use steel_utils::locks::{AsyncMutex, SyncMutex};
use steel_utils::{text::TextComponent, translations};
use tokio::io::{BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use crate::command::sender::CommandSender;
use crate::player::Player;
use crate::server::Server;

#[allow(clippy::struct_field_names)]
struct KeepAliveTracker {
    alive_time: u64,
    alive_pending: bool,
    alive_id: u64,
}

/// A connection to a Java client.
pub struct JavaConnection {
    outgoing_packets: UnboundedSender<EncodedPacket>,
    cancel_token: CancellationToken,
    compression: Option<CompressionInfo>,
    network_writer: Arc<AsyncMutex<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>,
    id: u64,

    player: Weak<Player>,
    keep_alive_tracker: SyncMutex<KeepAliveTracker>,
    latency: SyncMutex<u32>,
}

impl JavaConnection {
    /// Creates a new `JavaConnection`.
    pub fn new(
        outgoing_packets: UnboundedSender<EncodedPacket>,
        cancel_token: CancellationToken,
        compression: Option<CompressionInfo>,
        network_writer: Arc<AsyncMutex<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>,
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

    #[allow(clippy::unwrap_used)]
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
    #[allow(clippy::cast_possible_truncation)]
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

    /// Disconnects the client.
    pub fn disconnect(&self, reason: impl Into<TextComponent>) {
        self.send_packet(CDisconnect::new(reason.into()));
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
        if self.outgoing_packets.send(packet).is_err() {
            self.close();
        }
    }

    /// Sends an encoded packet to the client.
    ///
    /// # Panics
    /// - If the packet fails to be sent through the channel.
    pub fn send_encoded_packet(&self, packet: EncodedPacket) {
        if self.outgoing_packets.send(packet).is_err() {
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

    /// Processes a packet from the client.
    #[allow(clippy::too_many_lines)]
    pub fn process_packet(
        self: &Arc<Self>,
        packet: RawPacket,
        player: Arc<Player>,
        server: Arc<Server>,
    ) -> Result<(), PacketError> {
        let data = &mut Cursor::new(packet.payload);

        match packet.id {
            play::S_ACCEPT_TELEPORTATION => {
                player.handle_accept_teleportation(SAcceptTeleportation::read_packet(data)?);
            }
            play::C_CUSTOM_PAYLOAD => {
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
            play::S_PLAYER_LOADED => {
                let _ = SPlayerLoad::read_packet(data)?;
                player.client_loaded.store(true, Ordering::Relaxed);
                // Send initial inventory to client
                player.send_inventory_to_remote();
            }
            play::S_CHAT_COMMAND => {
                server.command_dispatcher.read().handle_command(
                    CommandSender::Player(player),
                    SChatCommand::read_packet(data)?.command,
                    &server,
                );
            }
            play::S_COMMAND_SUGGESTION => {
                let packet = SCommandSuggestion::read_packet(data)?;
                server.command_dispatcher.read().handle_suggestions(
                    &player,
                    packet.id,
                    &packet.command,
                );
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
            id => log::info!("play packet id {id} is not known"),
        }
        Ok(())
    }

    /// Listens for packets from the client.
    pub async fn listener(
        self: Arc<Self>,
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
    /// # Panics
    /// - If the player is not available.
    pub async fn sender(self: Arc<Self>, mut sender_recv: UnboundedReceiver<EncodedPacket>) {
        loop {
            select! {
                () = self.wait_for_close() => {
                    break;
                }
                packet = sender_recv.recv() => {
                    if let Some(packet) = packet {
                        if let Err(err) = self.network_writer.lock().await.write_packet(&packet).await
                        {
                            log::warn!("Failed to send packet to client {}: {err}", self.id);
                            self.close();
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

        let player = self.player.upgrade().expect("Player is not available");
        let world = player.world.clone();
        world.remove_player(player).await;
    }
}
