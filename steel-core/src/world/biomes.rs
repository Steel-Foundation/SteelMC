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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use steel_registry::{REGISTRY, RegistryExt as _, init_vanilla_registry, vanilla_biomes};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};

    fn setup(name: &'static str, radius: i32) -> TestWorld {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let fixture = fresh_test_world(name);
        for x in -radius..radius {
            for z in -radius..radius {
                insert_ready_full_chunk(&fixture.world, ChunkPos::new(x, z));
            }
        }
        fixture
    }

    fn stored(world: &World, pos: BlockPos) -> Option<BiomeRef> {
        let chunk_pos = ChunkPos::new(pos.0.x >> 4, pos.0.z >> 4);
        let id = world.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            let index = ((pos.0.y - chunk.min_y()) / 16) as usize;
            let quart = |block: i32| ((block & 15) >> 2) as usize;
            chunk.sections().sections[index].read().biomes.get(
                quart(pos.0.x),
                quart(pos.0.y),
                quart(pos.0.z),
            )
        })?;
        REGISTRY.biomes.by_id(usize::from(id))
    }

    #[test]
    fn fills_every_intersecting_cell_across_chunks_once() -> Result<(), FillBiomeError> {
        let fixture = setup("fill_biome_cells", 1);
        let world = &fixture.world;
        // x -2..=5 touches quarts -1..=1 (3 cells), y 0..=3 one cell, z 0..=0 one cell.
        let from = BlockPos::new(-2, 0, 0);
        let to = BlockPos::new(5, 3, 0);
        let changed = world.fill_biome(from, to, &vanilla_biomes::DESERT)?;
        assert_eq!(changed, 3);
        for x in [-4, 0, 7] {
            assert_eq!(
                stored(world, BlockPos::new(x, 2, 3)),
                Some(&*vanilla_biomes::DESERT),
                "x {x}"
            );
        }
        assert_ne!(
            stored(world, BlockPos::new(8, 2, 3)),
            Some(&*vanilla_biomes::DESERT)
        );
        assert_eq!(
            world.fill_biome(from, to, &vanilla_biomes::DESERT)?,
            0,
            "repainting changes nothing"
        );
        Ok(())
    }

    #[test]
    fn refuses_unloaded_chunks_without_changes() {
        let fixture = setup("fill_biome_unloaded", 1);
        let world = &fixture.world;
        let result = world.fill_biome(
            BlockPos::new(0, 0, 0),
            BlockPos::new(40, 3, 0),
            &vanilla_biomes::DESERT,
        );
        assert_eq!(result, Err(FillBiomeError::Unloaded { count: 2 }));
        assert_ne!(
            stored(world, BlockPos::new(0, 0, 0)),
            Some(&*vanilla_biomes::DESERT)
        );
    }

    #[test]
    fn refuses_boxes_outside_the_height_range() {
        let fixture = setup("fill_biome_empty", 1);
        let world = &fixture.world;
        let above = world.get_max_y() + 10;
        let result = world.fill_biome(
            BlockPos::new(0, above, 0),
            BlockPos::new(3, above + 3, 3),
            &vanilla_biomes::DESERT,
        );
        assert_eq!(result, Err(FillBiomeError::Empty));
    }

    /// SC-005: painting 256×256×384 takes under two seconds. Run in release:
    /// `cargo test --release -p steel-core --lib paints_a_large_region -- --ignored`.
    #[test]
    #[ignore = "timing benchmark; run in release"]
    fn paints_a_large_region_quickly() -> Result<(), FillBiomeError> {
        let fixture = setup("fill_biome_benchmark", 8);
        let world = &fixture.world;
        let started = Instant::now();
        let changed = world.fill_biome(
            BlockPos::new(-128, world.get_min_y(), -128),
            BlockPos::new(127, world.get_max_y(), 127),
            &vanilla_biomes::DESERT,
        )?;
        let elapsed = started.elapsed();
        println!("painted {changed} cells in {elapsed:?}");
        assert!(changed > 0);
        assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
        Ok(())
    }
}
