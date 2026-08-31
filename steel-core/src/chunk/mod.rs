//! This module contains all the chunk related structures and logic.

mod block_entity_listener;
pub mod chunk_generation_task;
pub mod chunk_holder;
/// The chunk map manages chunk loading, generation, and lifecycle.
pub mod chunk_map;
pub mod chunk_pyramid;
pub mod chunk_request;
mod chunk_scheduler;
pub mod chunk_status_tasks;
/// Tracks chunk levels based on ticket propagation.
pub mod chunk_ticket_manager;
pub(crate) mod chunk_ticket_storage;
mod data;
/// Tracks Full-neighborhood readiness for ticking chunk lifecycles.
pub(crate) mod full_chunk_readiness;
pub(crate) mod gameplay_chunk_lookup_cache;
pub mod heightmap;
pub mod light;
/// Tracks the chunks that are visible to a player.
pub mod player_chunk_view;
mod simulation_ticket_manager;

/// Internal chunk simulation APIs exposed only to benchmark targets.
#[cfg(feature = "benchmark-support")]
#[doc(hidden)]
pub mod simulation_benchmark_support {
    use rustc_hash::FxHashSet;
    use steel_utils::ChunkPos;
    use uuid::Uuid;

    use super::{
        chunk_ticket_manager::{ChunkTicket, ChunkTicketLevel},
        chunk_ticket_storage::{
            ChunkTicketOperation, ChunkTicketStorage, SourceLevelUpdate, SourceProjectionChanges,
        },
        simulation_ticket_manager::SimulationTicketManager,
    };

    /// Full storage-to-propagation pipeline used by simulation ticket benchmarks.
    pub struct SimulationTicketBenchmarkScenario {
        storage: ChunkTicketStorage,
        manager: SimulationTicketManager,
        player_load_level: ChunkTicketLevel,
    }

    impl SimulationTicketBenchmarkScenario {
        /// Seeds player sources and propagates the initial simulation levels.
        #[must_use]
        pub fn new(
            simulation_distance: u8,
            players: impl IntoIterator<Item = (Uuid, ChunkPos)>,
        ) -> Self {
            let player_load_level = ChunkTicket::player_loading(simulation_distance).load_level();
            let mut storage = ChunkTicketStorage::new(simulation_distance);
            let changes = storage.apply_operations(players.into_iter().map(|(player_id, pos)| {
                ChunkTicketOperation::AddPlayer {
                    pos,
                    player_id,
                    load_level: player_load_level,
                }
            }));
            let updates = materialize_simulation_updates(&storage, changes);

            let mut manager = SimulationTicketManager::new();
            manager.apply_source_updates(updates);
            manager.run_all_updates();

            Self {
                storage,
                manager,
                player_load_level,
            }
        }

        /// Moves one player through storage, projection, and propagation.
        #[must_use]
        pub fn move_player(
            &mut self,
            player_id: Uuid,
            old_pos: ChunkPos,
            new_pos: ChunkPos,
        ) -> usize {
            let changes = self.storage.apply_operations([
                ChunkTicketOperation::RemovePlayer {
                    pos: old_pos,
                    player_id,
                },
                ChunkTicketOperation::AddPlayer {
                    pos: new_pos,
                    player_id,
                    load_level: self.player_load_level,
                },
            ]);
            let updates = materialize_simulation_updates(&self.storage, changes);
            self.manager.apply_source_updates(updates);
            self.manager.run_all_updates().len()
        }
    }

    fn materialize_simulation_updates(
        storage: &ChunkTicketStorage,
        changes: SourceProjectionChanges,
    ) -> Vec<SourceLevelUpdate> {
        let mut positions: Vec<_> = changes
            .simulation_positions
            .into_iter()
            .collect::<FxHashSet<_>>()
            .into_iter()
            .collect();
        positions.sort_unstable_by_key(|pos| (pos.0.x, pos.0.y));
        positions
            .into_iter()
            .map(|pos| storage.simulation_source_update(pos))
            .collect()
    }
}

pub mod full_chunk;
pub mod paletted_container;
pub mod section;
pub mod status;

pub use data::Chunk;
