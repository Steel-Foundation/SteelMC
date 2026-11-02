//! Per-biome-cell counts of tinting fluid blocks in one chunk section.
//!
//! Clients see a fluid's water tint through client-only biome variants (see
//! `steel_registry::client_content::ClientBiomes`). A 4×4×4 biome cell shows the
//! variant of the tinting fluid with the most blocks in it. Ties go to the lowest
//! tint index. The counts are maintained incrementally on block changes, so a chunk
//! send only scans sections that actually contain such a fluid.

use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::BlockStateId;

use super::paletted_container::{BiomePalette, BlockPalette};

/// Biome cells per section (4×4×4).
pub const CELLS_PER_SECTION: usize = 64;

/// The biome cell index of a section-local block position, in vanilla quart order.
#[must_use]
pub const fn cell_index(x: usize, y: usize, z: usize) -> usize {
    ((y >> 2) << 4) | ((z >> 2) << 2) | (x >> 2)
}

/// The tint index of the fluid a block state holds, if that fluid tints water.
#[must_use]
pub fn state_tint_index(state: BlockStateId) -> Option<u16> {
    let content = &REGISTRY.client_content;
    if content.tinting_fluid_count() == 0 {
        return None;
    }
    let fluid = state.get_ticking_metadata().fluid_state();
    if fluid.is_empty() {
        return None;
    }
    content.tint_index_of(fluid.fluid_id)
}

/// Counts of tinting fluid blocks per cell, one row per tinting fluid present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TintCells {
    rows: Vec<(u16, [u16; CELLS_PER_SECTION])>,
}

impl TintCells {
    /// Counts every tinting fluid block of a palette. Returns `None` when the
    /// palette holds no tinting state, which is the case for every vanilla section.
    #[must_use]
    pub fn count(states: &BlockPalette) -> Option<Self> {
        if !states.maybe_has(|state| state_tint_index(state).is_some()) {
            return None;
        }
        let mut cells = Self::default();
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    if let Some(tint) = state_tint_index(states.get(x, y, z)) {
                        cells.add(tint, cell_index(x, y, z));
                    }
                }
            }
        }
        Some(cells)
    }

    /// Records one more block of `tint` in `cell`.
    pub fn add(&mut self, tint: u16, cell: usize) {
        if let Some((_, counts)) = self.rows.iter_mut().find(|(row, _)| *row == tint) {
            counts[cell] += 1;
            return;
        }
        let mut counts = [0; CELLS_PER_SECTION];
        counts[cell] = 1;
        self.rows.push((tint, counts));
        self.rows.sort_unstable_by_key(|(row, _)| *row);
    }

    /// Records one fewer block of `tint` in `cell`.
    pub fn remove(&mut self, tint: u16, cell: usize) {
        let Some((_, counts)) = self.rows.iter_mut().find(|(row, _)| *row == tint) else {
            debug_assert!(false, "removed a tint block that was never counted");
            return;
        };
        debug_assert!(counts[cell] > 0, "tint cell count underflow");
        counts[cell] = counts[cell].saturating_sub(1);
    }

    /// The tint a cell shows: the most blocks wins, ties go to the lowest index.
    #[must_use]
    pub fn dominant(&self, cell: usize) -> Option<u16> {
        let mut best: Option<(u16, u16)> = None;
        for (tint, counts) in &self.rows {
            let count = counts[cell];
            if count > 0 && best.is_none_or(|(_, most)| count > most) {
                best = Some((*tint, count));
            }
        }
        best.map(|(tint, _)| tint)
    }

    /// Whether any cell shows a tint.
    #[must_use]
    pub fn is_tinted(&self) -> bool {
        self.rows
            .iter()
            .any(|(_, counts)| counts.iter().any(|count| *count > 0))
    }

    /// The biome palette clients receive: tinted cells carry their variant id.
    #[must_use]
    pub fn project(&self, biomes: &BiomePalette) -> BiomePalette {
        let table = REGISTRY.client_content.client_biomes();
        let mut cube = Box::new([[[0_u16; 4]; 4]; 4]);
        for (y, plane) in cube.iter_mut().enumerate() {
            for (z, row) in plane.iter_mut().enumerate() {
                for (x, value) in row.iter_mut().enumerate() {
                    let biome = biomes.get(x, y, z);
                    *value = match self.dominant((y << 4) | (z << 2) | x) {
                        // Client ids stay below u16::MAX: the biome registry and its
                        // variants are bounded by the u16 server palette.
                        Some(tint) => table.variant_id(usize::from(biome), tint) as u16,
                        None => biome,
                    };
                }
            }
        }
        BiomePalette::from_cube(cube)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_index_matches_vanilla_quart_order() {
        assert_eq!(cell_index(0, 0, 0), 0);
        assert_eq!(cell_index(15, 0, 0), 3);
        assert_eq!(cell_index(0, 0, 15), 12);
        assert_eq!(cell_index(0, 15, 0), 48);
        assert_eq!(cell_index(15, 15, 15), 63);
    }

    #[test]
    fn dominant_prefers_more_blocks_then_lower_index() {
        let mut cells = TintCells::default();
        cells.add(1, 5);
        cells.add(1, 5);
        cells.add(0, 5);
        assert_eq!(cells.dominant(5), Some(1));
        cells.add(0, 5);
        assert_eq!(cells.dominant(5), Some(0));
        cells.remove(0, 5);
        cells.remove(0, 5);
        assert_eq!(cells.dominant(5), Some(1));
        cells.remove(1, 5);
        cells.remove(1, 5);
        assert_eq!(cells.dominant(5), None);
        assert!(!cells.is_tinted());
    }
}
