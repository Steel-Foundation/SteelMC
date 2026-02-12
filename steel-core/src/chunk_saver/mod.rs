//! Chunk persistence module.
//!
//! This module handles saving and loading chunks to/from disk using a sector-based
//! region file format. Each region file contains a 32×32 grid of chunks.
//!
//! ## Format Overview
//!
//! Region files use a fixed 8KB header containing chunk locations, followed by
//! 4KB-aligned sectors for chunk data. Only the header is kept in memory; chunk
//! data is read on-demand via file seeking.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │ Magic (4 bytes): "STLR"                             │
//! │ Version (2 bytes) + Padding (2 bytes)               │
//! ├─────────────────────────────────────────────────────┤
//! │ Header: 1024 entries × 8 bytes = 8KB                │
//! │   Each entry: offset (u32) + size (u24) + flags (u8)│
//! ├─────────────────────────────────────────────────────┤
//! │ Chunk data in 4KB sectors (zstd compressed)         │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ### Key Features
//! - **No memory duplication**: chunks are loaded directly to runtime format
//! - **Lazy loading**: only reads chunks when needed, not entire regions
//! - **Fast existence checks**: just read 8 bytes from header
//! - **Per-chunk block state and biome palettes** for self-contained chunks
//! - **Power-of-2 bit packing** for efficient storage (1, 2, 4, 8, 16 bits)
//! - **Homogeneous section optimization** (single block type = no bit array)
//! - **zstd compression** per-chunk for good compression ratios

mod bit_pack;
mod format;
mod ram_only;
mod region_manager;
mod storage;

pub use format::*;
pub use ram_only::*;
pub use region_manager::*;
pub use storage::*;
