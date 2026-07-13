//! # Steel Core
//!
//! The core library for the Steel Minecraft server. Handles everything related to the PLAY state.

use crate::chunk::chunk_map::ChunkMap;

pub mod behavior;
pub mod block_entity;
pub mod chunk;
pub mod chunk_saver;
pub mod command;
pub mod config;
pub(crate) mod enchantment_helper;
pub mod entity;
pub mod fluid;
pub mod inventory;
pub mod level_data;
pub mod physics;
pub mod player;
pub mod poi;
pub(crate) mod portal;
pub mod server;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
pub(crate) mod test_support;
pub mod world;
pub mod worldgen;
