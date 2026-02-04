//! # Steel Utils
//!
//! This crate contains a collection of utilities used by the Steel Minecraft server.

#![feature(
    const_trait_impl,
    const_slice_make_iter,
    const_cmp,
    derive_const,
    core_intrinsics
)]
#![allow(internal_features)]

pub mod codec;
/// Direction enum for the six cardinal directions.
pub mod direction;
mod front_vec;
/// CRC32C hashing for component validation.
pub mod hash;
/// A module for custom locks.
pub mod locks;
/// Utilities for Steel logging.
pub mod logger;
pub mod math;
pub mod random;
pub mod serial;
pub mod text;
/// A module for common types.
pub mod types;
/// UUID extension trait for Minecraft NBT serialization.
pub mod uuid_ext;

#[rustfmt::skip]
#[path = "generated/vanilla_translations/ids.rs"]
#[allow(missing_docs, warnings)]
pub mod translations;
#[rustfmt::skip]
#[path = "generated/vanilla_translations/registry.rs"]
#[allow(missing_docs, warnings)]
pub mod translations_registry;
/// helpful tools for registry
pub mod registry;

pub use direction::Direction;
pub use front_vec::FrontVec;
pub use types::BlockPos;
pub use types::BlockStateId;
pub use types::ChunkPos;
pub use types::Identifier;
pub use types::SectionPos;
pub use uuid_ext::UuidExt;
