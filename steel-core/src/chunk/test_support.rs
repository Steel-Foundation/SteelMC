//! Native chunk fixtures shared by unit tests and external test adapters.
use super::{
    Chunk,
    chunk_holder::{ChunkHolder, TickingReadiness},
    chunk_ticket_manager::ChunkTicketLevel,
    section::{ChunkSection, Sections},
    status::ChunkStatus,
};
use crate::world::World;
use std::{slice, sync::Arc};
use steel_utils::ChunkPos;

/// Installs an empty full chunk with the native visibility and ticking lifecycle.
///
/// # Panics
/// Panics if the fresh chunk cannot transition to block-ticking readiness.
#[must_use]
pub fn insert_full_chunk(
    world: &Arc<World>,
    pos: ChunkPos,
    block_ticking_ready: bool,
) -> Arc<ChunkHolder> {
    let min_y = world.get_min_y();
    let height = world.get_height();
    let sections = (0..height / 16)
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let proto = Chunk::new(
        Sections::from_owned(sections),
        pos,
        min_y,
        height,
        Arc::downgrade(world),
    );
    let _ = proto.promote_to_full();
    let holder = Arc::new(ChunkHolder::new(
        pos,
        ChunkTicketLevel::BLOCK_TICKING_CHUNK,
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
        min_y,
        height,
    ));
    holder.insert_chunk(proto, ChunkStatus::Full);
    if block_ticking_ready {
        assert_eq!(
            holder.transition_ticking_readiness(TickingReadiness::BlockTicking),
            Some(TickingReadiness::Unready)
        );
    }
    let _ = world.chunk_map.chunks.insert_sync(pos, Arc::clone(&holder));
    world.on_entity_chunk_loaded(pos);
    world.update_entity_chunk_visibility(pos, holder.entity_visibility());
    world
        .chunk_map
        .activate_block_entities(slice::from_ref(&holder));
    world.chunk_map.rebuild_ticking_chunk_snapshot();
    holder
}
