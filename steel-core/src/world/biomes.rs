//! Painting biomes at runtime, like vanilla's `/fillbiome`. Reading uses
//! [`World::biome_at`], vanilla's `Level.getBiome`.

use std::fmt::{self, Display, Formatter};
use std::sync::Arc;

use steel_registry::RegistryEntry as _;
use steel_registry::biome::BiomeRef;
use steel_utils::{BlockPos, ChunkPos, SectionPos};

use super::World;

/// Why a biome fill did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillBiomeError {
    /// The box misses the world's height range.
    Empty,
    /// Chunks in the box are not fully loaded; nothing was changed.
    Unloaded {
        /// How many touched chunks are not loaded.
        count: usize,
    },
}

impl Display for FillBiomeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the region is outside the world's height range"),
            Self::Unloaded { count } => write!(f, "{count} chunks in the region are not loaded"),
        }
    }
}

impl std::error::Error for FillBiomeError {}

impl World {
    /// Sets the biome of every 4×4×4 cell intersecting the inclusive box, with
    /// vanilla `/fillbiome` semantics. Clients see the change with this tick's
    /// block updates. Returns how many cells changed.
    ///
    /// # Errors
    /// Returns [`FillBiomeError`] before changing anything when the box misses the
    /// world's height range or touches a chunk that is not fully loaded.
    pub fn fill_biome(
        self: &Arc<Self>,
        from: BlockPos,
        to: BlockPos,
        biome: BiomeRef,
    ) -> Result<u64, FillBiomeError> {
        let min = BlockPos::new(
            from.0.x.min(to.0.x),
            from.0.y.min(to.0.y).max(self.get_min_y()),
            from.0.z.min(to.0.z),
        );
        let max = BlockPos::new(
            from.0.x.max(to.0.x),
            from.0.y.max(to.0.y).min(self.get_max_y()),
            from.0.z.max(to.0.z),
        );
        if min.0.y > max.0.y {
            return Err(FillBiomeError::Empty);
        }
        let chunk_range = |a: i32, b: i32| {
            SectionPos::block_to_section_coord(a)..=SectionPos::block_to_section_coord(b)
        };
        let chunks: Vec<ChunkPos> = chunk_range(min.0.z, max.0.z)
            .flat_map(|cz| chunk_range(min.0.x, max.0.x).map(move |cx| ChunkPos::new(cx, cz)))
            .collect();
        let unloaded = chunks
            .iter()
            .filter(|pos| self.chunk_map.with_full_chunk(**pos, |_| ()).is_none())
            .count();
        if unloaded > 0 {
            return Err(FillBiomeError::Unloaded { count: unloaded });
        }
        // Biome ids fit the u16 palette: the registry is bounded by it.
        let id = biome.id() as u16;
        let mut changed = 0;
        for chunk_pos in chunks {
            let chunk_min = BlockPos::new(chunk_pos.0.x * 16, min.0.y, chunk_pos.0.y * 16);
            let box_min = BlockPos::new(
                min.0.x.max(chunk_min.0.x),
                min.0.y,
                min.0.z.max(chunk_min.0.z),
            );
            let box_max = BlockPos::new(
                max.0.x.min(chunk_min.0.x + 15),
                max.0.y,
                max.0.z.min(chunk_min.0.z + 15),
            );
            let cells = self
                .chunk_map
                .with_full_chunk(chunk_pos, |chunk| {
                    chunk.fill_biome_cells(box_min, box_max, id)
                })
                .unwrap_or(0);
            if cells > 0 {
                changed += cells;
                self.chunk_map.client_biomes_changed(chunk_pos);
            }
        }
        Ok(changed)
    }
}
