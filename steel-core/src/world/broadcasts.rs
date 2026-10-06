use super::{
    Arc, CPlayerChat, CSystemChat, ChunkPos, ClientPacket, ConnectionProtocol, EncodedPacket,
    Entity, EntityMovementSyncPacket, LastSeen, NetworkConnection, Player, PlayerChunkView, World,
};

impl World {
    /// Broadcasts a signed chat message to all players in the world.
    ///
    /// # Panics
    /// Panics if `message_signature` is `None` after checking `is_some()` (should never happen).
    pub fn broadcast_chat(
        &self,
        mut packet: CPlayerChat,
        _sender: Arc<Player>,
        sender_last_seen: LastSeen,
        message_signature: Option<&[u8; 256]>,
    ) {
        log::debug!(
            "broadcast_chat: sender_last_seen has {} signatures, message_signature present: {}",
            sender_last_seen.len(),
            message_signature.is_some()
        );

        self.players.iter_players(|_, recipient| {
            let messages_received = recipient.get_and_increment_messages_received();
            packet.global_index = messages_received;

            log::debug!(
                "Broadcasting to player {} (UUID: {}), global_index={}",
                recipient.gameprofile.name,
                recipient.gameprofile.id,
                messages_received
            );

            // IMPORTANT: Index previous messages BEFORE updating the cache
            // This matches vanilla's order: pack() then push()
            let previous_messages = {
                let chat = recipient.chat.lock();
                chat.signature_cache
                    .index_previous_messages(&sender_last_seen)
            };

            log::debug!(
                "  Indexed {} previous messages for recipient",
                previous_messages.len()
            );

            packet.previous_messages.clone_from(&previous_messages);

            // Send the packet
            recipient.send_packet(packet.clone());

            // AFTER sending, update the recipient's cache using vanilla's push algorithm
            // This adds all lastSeen signatures + current signature to the cache
            {
                let mut chat = recipient.chat.lock();
                if let Some(signature) = message_signature {
                    chat.signature_cache
                        .push(&sender_last_seen, Some(signature));

                    log::debug!("  Added signature to recipient's cache and pending list");

                    // Add to pending messages for acknowledgment tracking
                    chat.message_validator
                        .add_pending(Some(Box::new(*signature) as Box<[u8]>));
                } else {
                    // Even unsigned messages update the pending tracker
                    chat.message_validator.add_pending(None);
                    log::debug!("  Added unsigned message to pending list");
                }
            }

            true
        });
    }

    /// Broadcasts a system chat message to all players.
    pub fn broadcast_system_chat(&self, packet: CSystemChat) {
        self.broadcast_to_all(packet);
    }

    /// Broadcasts a packet to all players in the world.
    pub fn broadcast_to_all<P: ClientPacket>(&self, packet: P) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_all_encoded(encoded);
    }

    /// Broadcasts a packet to all players in the world except one (identified by entity ID).
    pub fn broadcast_to_all_except<P: ClientPacket>(&self, packet: P, exclude: i32) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_all_encoded_except(encoded, exclude);
    }

    /// Broadcasts a packet to all players in the world.
    ///
    /// This method handles encoding the packets produced from the function passed.
    pub fn broadcast_to_all_with<P: ClientPacket, F: Fn(&Player) -> P>(&self, packet: F) {
        self.players.iter_players(|_, player| {
            let Ok(encoded) = EncodedPacket::from_bare(
                packet(player),
                self.compression,
                ConnectionProtocol::Play,
            ) else {
                return false;
            };
            player.connection.send_encoded(encoded);
            true
        });
    }

    /// Broadcasts an already-encoded packet to all players in the world.
    pub fn broadcast_to_all_encoded(&self, packet: EncodedPacket) {
        self.players.iter_players(|_, player| {
            player.connection.send_encoded(packet.clone());
            true
        });
    }

    /// Broadcasts an already-encoded packet to all players except one.
    pub fn broadcast_to_all_encoded_except(&self, packet: EncodedPacket, exclude: i32) {
        self.players.iter_players(|_, player| {
            if player.id() != exclude {
                player.connection.send_encoded(packet.clone());
            }
            true
        });
    }

    /// Broadcasts an unsigned player chat message to all players.
    pub fn broadcast_unsigned_chat(&self, mut packet: CPlayerChat) {
        self.players.iter_players(|_, recipient| {
            let messages_received = recipient.get_and_increment_messages_received();
            packet.global_index = messages_received;

            recipient.send_packet(packet.clone());
            true
        });
    }

    /// Broadcasts a packet to all players tracking the given chunk.
    ///
    /// This method handles encoding the packet internally, avoiding boilerplate at call sites.
    /// If encoding fails, the broadcast is silently skipped.
    pub fn broadcast_to_nearby<P: ClientPacket>(
        &self,
        chunk: ChunkPos,
        packet: P,
        exclude: Option<i32>,
    ) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_nearby_encoded(chunk, encoded, exclude);
    }

    /// Broadcasts an already-encoded packet to all players tracking the given chunk.
    ///
    /// Use this when you have a pre-encoded packet to avoid re-encoding.
    pub fn broadcast_to_nearby_encoded(
        &self,
        chunk: ChunkPos,
        packet: EncodedPacket,
        exclude: Option<i32>,
    ) {
        let tracking_players = self.get_packet_tracking_players(chunk);
        for entity_id in tracking_players {
            if Some(entity_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                player.connection.send_encoded(packet.clone());
            }
        }
    }

    /// Returns players whose view includes the chunk and whose client has the base chunk packet.
    pub fn get_packet_tracking_players(&self, chunk: ChunkPos) -> Vec<i32> {
        self.player_area_map
            .get_tracking_players(chunk)
            .into_iter()
            .filter(|entity_id| {
                self.players
                    .get_by_entity_id(*entity_id)
                    .is_some_and(|player| player.chunk_sender.lock().is_chunk_sent(chunk))
            })
            .collect()
    }

    /// Returns players on the tracked border of a chunk whose client has its base chunk packet.
    pub fn get_light_packet_tracking_players(&self, chunk: ChunkPos) -> Vec<i32> {
        self.player_area_map
            .get_tracking_players(chunk)
            .into_iter()
            .filter(|entity_id| {
                let Some(player) = self.players.get_by_entity_id(*entity_id) else {
                    return false;
                };
                let Some(view) = *player.last_tracking_view.lock() else {
                    return false;
                };
                let chunk_sender = player.chunk_sender.lock();
                let is_chunk_sent = |pos| chunk_sender.is_chunk_sent(pos);
                Self::chunk_is_on_packet_tracked_border(view, chunk, &is_chunk_sent)
            })
            .collect()
    }

    pub(super) fn chunk_is_on_packet_tracked_border(
        view: PlayerChunkView,
        chunk: ChunkPos,
        is_chunk_sent: &impl Fn(ChunkPos) -> bool,
    ) -> bool {
        if !Self::chunk_is_packet_tracked(view, chunk, is_chunk_sent) {
            return false;
        }

        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }

                let neighbor = ChunkPos::new(chunk.0.x + dx, chunk.0.y + dz);
                if !Self::chunk_is_packet_tracked(view, neighbor, is_chunk_sent) {
                    return true;
                }
            }
        }

        false
    }

    pub(super) fn chunk_is_packet_tracked(
        view: PlayerChunkView,
        chunk: ChunkPos,
        is_chunk_sent: &impl Fn(ChunkPos) -> bool,
    ) -> bool {
        view.contains(chunk) && is_chunk_sent(chunk)
    }

    /// Broadcasts a packet to players currently tracking an entity.
    pub fn broadcast_to_entity_trackers<P: ClientPacket>(
        &self,
        entity_id: i32,
        packet: P,
        exclude: Option<i32>,
    ) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_entity_trackers_encoded(entity_id, encoded, exclude);
    }

    /// Broadcasts a packet to players tracking an entity, excluding several players.
    pub fn broadcast_to_entity_trackers_except_many<P: ClientPacket>(
        &self,
        entity_id: i32,
        packet: P,
        excluded_player_ids: &[i32],
    ) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };

        for player_id in self.entity_tracker.tracking_player_ids(entity_id) {
            if excluded_player_ids.contains(&player_id) {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(player_id) {
                player.connection.send_encoded(encoded.clone());
            }
        }
    }

    /// Broadcasts an entity movement sync packet to players currently tracking an entity.
    pub fn broadcast_movement_sync_to_entity_trackers(
        &self,
        entity_id: i32,
        packet: EntityMovementSyncPacket,
        exclude: Option<i32>,
    ) {
        let Some(encoded) = self.encode_movement_sync_packet(packet) else {
            return;
        };
        self.broadcast_to_entity_trackers_encoded(entity_id, encoded, exclude);
    }

    /// Broadcasts an already-encoded packet to players currently tracking an entity.
    pub fn broadcast_to_entity_trackers_encoded(
        &self,
        entity_id: i32,
        packet: EncodedPacket,
        exclude: Option<i32>,
    ) {
        for player_id in self.entity_tracker.tracking_player_ids(entity_id) {
            if Some(player_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(player_id) {
                player.connection.send_encoded(packet.clone());
            }
        }
    }

    pub(super) fn encode_movement_sync_packet(
        &self,
        packet: EntityMovementSyncPacket,
    ) -> Option<EncodedPacket> {
        let encoded = match packet {
            EntityMovementSyncPacket::Position(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::PositionRotation(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::Rotation(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::HeadRotation(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::PositionSync(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::Velocity(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
        };
        encoded.ok()
    }
}
