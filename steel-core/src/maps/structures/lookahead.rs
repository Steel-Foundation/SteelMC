use std::{io, sync::Arc};

use rustc_hash::FxHashMap;
use steel_utils::ChunkPos;

use crate::{
    chunk::{
        chunk_request::{ChunkRequestHandle, ChunkTicketKind},
        status::ChunkStatus,
    },
    maps::chunk_io::MapChunk,
    world::World,
    worldgen::{
        generator::ChunkGenerator,
        structure::{StructureLocateCandidate, StructureLocatePlan},
    },
};

/// Prepares a bounded window through the normal generator. Selection and claims
/// remain sequential. Approved divergence: unused generated chunks may be saved.
pub(super) struct MapStructureLookahead {
    limit: usize,
    pending: FxHashMap<ChunkPos, ChunkRequestHandle>,
}

impl MapStructureLookahead {
    pub(super) fn new(limit: usize) -> io::Result<Self> {
        if limit == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "map lookahead must be nonzero",
            ));
        }
        Ok(Self {
            limit,
            pending: FxHashMap::default(),
        })
    }

    fn prepare(&mut self, world: &Arc<World>, positions: impl IntoIterator<Item = ChunkPos>) {
        for pos in positions {
            if self.pending.len() == self.limit {
                break;
            }
            self.pending.entry(pos).or_insert_with(|| {
                world.chunk_map.request_chunk(
                    pos,
                    ChunkStatus::StructureStarts,
                    ChunkTicketKind::Map,
                )
            });
        }
    }

    pub(super) fn discard(&mut self, pos: ChunkPos) {
        drop(self.pending.remove(&pos));
    }

    pub(super) async fn load(
        &mut self,
        world: &Arc<World>,
        candidate: StructureLocateCandidate,
        remaining: &[StructureLocateCandidate],
        plan: &StructureLocatePlan,
    ) -> io::Result<MapChunk> {
        if let Some(chunk) =
            MapChunk::get_loaded(world, candidate.chunk_pos, ChunkStatus::StructureStarts)
        {
            self.discard(candidate.chunk_pos);
            return Ok(chunk);
        }
        let generator = world
            .chunk_map
            .world_gen_context
            .generator
            .structure_generator()
            .ok_or_else(|| io::Error::other("structure search lost its generator"))?;
        // The current candidate is mandatory, including saved-data recovery.
        // Speculate only where placement frequency and exclusion checks pass.
        self.prepare(
            world,
            remaining
                .iter()
                .filter(|next| {
                    next.chunk_pos == candidate.chunk_pos
                        || generator.locate_candidate_is_structure_chunk(plan, **next) == Some(true)
                })
                .map(|next| next.chunk_pos),
        );
        let request = self
            .pending
            .remove(&candidate.chunk_pos)
            .ok_or_else(|| io::Error::other("map lookahead lost its current candidate"))?;
        // Each request is independent: a later, slower candidate cannot delay this one.
        MapChunk::from_request(request).await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    use super::*;
    use crate::{
        bootstrap::init_globals,
        chunk::{
            Chunk,
            chunk_holder::ChunkHolder,
            chunk_request::ChunkRequestState,
            chunk_ticket_manager::ticket_level_for_status,
            section::{ChunkSection, Sections},
        },
        test_support::fresh_test_world,
    };
    use futures::FutureExt;

    #[test]
    fn lookahead_bounds_distinct_requests_and_releases_unused_tickets() {
        init_globals();
        let fixture = fresh_test_world("map_lookahead_bound");
        let world = &fixture.world;
        world.chunk_map.stop_generation_refill_loop();
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(0, 0),
            ChunkPos::new(1, 0),
            ChunkPos::new(2, 0),
            ChunkPos::new(3, 0),
        ];
        let mut lookahead = MapStructureLookahead::new(3).expect("window");
        lookahead.prepare(world, positions);
        assert_eq!(lookahead.pending.len(), 3);
        assert!(!lookahead.pending.contains_key(&positions[4]));
        world.chunk_map.advance_scheduling();
        for pos in &positions[..4] {
            let holder = world
                .chunk_map
                .chunks
                .read_sync(pos, |_, holder| Arc::clone(holder))
                .expect("requested holder");
            assert!(!holder.is_status_disallowed(ChunkStatus::StructureStarts));
            assert!(holder.is_status_disallowed(ChunkStatus::Biomes));
        }
        drop(lookahead);
        world.chunk_map.advance_scheduling();
        for pos in positions {
            assert!(
                world
                    .chunk_map
                    .chunks
                    .read_sync(&pos, |_, holder| holder
                        .is_status_disallowed(ChunkStatus::StructureStarts))
                    .unwrap_or(true)
            );
        }
        world.chunk_map.stop_generation_refill_loop();
    }

    #[test]
    fn current_candidate_does_not_wait_for_later_chunk_requests() {
        init_globals();
        let fixture = fresh_test_world("map_lookahead_ready");
        let world = &fixture.world;
        world.chunk_map.stop_generation_refill_loop();
        let current = ChunkPos::new(0, 0);
        let sections = (0..world.get_height() / 16)
            .map(|_| ChunkSection::new_empty())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let holder = Arc::new(ChunkHolder::new(
            current,
            ticket_level_for_status(ChunkStatus::StructureStarts),
            None,
            world.get_min_y(),
            world.get_height(),
        ));
        holder.insert_chunk(
            Chunk::new(
                Sections::from_owned(sections),
                current,
                world.get_min_y(),
                world.get_height(),
                Arc::downgrade(world),
            ),
            ChunkStatus::StructureStarts,
        );
        assert!(world.chunk_map.chunks.insert_sync(current, holder).is_ok());
        let later = ChunkPos::new(1, 0);
        let mut lookahead = MapStructureLookahead::new(2).expect("window");
        lookahead.prepare(world, [current, later]);
        world.chunk_map.advance_scheduling();
        assert!(matches!(
            lookahead.pending[&later].poll(),
            ChunkRequestState::Pending { .. }
        ));
        let request = lookahead.pending.remove(&current).expect("current request");
        let loaded = MapChunk::from_request(request)
            .now_or_never()
            .expect("current readiness is independent")
            .expect("current chunk");
        assert_eq!(loaded.holder.get_pos(), current);
        drop(loaded);
        drop(lookahead);
        world.chunk_map.advance_scheduling();
        world.chunk_map.stop_generation_refill_loop();
    }

    #[test]
    fn cancelling_a_search_releases_the_transferred_current_request_and_lookahead() {
        init_globals();
        let fixture = fresh_test_world("map_lookahead_cancel");
        let world = &fixture.world;
        let current = ChunkPos::new(0, 0);
        let mut lookahead = MapStructureLookahead::new(2).expect("window");
        lookahead.prepare(world, [current, ChunkPos::new(1, 0)]);
        {
            let request = lookahead.pending.remove(&current).expect("current request");
            let mut future = pin!(MapChunk::from_request(request));
            assert!(matches!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
        }
        drop(lookahead);
        world.chunk_map.advance_scheduling();
        assert_eq!(world.chunk_map.chunks.len(), 0);
        world.chunk_map.stop_generation_refill_loop();
    }
}
