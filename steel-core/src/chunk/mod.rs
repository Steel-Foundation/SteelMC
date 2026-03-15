//! This module contains all the chunk related structures and logic.

/// Noise-based aquifer for underground fluid placement.
pub mod aquifer;
/// Terrain density modification around structure pieces.
pub mod beardifier;
pub mod chunk_access;
pub mod chunk_generation_task;
pub mod chunk_generator;
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

/// Generates an empty world which can be used for testing
pub mod empty_chunk_generator;
/// Generates flat worlds with configurable layers.
pub mod flat_chunk_generator;
pub mod level_chunk;
/// Noise-based terrain generation with cell-level interpolation.
pub mod noise_chunk;
/// Ore vein generation during terrain fill.
pub mod ore_veinifier;
pub mod paletted_container;
pub mod proto_chunk;
pub mod section;
/// Surface system for biome-specific block replacement.
pub mod surface_system;
/// Generates vanilla worlds using noise-based biomes and terrain.
pub mod vanilla_generator;
pub mod world_gen_context;
