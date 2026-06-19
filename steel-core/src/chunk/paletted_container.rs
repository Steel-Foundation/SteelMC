//! A paletted container is a container that can be either homogeneous or heterogeneous.
use std::{
    fmt::Debug,
    hash::Hash,
    io::{Result, Write},
    mem, slice,
};

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockStateId, codec::VarInt, serial::WriteTo};

/// A trait for converting a value to a global ID.
pub trait ToGlobalId {
    /// Converts the value to a global ID.
    fn to_global_id(&self) -> u32;
}

impl ToGlobalId for BlockStateId {
    fn to_global_id(&self) -> u32 {
        u32::from(self.0)
    }
}

impl ToGlobalId for u16 {
    fn to_global_id(&self) -> u32 {
        u32::from(*self)
    }
}

/// 3d array indexed by y,z,x
type Cube<T, const DIM: usize> = [[[T; DIM]; DIM]; DIM];

/// A heterogeneous palette container.
#[derive(Debug, Clone)]
pub struct HeterogeneousPalette<V: Hash + Eq + Copy, const DIM: usize> {
    pub(crate) cube: Box<Cube<V, DIM>>,
    // Keeps track of how many different times each value appears in the cube. (value, count)
    pub(crate) palette: Vec<(V, u16)>,
}

impl<V: Hash + Eq + Copy, const DIM: usize> HeterogeneousPalette<V, DIM> {
    fn get(&self, x: usize, y: usize, z: usize) -> V {
        debug_assert!(x < DIM);
        debug_assert!(y < DIM);
        debug_assert!(z < DIM);

        self.cube[y][z][x]
    }

    /// Returns an iterator over all values in the cube in y, z, x order.
    pub fn iter_values(&self) -> impl Iterator<Item = &V> {
        self.cube.iter().flatten().flatten()
    }

    fn set(&mut self, x: usize, y: usize, z: usize, value: V) -> V {
        debug_assert!(x < DIM);
        debug_assert!(y < DIM);
        debug_assert!(z < DIM);

        let old_value = self.cube[y][z][x];

        if let Some((_, count)) = self.palette.iter_mut().find(|(v, _)| *v == value) {
            *count += 1;
        } else {
            self.palette.push((value, 1));
        }

        if let Some((index, (_, count))) = self
            .palette
            .iter_mut()
            .enumerate()
            .find(|(_, (v, _))| *v == old_value)
        {
            *count -= 1;
            if *count == 0 {
                self.palette.swap_remove(index);
            }
        }

        self.cube[y][z][x] = value;

        old_value
    }
}

/// A paletted container.
///
/// `Building` is a transient mode used during worldgen: it's a raw cube without
/// palette tracking, so writes are O(1) stores. Must be finalized via
/// [`Self::finalize_building`] (or implicitly by [`Self::recalculate_counts_with`]
/// on the parent section) before any serialization or paletted access. Mirrors
/// `FastNoise`'s `FastChunkSection` write-only fill mode.
#[derive(Debug, Clone)]
pub enum PalettedContainer<V: Hash + Eq + Copy + Default, const DIM: usize> {
    /// A homogeneous container, where all values are the same.
    Homogeneous(V),
    /// A heterogeneous container, where values can be different.
    Heterogeneous(HeterogeneousPalette<V, DIM>),
    /// Write-only build mode: raw cube without palette tracking.
    /// `set` is a single store; `get` is a direct read.
    /// Convert back via [`Self::finalize_building`].
    Building(Box<Cube<V, DIM>>),
}

enum PaletteMode {
    Linear,
    Hash,
    Global,
}

impl<V: Hash + Eq + Copy + Default + Debug, const DIM: usize> PalettedContainer<V, DIM> {
    /// The size of the container in one dimension.
    pub const SIZE: usize = DIM;
    /// The volume of the container.
    pub const VOLUME: usize = DIM * DIM * DIM;

    /// Creates a `PalettedContainer` from a pre-built cube.
    ///
    /// Will automatically determine if the result should be homogeneous or heterogeneous.
    ///
    /// Walks the cube as a flat slice (it's `[[[V; DIM]; DIM]; DIM]` so memory
    /// is contiguous) and counts identical cells in runs. The inner "find run
    /// end" loop is a vectorizable equality scan, so long stone columns collapse
    /// to a single palette increment.
    #[must_use]
    pub fn from_cube(cube: Box<Cube<V, DIM>>) -> Self {
        let mut palette: Vec<(V, u16)> = Vec::new();
        let total = DIM * DIM * DIM;
        // SAFETY: `[[[V; DIM]; DIM]; DIM]` is a fully-contiguous array of
        // `DIM*DIM*DIM` `V`s, so casting its base pointer to `*const V` and
        // building a slice of that length is sound. `cube` is a live `Box`, so
        // the pointer is valid for the lifetime of the slice.
        let flat: &[V] = unsafe { slice::from_raw_parts(cube.as_ptr().cast::<V>(), total) };

        let mut i = 0;
        while i < total {
            let v = flat[i];
            let mut j = i + 1;
            while j < total && flat[j] == v {
                j += 1;
            }
            let run_len = (j - i) as u16;
            if let Some(pos) = palette.iter().position(|(value, _)| *value == v) {
                palette[pos].1 += run_len;
            } else {
                palette.push((v, run_len));
            }
            i = j;
        }

        if palette.len() == 1 {
            Self::Homogeneous(palette[0].0)
        } else {
            Self::Heterogeneous(HeterogeneousPalette { cube, palette })
        }
    }

    /// Gets the value at the given coordinates.
    pub fn get(&self, x: usize, y: usize, z: usize) -> V {
        match self {
            Self::Homogeneous(value) => *value,
            Self::Heterogeneous(data) => data.get(x, y, z),
            Self::Building(cube) => {
                debug_assert!(x < DIM);
                debug_assert!(y < DIM);
                debug_assert!(z < DIM);
                cube[y][z][x]
            }
        }
    }

    /// Copies the full vertical column at `(x, z)` into `out`.
    pub(crate) fn copy_column_into(&self, x: usize, z: usize, out: &mut [V]) {
        debug_assert!(x < DIM);
        debug_assert!(z < DIM);
        debug_assert!(out.len() >= DIM);

        match self {
            Self::Homogeneous(value) => {
                for slot in &mut out[..DIM] {
                    *slot = *value;
                }
            }
            Self::Heterogeneous(data) => {
                for (y, slot) in out[..DIM].iter_mut().enumerate() {
                    *slot = data.cube[y][z][x];
                }
            }
            Self::Building(cube) => {
                for (y, slot) in out[..DIM].iter_mut().enumerate() {
                    *slot = cube[y][z][x];
                }
            }
        }
    }

    /// Collects all values in the container in y, z, x order.
    #[must_use]
    pub fn collect_values(&self) -> Vec<V> {
        match self {
            Self::Homogeneous(value) => vec![*value; Self::VOLUME],
            Self::Heterogeneous(data) => data.iter_values().copied().collect(),
            Self::Building(cube) => cube.iter().flatten().flatten().copied().collect(),
        }
    }

    /// Switches the container into write-only build mode for fast bulk writes.
    /// Idempotent: a no-op if already in [`Self::Building`].
    ///
    /// Allocates a `Cube` if currently `Homogeneous`. For `Heterogeneous` it
    /// reuses the existing cube allocation.
    pub fn enter_building_mode(&mut self) {
        match self {
            Self::Building(_) => {}
            Self::Homogeneous(value) => {
                let cube: Box<Cube<V, DIM>> = Box::new([[[*value; DIM]; DIM]; DIM]);
                *self = Self::Building(cube);
            }
            Self::Heterogeneous(_) => {
                let taken = mem::replace(self, Self::Homogeneous(V::default()));
                let Self::Heterogeneous(data) = taken else {
                    unreachable!()
                };
                *self = Self::Building(data.cube);
            }
        }
    }

    /// Returns the raw cube backing this container as a flat mutable slice if
    /// it's currently in [`Self::Building`] mode. Indexing is `[y * DIM*DIM + z * DIM + x]`.
    ///
    /// Used by the chunk fill path to bypass the 3-arm `set` dispatch and the
    /// unused old-value load — write-only worldgen never reads back what it
    /// just wrote, so the read in `set` is wasted memory traffic.
    #[inline]
    pub fn as_building_slice_mut(&mut self) -> Option<&mut [V]> {
        if let Self::Building(cube) = self {
            // SAFETY: `[[[V; DIM]; DIM]; DIM]` is a contiguous array of
            // `DIM*DIM*DIM` `V`s; the cast preserves the live `&mut Box`'s
            // borrow because the returned slice cannot outlive `self`.
            Some(unsafe {
                slice::from_raw_parts_mut(cube.as_mut_ptr().cast::<V>(), DIM * DIM * DIM)
            })
        } else {
            None
        }
    }

    /// Finalizes a [`Self::Building`] container back to `Homogeneous` or
    /// `Heterogeneous` by scanning the cube once and constructing the palette.
    /// No-op if not in build mode.
    pub fn finalize_building(&mut self) {
        if !matches!(self, Self::Building(_)) {
            return;
        }
        let taken = mem::replace(self, Self::Homogeneous(V::default()));
        let Self::Building(cube) = taken else {
            unreachable!()
        };
        *self = Self::from_cube(cube);
    }

    /// Sets the value at the given coordinates.
    pub fn set(&mut self, x: usize, y: usize, z: usize, value: V) -> V {
        debug_assert!(x < Self::SIZE);
        debug_assert!(y < Self::SIZE);
        debug_assert!(z < Self::SIZE);

        match self {
            Self::Homogeneous(original) => {
                let original = *original;
                if value != original {
                    let mut cube = Box::new([[[original; DIM]; DIM]; DIM]);
                    cube[y][z][x] = value;
                    *self = Self::from_cube(cube);
                }
                original
            }
            Self::Heterogeneous(data) => {
                let original = data.set(x, y, z, value);
                if data.palette.len() == 1 {
                    *self = Self::Homogeneous(data.palette[0].0);
                }
                original
            }
            Self::Building(cube) => {
                let old = cube[y][z][x];
                cube[y][z][x] = value;
                old
            }
        }
    }

    /// Writes the container to the given writer.
    ///
    /// # Errors
    /// - If the writer fails to write.
    #[expect(
        clippy::missing_panics_doc,
        clippy::unwrap_used,
        reason = "position() is guaranteed to exist: palette was built from the cube's own values"
    )]
    pub fn write(&self, writer: &mut impl Write) -> Result<()>
    where
        V: ToGlobalId,
    {
        match self {
            Self::Homogeneous(value) => {
                // bits per entry = 0 (ZeroBitStorage)
                0u8.write(writer)?;
                // Single-value palette
                VarInt(value.to_global_id() as i32).write(writer)?;
                // writeFixedSizeLongArray(new long[0]) writes nothing
            }
            Self::Heterogeneous(data) => {
                let (bits, mode) = Self::calculate_strategy(data.palette.len());

                // Write bits per entry
                bits.write(writer)?;

                // Write Palette
                match mode {
                    PaletteMode::Linear | PaletteMode::Hash => {
                        VarInt(data.palette.len() as i32).write(writer)?;
                        for (val, _) in &data.palette {
                            VarInt(val.to_global_id() as i32).write(writer)?;
                        }
                    }
                    PaletteMode::Global => {}
                }

                // Pack data
                let indices: Vec<u32> = data
                    .cube
                    .iter()
                    .flatten()
                    .flatten()
                    .map(|val| {
                        if matches!(mode, PaletteMode::Global) {
                            val.to_global_id()
                        } else {
                            data.palette.iter().position(|(v, _)| v == val).unwrap() as u32
                        }
                    })
                    .collect();

                let packed = pack_bits(&indices, bits as usize);

                // writeFixedSizeLongArray: raw longs, no VarInt length prefix
                for long in packed {
                    long.write(writer)?;
                }
            }
            Self::Building(_) => {
                panic!(
                    "PalettedContainer in Building mode cannot be serialized; \
                     call finalize_building() first"
                );
            }
        }
        Ok(())
    }

    fn calculate_strategy(count: usize) -> (u8, PaletteMode) {
        if DIM == 16 {
            // Block states
            match count {
                0..=1 => unreachable!("Homogeneous handled separately"),
                2..=16 => (4, PaletteMode::Linear),
                17..=32 => (5, PaletteMode::Hash),
                33..=64 => (6, PaletteMode::Hash),
                65..=128 => (7, PaletteMode::Hash),
                129..=256 => (8, PaletteMode::Hash),
                _ => (15, PaletteMode::Global), // ceil(log2(max_block_state_id)) approx 15
            }
        } else {
            // Biomes
            match count {
                0..=1 => unreachable!("Homogeneous handled separately"),
                2 => (1, PaletteMode::Linear),
                3..=4 => (2, PaletteMode::Linear),
                5..=8 => (3, PaletteMode::Hash),
                _ => (6, PaletteMode::Global), // ceil(log2(max_biome_id)) approx 6
            }
        }
    }
}

fn pack_bits(indices: &[u32], bits: usize) -> Vec<u64> {
    let values_per_long = 64 / bits;
    let len = indices.len().div_ceil(values_per_long);
    let mut data = vec![0u64; len];

    for (i, &index) in indices.iter().enumerate() {
        let array_index = i / values_per_long;
        let offset = (i % values_per_long) * bits;
        data[array_index] |= u64::from(index) << offset;
    }

    data
}

/// A palette container for blocks.
pub type BlockPalette = PalettedContainer<BlockStateId, 16>;
/// A palette container for biomes.
pub type BiomePalette = PalettedContainer<u16, 4>;

impl BlockPalette {
    /// Gets the number of non-empty blocks in the container.
    #[must_use]
    pub fn non_empty_block_count(&self) -> u16 {
        match self {
            Self::Homogeneous(v) => {
                if v.0 == 0 {
                    0
                } else {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "VOLUME = 16^3 = 4096, fits in u16"
                    )]
                    {
                        Self::VOLUME as u16
                    }
                }
            }
            Self::Heterogeneous(data) => {
                let mut count = 0;
                for (v, c) in &data.palette {
                    if v.0 != 0 {
                        count += c;
                    }
                }
                count
            }
            Self::Building(cube) => {
                let mut count: u16 = 0;
                for slab in cube.iter() {
                    for row in slab {
                        for v in row {
                            if v.0 != 0 {
                                count += 1;
                            }
                        }
                    }
                }
                count
            }
        }
    }

    /// Returns `true` if this palette contains only air blocks.
    #[must_use]
    pub fn has_only_air(&self) -> bool {
        match self {
            Self::Homogeneous(v) => v.is_air(),
            //TODO: Use a nonEmpty counter?
            Self::Heterogeneous(_data) => false,
            Self::Building(cube) => cube
                .iter()
                .flatten()
                .flatten()
                .all(steel_utils::BlockStateId::is_air),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BlockPalette;
    use steel_utils::BlockStateId;

    fn assert_column_matches_get(container: &BlockPalette, x: usize, z: usize) {
        let mut column = [BlockStateId::default(); 16];
        container.copy_column_into(x, z, &mut column);
        for (y, state) in column.into_iter().enumerate() {
            assert_eq!(state, container.get(x, y, z));
        }
    }

    #[test]
    fn copy_column_into_matches_get_for_homogeneous_container() {
        let container = BlockPalette::Homogeneous(BlockStateId(7));
        assert_column_matches_get(&container, 3, 12);
    }

    #[test]
    fn copy_column_into_matches_get_for_heterogeneous_container() {
        let x = 5;
        let z = 9;
        let mut cube = Box::new([[[BlockStateId::default(); 16]; 16]; 16]);
        for y in 0..16 {
            cube[y][z][x] = BlockStateId((y + 1) as u16);
        }

        let container = BlockPalette::from_cube(cube);
        assert_column_matches_get(&container, x, z);
    }

    #[test]
    fn copy_column_into_matches_get_for_building_container() {
        let x = 11;
        let z = 2;
        let mut container = BlockPalette::Homogeneous(BlockStateId(3));
        container.enter_building_mode();
        for y in 0..16 {
            container.set(x, y, z, BlockStateId((31 + y) as u16));
        }

        assert_column_matches_get(&container, x, z);
    }
}
