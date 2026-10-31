//! World preparation owns generation and preload leases until tick-stage publication.
use super::{Server, TemporaryWorldRequest};
use crate::{
    chunk::{
        chunk_request::{ChunkRequestHandle, ChunkTicketKind},
        status::ChunkStatus,
    },
    config::WorldStorageConfig,
    level_data::{GameTimeSource, RespawnData, WorldGenerationSettings},
    world::{World, WorldConfig},
    worldgen::generator::registry::ValidatedWorldGeneratorConfig,
};
use std::sync::Arc;
use steel_utils::ChunkPos;
use tokio_util::task::TaskTracker;
pub(super) struct PreparedWorld {
    pub(super) world: Arc<World>,
    pub(super) published: bool,
    pub(super) spawn_ticket: Option<ChunkRequestHandle>,
    cleanup: TaskTracker,
}
impl Drop for PreparedWorld {
    fn drop(&mut self) {
        if !self.published {
            self.world.chunk_map.stop_generation_refill_loop();
            self.world.chunk_map.task_tracker.close();
            let world = Arc::clone(&self.world);
            self.cleanup.spawn_on(
                async move {
                    world.chunk_map.task_tracker.wait().await;
                },
                self.world.chunk_map.chunk_runtime.handle(),
            );
        }
    }
}
impl PreparedWorld {
    pub(super) async fn prepare(
        server: Arc<Server>,
        source: Arc<World>,
        request: TemporaryWorldRequest,
        validated: ValidatedWorldGeneratorConfig,
        game_time_source: GameTimeSource,
    ) -> Result<Self, String> {
        let output = server.world_generators.create(
            None,
            &validated,
            request.seed,
            Arc::clone(&source.chunk_map.generation_pool),
        )?;
        let generation_settings = WorldGenerationSettings::from_generator_config(
            request.generator.clone(),
            &output.config,
            output.dimension_type.key.clone(),
            output.dimension_type.min_y,
            output.dimension_type.height,
        );
        let world = World::new_with_config_and_encoding_pool(
            Arc::clone(&source.chunk_map.chunk_runtime),
            request.key.clone(),
            output.dimension_type,
            request.seed,
            WorldConfig {
                damage_history: Arc::clone(&server.damage_history),
                game_time_source,
                storage: WorldStorageConfig::RamOnly,
                level_data_path: None,
                generator: Arc::new(output.generator),
                generation_settings,
                view_distance: server.config.view_distance,
                simulation_distance: server.config.simulation_distance,
                max_chained_neighbor_updates: server.config.max_chained_neighbor_updates,
                compression: server.config.compression,
                is_flat: output.is_flat,
                sea_level: output.sea_level,
                default_gamemode: request.default_gamemode,
                difficulty: request.difficulty,
            },
            Arc::clone(&source.chunk_map.generation_pool),
            Arc::clone(&server.chunk_encoding_pool),
        )
        .await
        .map_err(|e| e.to_string())?;
        let center = ChunkPos::from_block_pos(request.spawn);
        let ticket =
            world
                .chunk_map
                .request_square(center, 1, ChunkStatus::Full, ChunkTicketKind::Pregen);
        let prepared = PreparedWorld {
            world,
            published: false,
            spawn_ticket: Some(ticket),
            cleanup: server.temporary_worlds.cleanup.clone(),
        };
        let world = &prepared.world;
        world
            .extensions
            .set(Arc::clone(&server.extensions))
            .map_err(|_| "world extensions already set")?;
        {
            let mut data = world.level_data.write();
            data.data_mut().set_spawn_pos(request.spawn);
            data.data_mut().set_respawn_data(RespawnData::of(
                request.key.clone(),
                request.spawn,
                0.0,
                0.0,
            ));
            data.data_mut().initialized = true;
        }
        // Prepare the authored spawn before anyone can enter the world.
        let chunk = ChunkPos::from_block_pos(request.spawn);
        world
            .chunk_map
            .with_full_chunks_in_radius(chunk, 1, || ())
            .await
            .ok_or("failed to prepare temporary spawn chunks")?;
        Ok(prepared)
    }
}
