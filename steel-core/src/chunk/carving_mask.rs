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
    /// 256 bits per Y layer (`x` in low 4 bits, `z` in next 4 bits).
    bits: Vec<u64>,
}

impl CarvingMask {
    /// Creates an empty mask covering `[min_y, min_y + height)`.
    #[must_use]
    pub fn new(height: i32, min_y: i32) -> Self {
        let total_bits = (256 * height) as usize;
        let words = total_bits.div_ceil(64);
        Self {
            min_y,
            height,
            bits: vec![0; words],
        }
    }

    /// Vanilla's `getIndex`: `x & 15 | (z & 15) << 4 | (y - min_y) << 8`.
    #[inline]
    const fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let xi = (x & 15) as u32;
        let zi = ((z & 15) as u32) << 4;
        let yi = ((y - self.min_y) as u32) << 8;
        (xi | zi | yi) as usize
    }

    /// Marks `(x, y, z)` as carved.
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32) {
        let idx = self.index(x, y, z);
        let word = idx / 64;
        let bit = idx % 64;
        self.bits[word] |= 1u64 << bit;
    }

    /// Returns whether `(x, y, z)` has been carved.
    #[inline]
    #[must_use]
    pub fn get(&self, x: i32, y: i32, z: i32) -> bool {
        let idx = self.index(x, y, z);
        let word = idx / 64;
        let bit = idx % 64;
        (self.bits[word] >> bit) & 1 != 0
    }

    /// Y range bound at construction.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Height in blocks.
    #[must_use]
    pub const fn height(&self) -> i32 {
        self.height
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_and_get_roundtrip() {
        let mut mask = CarvingMask::new(384, -64);
        assert!(!mask.get(5, 10, 7));
        mask.set(5, 10, 7);
        assert!(mask.get(5, 10, 7));
        // Neighbours untouched
        assert!(!mask.get(4, 10, 7));
        assert!(!mask.get(5, 11, 7));
        assert!(!mask.get(5, 10, 8));
    }

    #[test]
    fn indexing_matches_vanilla_layout() {
        let mask = CarvingMask::new(384, -64);
        // x=0, z=0, y=min_y → index 0
        assert_eq!(mask.index(0, -64, 0), 0);
        // x=15, z=0, y=min_y → 15
        assert_eq!(mask.index(15, -64, 0), 15);
        // x=0, z=1, y=min_y → 16
        assert_eq!(mask.index(0, -64, 1), 16);
        // x=0, z=0, y=min_y+1 → 256
        assert_eq!(mask.index(0, -63, 0), 256);
    }

    #[test]
    fn x_and_z_are_masked_to_chunk_local() {
        let mut mask = CarvingMask::new(384, -64);
        // Chunk-local: 17 → 1, 18 → 2
        mask.set(17, 0, 18);
        assert!(mask.get(1, 0, 2));
        assert!(mask.get(17, 0, 18));
    }
}
