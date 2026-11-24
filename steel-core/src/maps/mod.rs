//! Domain-shared map pixels, markers, synchronization, and persistence.

mod banners;
mod carrying;
mod chunk_io;
mod data;
mod preview;
mod storage;
mod structures;
mod terrain;
mod world;

pub use data::{MapBanner, MapFrame, MapItemSavedData};
pub(crate) use storage::{DomainMaps, MapStorage};

/// Width and height of a vanilla map image.
pub const MAP_SIZE: usize = 128;

const HALF_MAP_SIZE: i32 = (MAP_SIZE / 2) as i32;
const DECORATION_EDGE: f32 = (HALF_MAP_SIZE - 1) as f32;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod preview_tests;

#[cfg(test)]
mod interaction_tests;
