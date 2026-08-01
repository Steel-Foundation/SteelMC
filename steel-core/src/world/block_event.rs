//! Server-side block-event queue and client publication.

use std::collections::VecDeque;
use std::sync::Arc;

use rustc_hash::FxHashSet;
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::CBlockEvent;
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::RegistryEntry;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::{BlockPos, ChunkPos};

use super::World;
use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::Entity as _;
use crate::player::connection::NetworkConnection as _;

#[derive(Clone, Copy, Debug)]
struct BlockEventData {
    pos: BlockPos,
    block: BlockRef,
    param_a: i32,
    param_b: i32,
}

impl BlockEventData {
    fn key(self) -> BlockEventKey {
        BlockEventKey {
            pos: self.pos,
            block_id: self.block.id(),
            param_a: self.param_a,
            param_b: self.param_b,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct BlockEventKey {
    pos: BlockPos,
    block_id: usize,
    param_a: i32,
    param_b: i32,
}

/// Insertion-ordered set matching Vanilla's `ObjectLinkedOpenHashSet<BlockEventData>`.
#[derive(Default)]
pub(super) struct BlockEventQueue {
    events: VecDeque<BlockEventData>,
    members: FxHashSet<BlockEventKey>,
}

impl BlockEventQueue {
    fn push(&mut self, event: BlockEventData) -> bool {
        if !self.members.insert(event.key()) {
            return false;
        }
        self.events.push_back(event);
        true
    }

    fn pop_front(&mut self) -> Option<BlockEventData> {
        let event = self.events.pop_front()?;
        let removed = self.members.remove(&event.key());
        debug_assert!(removed, "queued block event must have a membership entry");
        Some(event)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.events.len()
    }
}

impl World {
    /// Queues a server block event for the next eligible block-event pass.
    ///
    /// Exact duplicates are suppressed while queued. Parameters remain `i32`
    /// for Vanilla behavior parity and are truncated to bytes only for the
    /// client packet.
    pub fn block_event(&self, pos: BlockPos, block: BlockRef, param_a: i32, param_b: i32) {
        self.block_events.lock().push(BlockEventData {
            pos,
            block,
            param_a,
            param_b,
        });
    }

    /// Runs queued block events in Vanilla insertion order.
    ///
    /// Vanilla gates this queue only on simulation range and may synchronously
    /// load chunks from a piston callback. Steel additionally requires the
    /// confirmed radius-1 Full neighborhood so the game tick never waits for
    /// chunk I/O. Deferred events retain their Vanilla retry behavior.
    pub(crate) fn run_block_events(self: &Arc<Self>) {
        let mut deferred = Vec::new();

        loop {
            let event = self.block_events.lock().pop_front();
            let Some(event) = event else {
                break;
            };

            let chunk_pos = ChunkPos::from_block_pos(event.pos);
            if !self
                .chunk_map
                .is_block_ticking_full_chunk_simulated(chunk_pos)
            {
                deferred.push(event);
                continue;
            }

            if self.do_block_event(event) {
                self.broadcast_block_event(event);
            }
        }

        if deferred.is_empty() {
            return;
        }
        let mut queue = self.block_events.lock();
        for event in deferred {
            queue.push(event);
        }
    }

    fn do_block_event(self: &Arc<Self>, event: BlockEventData) -> bool {
        let state = self.get_block_state(event.pos);
        if state.get_block().id() != event.block.id() {
            return false;
        }

        BLOCK_BEHAVIORS.get_behavior(event.block).trigger_event(
            state,
            self,
            event.pos,
            event.param_a,
            event.param_b,
        )
    }

    fn broadcast_block_event(&self, event: BlockEventData) {
        let packet = CBlockEvent::new(
            event.pos,
            event.param_a as u8,
            event.param_b as u8,
            event.block.id() as i32,
        );
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            tracing::warn!(
                pos = ?event.pos,
                block = %event.block.key,
                "Failed to encode block event packet"
            );
            return;
        };

        self.players.iter_players(|_, player| {
            if Self::recipient_within_64_blocks(player.position(), event.pos) {
                player.connection.send_encoded(encoded.clone());
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::Downcast as _;
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::entities::EndGatewayBlockEntity;
    use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn ordered_queue_suppresses_only_exact_duplicates() {
        init_test_registry();
        let mut queue = BlockEventQueue::default();
        let first = BlockEventData {
            pos: BlockPos::new(1, 2, 3),
            block: &vanilla_blocks::STONE,
            param_a: 4,
            param_b: 5,
        };
        let second = BlockEventData {
            param_b: 6,
            ..first
        };

        assert!(queue.push(first));
        assert!(!queue.push(first));
        assert!(queue.push(second));
        assert_eq!(queue.len(), 2);

        let popped_first = queue.pop_front().expect("the first event should be queued");
        assert!(
            queue.push(first),
            "a callback may requeue the event after it was popped"
        );
        let popped_second = queue
            .pop_front()
            .expect("the second event should be queued");
        let requeued_first = queue
            .pop_front()
            .expect("the callback event should be appended");
        assert_eq!(popped_first.key(), first.key());
        assert_eq!(popped_second.key(), second.key());
        assert_eq!(requeued_first.key(), first.key());
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn server_queue_defers_then_dispatches_the_current_block_event() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("server_block_event_queue");
        let pos = BlockPos::new(1, 64, 1);
        let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::END_GATEWAY.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let gateway = world
            .get_block_entity(pos)
            .expect("placing an end gateway should create its block entity");
        assert!(
            !gateway
                .downcast_ref::<EndGatewayBlockEntity>()
                .expect("the gateway should have its concrete block entity")
                .is_cooling_down()
        );

        world.block_event(pos, &vanilla_blocks::STONE, 1, 0);
        world.run_block_events();
        assert_eq!(world.block_events.lock().len(), 0);
        assert!(
            !gateway
                .downcast_ref::<EndGatewayBlockEntity>()
                .expect("the gateway should remain concrete")
                .is_cooling_down()
        );

        world.block_event(pos, &vanilla_blocks::END_GATEWAY, 2, 0);
        world.run_block_events();
        assert_eq!(world.block_events.lock().len(), 0);
        assert!(
            !gateway
                .downcast_ref::<EndGatewayBlockEntity>()
                .expect("the gateway should remain concrete")
                .is_cooling_down()
        );

        holder.set_simulation_level(None);
        world.block_event(pos, &vanilla_blocks::END_GATEWAY, 1, 0);
        world.block_event(pos, &vanilla_blocks::END_GATEWAY, 1, 0);
        assert_eq!(world.block_events.lock().len(), 1);
        world.run_block_events();
        assert_eq!(world.block_events.lock().len(), 1);
        assert!(
            !gateway
                .downcast_ref::<EndGatewayBlockEntity>()
                .expect("the gateway should remain concrete")
                .is_cooling_down()
        );

        holder.set_simulation_level(Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK));
        world.run_block_events();

        assert_eq!(world.block_events.lock().len(), 0);
        assert!(
            gateway
                .downcast_ref::<EndGatewayBlockEntity>()
                .expect("the gateway should remain concrete")
                .is_cooling_down()
        );
    }
}
