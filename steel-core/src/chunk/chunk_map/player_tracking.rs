use super::{CSetChunkCenter, ChunkMap, ChunkPos, ChunkTicket, Entity, Player, PlayerChunkView};
use crate::chunk::chunk_scheduler::ChunkTicketOperation;
use crate::player::chunk_sender::ChunkSender;

impl ChunkMap {
    /// Updates the player's status in the chunk map.
    pub fn update_player_status(&self, player: &Player) {
        let current_chunk_pos = ChunkPos::from_entity_pos(player.position());
        *player.last_chunk_pos.lock() = current_chunk_pos;
        let view_distance = player.view_distance();

        let new_view = PlayerChunkView::new(current_chunk_pos, view_distance);
        let world = self.world_gen_context.world();
        let player_load_level = ChunkTicket::player_loading(world.view_distance).load_level();
        let player_id = player.gameprofile.id;
        let mut last_view_guard = player.last_tracking_view.lock();

        if last_view_guard.as_ref() != Some(&new_view) {
            if let Some(last_view) = last_view_guard.as_ref() {
                if last_view.center != new_view.center {
                    let _ = self.scheduling.queue_ticket_operations([
                        ChunkTicketOperation::RemovePlayer {
                            pos: last_view.center,
                            player_id,
                        },
                        ChunkTicketOperation::AddPlayer {
                            pos: new_view.center,
                            player_id,
                            load_level: player_load_level,
                        },
                    ]);

                    player.send_packet(CSetChunkCenter {
                        x: new_view.center.0.x,
                        y: new_view.center.0.y,
                    });
                }

                // Track chunks for PlayerAreaMap update
                let mut added_chunks = Vec::new();
                let mut removed_chunks = Vec::new();

                // We lock here to ensure we have unique access for the duration of the diff
                let mut chunk_sender = player.chunk_sender.lock();
                let connection = &*player.connection;
                PlayerChunkView::difference(
                    last_view,
                    &new_view,
                    |pos, ctx: &mut (&mut _, &mut Vec<_>, &mut Vec<_>)| {
                        ctx.0.mark_chunk_pending_to_send(pos);
                        ctx.1.push(pos);
                    },
                    |pos, ctx: &mut (&mut _, &mut Vec<_>, &mut Vec<_>)| {
                        ctx.0.drop_chunk(connection, pos);
                        ctx.2.push(pos);
                    },
                    &mut (&mut chunk_sender, &mut added_chunks, &mut removed_chunks),
                );
                drop(chunk_sender);

                // Update the player area map with the diff
                world.player_area_map.on_player_view_change(
                    player.id(),
                    &added_chunks,
                    &removed_chunks,
                );
            } else {
                self.scheduling
                    .queue_ticket_operation(ChunkTicketOperation::AddPlayer {
                        pos: new_view.center,
                        player_id,
                        load_level: player_load_level,
                    });

                // Send initial chunk cache center to client
                player.send_packet(CSetChunkCenter {
                    x: new_view.center.0.x,
                    y: new_view.center.0.y,
                });

                let mut chunk_sender = player.chunk_sender.lock();
                new_view.for_each(|pos| {
                    chunk_sender.mark_chunk_pending_to_send(pos);
                });
                drop(chunk_sender);

                // First time - add all chunks in view to player area map
                world.player_area_map.on_player_join(player, &new_view);
            }

            *last_view_guard = Some(new_view);
        }
        drop(last_view_guard);

        // Entity visibility also depends on exact player position, not only
        // chunk-view changes. Vanilla refreshes tracked entities for accepted
        // movement within the same chunk as well.
        let sent_chunks = player.chunk_sender.lock().sent_chunks_snapshot();
        world
            .entity_tracker()
            .update_player(player, &new_view, |chunk| sent_chunks.contains(&chunk));
    }

    /// Removes a player from the chunk map.
    pub fn remove_player(&self, player: &Player) {
        // Keep the view guard through ticket submission so a concurrent tracking
        // update cannot re-add the player before this removal is ordered.
        let mut last_view = player.last_tracking_view.lock();
        let removed_view = last_view.take();
        {
            let mut chunk_sender = player.chunk_sender.lock();
            let mut chunk_send_epoch = player.chunk_send_epoch.lock();
            *chunk_send_epoch = chunk_send_epoch.wrapping_add(1);
            *chunk_sender = ChunkSender::default();
            *player.last_chunk_pos.lock() = ChunkPos::new(i32::MAX, i32::MAX);
        }

        if let Some(removed_view) = removed_view {
            let player_id = player.gameprofile.id;
            self.scheduling
                .queue_ticket_operation(ChunkTicketOperation::RemovePlayer {
                    pos: removed_view.center,
                    player_id,
                });
        }
    }
}
