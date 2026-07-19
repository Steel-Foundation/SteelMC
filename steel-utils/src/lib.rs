//! # Steel Utils
//!
//! This crate contains a collection of utilities used by the Steel Minecraft server.

#![feature(const_trait_impl, const_cmp, derive_const, array_try_from_fn)]

/// axis
pub mod axis;
/// Vanilla `BlockUtil` helpers.
pub mod block_util;
/// Climate system for biome selection.
pub mod climate;
pub mod codec;
/// Packed RGB and ARGB colors.
pub mod color;
/// Direction enum for the six cardinal directions.
pub mod direction;
/// Deterministic concrete-type downcasting for erased Steel objects.
pub mod downcast;
mod front_vec;
/// Shared geometry primitives.
pub mod geometry;
/// CRC32C hashing for component validation.
pub mod hash;
/// Java standard-library compatibility helpers.
pub mod java;
/// A module for custom locks.
pub mod locks;
/// Utilities for Steel logging.
pub mod logger;
/// Vanilla-compatible NBT helpers.
pub mod nbt;
pub mod random;
/// helpful tools for registry
pub mod registry;
pub mod rotation;
/// Data saving helpers
pub mod saved_data;
pub mod serial;
pub mod text;
/// A module for common types.
pub mod types;
/// UUID extension trait for Minecraft NBT serialization.
pub mod uuid_ext;
/// Vanilla-compatible value provider types (`VerticalAnchor`,
/// `HeightProvider`, `FloatProvider`).
pub mod value_providers;

#[rustfmt::skip]
#[path = "generated/vanilla_translations/ids.rs"]
#[expect(missing_docs, warnings)]
pub mod translations;
#[rustfmt::skip]
#[path = "generated/vanilla_translations/registry.rs"]
#[expect(missing_docs, warnings)]
pub mod translations_registry;
#[rustfmt::skip]
#[path = "generated/entity_events.rs"]
#[expect(missing_docs, warnings)]
pub mod entity_events;
#[rustfmt::skip]
#[path = "generated/version.rs"]
#[expect(missing_docs, warnings)]
pub mod version;

/// The Minecraft version this server supports.
pub const MC_VERSION: &str = version::MINECRAFT_VERSION;

pub use color::{ArgbColor, RgbColor};
pub use direction::Direction;
pub use downcast::{Downcast, DowncastType, DowncastTypeKey, ErasedType};
pub use front_vec::FrontVec;
pub use geometry::{BlockLocalAabb, BoundingBox, WorldAabb};
pub use rotation::Rotation;
pub use types::BlockPos;
pub use types::BlockStateId;
pub use types::ChunkPos;
pub use types::GlobalPos;
pub use types::Identifier;
pub use types::PackedBlockPos;
pub use types::PackedChunkLocalXZ;
pub use types::PackedChunkPos;
pub use types::PackedSectionBlockPos;
pub use types::PackedSectionPos;
pub use types::SectionPos;
pub use uuid_ext::UuidExt;
