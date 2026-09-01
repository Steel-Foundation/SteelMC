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
pub(crate) mod chunk_ticket;
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
mod player_ticket_tracker;
mod simulation_ticket_manager;

pub mod full_chunk;
pub mod paletted_container;
pub mod section;
pub mod status;

pub use data::Chunk;
