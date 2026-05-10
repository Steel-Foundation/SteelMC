//! This module contains all the chunk related structures and logic.

pub mod chunk_access;
pub mod chunk_generation_task;
pub mod chunk_holder;
/// The chunk map manages chunk loading, generation, and lifecycle.
pub mod chunk_map;
pub mod chunk_pyramid;
pub mod chunk_status_tasks;
/// Tracks chunk levels based on ticket propagation.
pub mod chunk_ticket_manager;
pub mod heightmap;
/// Tracks the chunks that are visible to a player.
pub mod player_chunk_view;

pub mod level_chunk;
pub mod paletted_container;
pub mod proto_chunk;
pub mod section;
