//! Per-chunk bitset marking which block positions have already been visited
//! by a carver.
//!
//! Mirrors vanilla's `net.minecraft.world.level.chunk.CarvingMask`. Used by
//! `WorldCarver` to avoid repeatedly processing the same position when
//! multiple carver steps overlap.

/// A `16 × height × 16` bitset of local block positions in a chunk.
#[derive(Debug, Clone)]
pub struct CarvingMask {
    min_y: i32,
    height: i32,
    bits: Vec<u64>,
}

impl CarvingMask {
    /// Creates an empty mask covering the inclusive range `[min_y, max_y]`.
    ///
    /// # Panics
    ///
    /// Panics if `min_y` is greater than `max_y`.
    #[must_use]
    pub fn new(min_y: i32, max_y: i32) -> Self {
        assert!(min_y <= max_y, "carving mask range must not be empty");
        let height = max_y - min_y + 1;
        let total_bits = (256 * height) as usize;
        let lanes = total_bits.div_ceil(64);
        Self {
            min_y,
            height,
            bits: vec![0; lanes],
        }
    }

    /// Creates the range used by `NoiseBasedChunkGenerator.generateCarvers`.
    ///
    /// The bottom block and the top seven blocks are protected for normal
    /// generation, matching vanilla's non-upgrading chunk path.
    #[must_use]
    pub fn for_worldgen_context(min_y: i32, height: i32) -> Self {
        Self::new(min_y + 1, min_y + height - 8)
    }

    /// Vanilla's `getIndex`: `y - min_y + (z + (x << 4)) * height`.
    #[inline]
    const fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let column = ((z & 15) + ((x & 15) << 4)) as usize;
        (y - self.min_y) as usize + column * self.height as usize
    }

    /// Marks `(x, y, z)` as carved.
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32) {
        let idx = self.index(x, y, z);
        let lane = idx / 64;
        let bit = idx % 64;
        self.bits[lane] |= 1u64 << bit;
    }

    /// Marks `(x, y, z)` as carved if it was not already marked.
    ///
    /// Returns `true` when this call set the bit, or `false` when a previous
    /// carver step had already visited the position.
    #[inline]
    pub fn set_if_unset(&mut self, x: i32, y: i32, z: i32) -> bool {
        let idx = self.index(x, y, z);
        let lane = idx / 64;
        let bit = 1u64 << (idx % 64);
        if self.bits[lane] & bit != 0 {
            return false;
        }
        self.bits[lane] |= bit;
        true
    }

    /// Returns whether `(x, y, z)` has been carved.
    #[inline]
    #[must_use]
    pub fn get(&self, x: i32, y: i32, z: i32) -> bool {
        let idx = self.index(x, y, z);
        let lane = idx / 64;
        let bit = idx % 64;
        (self.bits[lane] >> bit) & 1 != 0
    }

    /// Returns whether no carver marked any position.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&word| word == 0)
    }

    /// Visits the marked vertical ranges in vanilla's bitset order.
    pub fn visit(&self, mut visitor: impl FnMut(i32, i32, i32, i32)) {
        let Some(mut start_index) = self.next_set_bit(0) else {
            return;
        };

        loop {
            let end_index = self.next_clear_bit(start_index) - 1;
            self.visit_segment(&mut visitor, start_index, end_index);

            let Some(next_start) = self.next_set_bit(end_index + 1) else {
                return;
            };
            start_index = next_start;
        }
    }

    fn visit_segment(
        &self,
        visitor: &mut impl FnMut(i32, i32, i32, i32),
        start_index: usize,
        end_index: usize,
    ) {
        let height = self.height as usize;
        for column in start_index / height..=end_index / height {
            let column_base = column * height;
            let bottom_y = (start_index.saturating_sub(column_base)).min(height - 1) as i32;
            let top_y = (end_index.saturating_sub(column_base)).min(height - 1) as i32;
            visitor(
                ((column >> 4) & 15) as i32,
                (column & 15) as i32,
                self.min_y + bottom_y,
                self.min_y + top_y,
            );
        }
    }

    fn next_set_bit(&self, from: usize) -> Option<usize> {
        let mut lane = from / 64;
        if lane >= self.bits.len() {
            return None;
        }
        let mut word = self.bits[lane] & (u64::MAX << (from % 64));
        loop {
            if word != 0 {
                return Some(lane * 64 + word.trailing_zeros() as usize);
            }
            lane += 1;
            word = *self.bits.get(lane)?;
        }
    }

    fn next_clear_bit(&self, from: usize) -> usize {
        let mut lane = from / 64;
        if lane >= self.bits.len() {
            return from;
        }
        let mut word = !self.bits[lane] & (u64::MAX << (from % 64));
        loop {
            if word != 0 {
                return lane * 64 + word.trailing_zeros() as usize;
            }
            lane += 1;
            let Some(next_word) = self.bits.get(lane) else {
                return self.bits.len() * 64;
            };
            word = !next_word;
        }
    }

    /// Y range bound at construction.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Inclusive upper Y bound.
    #[must_use]
    pub const fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }
}

#[cfg(test)]
mod test {
    use steel_worldgen::density_functions::{
        nether::NetherNoiseSettings, overworld::OverworldNoiseSettings,
    };

    use super::*;

    const OVERWORLD_CARVER_MIN_Y: i32 = OverworldNoiseSettings::MIN_Y + 1;
    const OVERWORLD_CARVER_MAX_Y: i32 =
        OverworldNoiseSettings::MIN_Y + OverworldNoiseSettings::HEIGHT - 8;
    const OVERWORLD_CARVER_HEIGHT: usize = OverworldNoiseSettings::HEIGHT as usize - 8;

    #[test]
    fn set_and_get_roundtrip() {
        let mut mask = CarvingMask::new(OVERWORLD_CARVER_MIN_Y, OVERWORLD_CARVER_MAX_Y);
        assert!(!mask.get(5, 10, 7));
        mask.set(5, 10, 7);
        assert!(mask.get(5, 10, 7));
        // Neighbors untouched
        assert!(!mask.get(4, 10, 7));
        assert!(!mask.get(5, 11, 7));
        assert!(!mask.get(5, 10, 8));
    }

    #[test]
    fn set_if_unset_reports_first_visit() {
        let mut mask = CarvingMask::new(OVERWORLD_CARVER_MIN_Y, OVERWORLD_CARVER_MAX_Y);
        assert!(mask.set_if_unset(5, 10, 7));
        assert!(!mask.set_if_unset(5, 10, 7));
        assert!(mask.get(5, 10, 7));
    }

    #[test]
    fn indexing_matches_vanilla_layout() {
        let mask = CarvingMask::new(OVERWORLD_CARVER_MIN_Y, OVERWORLD_CARVER_MAX_Y);
        // x=0, z=0, y=min_y → index 0
        assert_eq!(mask.index(0, OVERWORLD_CARVER_MIN_Y, 0), 0);
        // x=15, z=0, y=min_y → 240 columns × mask height.
        assert_eq!(
            mask.index(15, OVERWORLD_CARVER_MIN_Y, 0),
            240 * OVERWORLD_CARVER_HEIGHT
        );
        // x=0, z=1, y=min_y → one column.
        assert_eq!(
            mask.index(0, OVERWORLD_CARVER_MIN_Y, 1),
            OVERWORLD_CARVER_HEIGHT
        );
        // x=0, z=0, y=min_y+1 → next row in the column.
        assert_eq!(mask.index(0, OVERWORLD_CARVER_MIN_Y + 1, 0), 1);
    }

    #[test]
    fn x_and_z_are_masked_to_chunk_local() {
        let mut mask = CarvingMask::new(OVERWORLD_CARVER_MIN_Y, OVERWORLD_CARVER_MAX_Y);
        // Chunk-local: 17 → 1, 18 → 2
        mask.set(17, 0, 18);
        assert!(mask.get(1, 0, 2));
        assert!(mask.get(17, 0, 18));
    }

    #[test]
    fn worldgen_context_excludes_protected_edges() {
        let mask = CarvingMask::for_worldgen_context(
            OverworldNoiseSettings::MIN_Y,
            OverworldNoiseSettings::HEIGHT,
        );
        assert_eq!(mask.min_y(), OverworldNoiseSettings::MIN_Y + 1);
        assert_eq!(mask.max_y(), OVERWORLD_CARVER_MAX_Y);

        let nether = CarvingMask::for_worldgen_context(
            NetherNoiseSettings::MIN_Y,
            NetherNoiseSettings::HEIGHT,
        );
        assert_eq!(nether.min_y(), NetherNoiseSettings::MIN_Y + 1);
        assert_eq!(
            nether.max_y(),
            NetherNoiseSettings::MIN_Y + NetherNoiseSettings::HEIGHT - 8
        );
    }

    #[test]
    fn visit_splits_ranges_at_clear_bits_and_columns() {
        let mut mask = CarvingMask::new(-4, 3);
        mask.set(0, 3, 0);
        mask.set(0, -4, 1);
        mask.set(1, -3, 2);
        mask.set(1, -2, 2);
        mask.set(1, 0, 2);

        let mut visited = Vec::new();
        mask.visit(|x, z, bottom, top| visited.push((x, z, bottom, top)));

        assert_eq!(
            visited,
            vec![(0, 0, 3, 3), (0, 1, -4, -4), (1, 2, -3, -2), (1, 2, 0, 0)]
        );
    }
}
