//! A paletted container is a container that can be either homogeneous or heterogeneous.
use std::{
    fmt::Debug,
    hash::Hash,
    io::{Result, Write},
    marker::PhantomData,
    mem, slice,
    sync::OnceLock,
};

use rustc_hash::FxHashMap;

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::{REGISTRY, RegistryExt as _};
use steel_utils::{BlockStateId, codec::VarInt, mth::ceil_log2, serial::WriteTo};

mod encode;

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

/// Index width, matching the chunk save format.
const fn storage_bits(palette_len: usize) -> u8 {
    match palette_len {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        17..=256 => 8,
        _ => 16,
    }
}

/// A heterogeneous palette container backed by bit-packed indices.
///
/// Palette order is kept as is so serialization stays the same.
#[derive(Debug, Clone)]
pub struct HeterogeneousPalette<V: Hash + Eq + Copy + Default, const DIM: usize> {
    palette: Vec<(V, u16)>,
    index_by_value: FxHashMap<V, usize>,
    indices: Box<[u64]>,
    bits: u8,
    _dim: PhantomData<[(); DIM]>,
}

impl<V: Hash + Eq + Copy + Default, const DIM: usize> HeterogeneousPalette<V, DIM> {
    const VOLUME: usize = DIM * DIM * DIM;

    /// The distinct values, each with the number of cells holding it.
    pub(crate) fn palette(&self) -> &[(V, u16)] {
        &self.palette
    }

    /// All values in y, z, x order.
    pub(crate) fn values(&self) -> impl ExactSizeIterator<Item = V> + '_ {
        (0..Self::VOLUME).map(move |linear| self.value_at_index(linear))
    }

    /// Values in y, z, x order.
    pub fn iter_values(&self) -> impl ExactSizeIterator<Item = V> + '_ {
        self.values()
    }

    pub(crate) fn indices_iter(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        (0..Self::VOLUME).map(|linear| self.index_at(linear))
    }

    #[inline]
    const fn linear(x: usize, y: usize, z: usize) -> usize {
        x + z * DIM + y * DIM * DIM
    }

    #[inline]
    fn index_at(&self, linear: usize) -> usize {
        let bits = usize::from(self.bits);
        let per_word = 64 / bits;
        let word = self.indices[linear / per_word];
        let shift = (linear % per_word) * bits;
        ((word >> shift) & ((1u64 << bits) - 1)) as usize
    }

    #[inline]
    fn value_at_index(&self, linear: usize) -> V {
        // Only the test helper makes a bad index, encoders check the raw indices.
        self.palette
            .get(self.index_at(linear))
            .map_or(V::default(), |(value, _)| *value)
    }

    #[inline]
    fn set_index_at(&mut self, linear: usize, index: usize) {
        let bits = usize::from(self.bits);
        let per_word = 64 / bits;
        let shift = (linear % per_word) * bits;
        let mask = ((1u64 << bits) - 1) << shift;
        let word = &mut self.indices[linear / per_word];
        *word = (*word & !mask) | ((index as u64) << shift);
    }

    /// Repacks indices at a new width.
    fn repack(&mut self, new_bits: u8) {
        let indices: Vec<usize> = (0..Self::VOLUME)
            .map(|linear| self.index_at(linear))
            .collect();
        let per_word = 64 / usize::from(new_bits);
        let mut packed = vec![0u64; Self::VOLUME.div_ceil(per_word)];
        for (linear, index) in indices.into_iter().enumerate() {
            let shift = (linear % per_word) * usize::from(new_bits);
            packed[linear / per_word] |= (index as u64) << shift;
        }
        self.indices = packed.into_boxed_slice();
        self.bits = new_bits;
    }

    /// Removes an entry and remaps cells that used the last slot.
    fn remove_palette_entry(&mut self, index: usize) {
        let last = self.palette.len() - 1;
        let removed = self.palette[index].0;
        self.index_by_value.remove(&removed);
        if index == last {
            self.palette.pop();
            return;
        }
        let swapped = self.palette[last].0;
        self.palette.swap_remove(index);
        self.index_by_value.insert(swapped, index);
        for linear in 0..Self::VOLUME {
            if self.index_at(linear) == last {
                self.set_index_at(linear, index);
            }
        }
    }

    fn get(&self, x: usize, y: usize, z: usize) -> V {
        self.value_at_index(Self::linear(x, y, z))
    }

    fn get_at_index(&self, linear: usize) -> V {
        self.value_at_index(linear)
    }

    fn set(&mut self, x: usize, y: usize, z: usize, value: V) -> V {
        let linear = Self::linear(x, y, z);
        let old_index = self.index_at(linear);
        let old_value = self.palette[old_index].0;
        if value == old_value {
            return old_value;
        }

        self.palette[old_index].1 -= 1;
        if self.palette[old_index].1 == 0 {
            self.remove_palette_entry(old_index);
        }

        let new_index = if let Some(&index) = self.index_by_value.get(&value) {
            index
        } else {
            let index = self.palette.len();
            self.palette.push((value, 0));
            self.index_by_value.insert(value, index);
            let new_bits = storage_bits(self.palette.len());
            if new_bits != self.bits {
                self.repack(new_bits);
            }
            index
        };
        self.palette[new_index].1 += 1;
        self.set_index_at(linear, new_index);
        old_value
    }
}

/// A paletted container.
///
/// `Building` is a transient mode used during worldgen: it's a raw cube without
/// palette tracking, so writes are O(1) stores. Must be finalized via
/// [`Self::finalize_building`] (or implicitly when the parent section recalculates
/// its counters) before any serialization or paletted access. Mirrors
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
    #[expect(clippy::boxed_local, reason = "callers pass boxed cubes from worldgen")]
    pub fn from_cube(cube: Box<Cube<V, DIM>>) -> Self {
        let mut palette: Vec<(V, u16)> = Vec::new();
        let mut index_by_value: FxHashMap<V, usize> = FxHashMap::default();
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
            if let Some(&pos) = index_by_value.get(&v) {
                palette[pos].1 += run_len;
            } else {
                index_by_value.insert(v, palette.len());
                palette.push((v, run_len));
            }
            i = j;
        }

        if palette.len() == 1 {
            Self::Homogeneous(palette[0].0)
        } else {
            let bits = storage_bits(palette.len());
            let per_word = 64 / usize::from(bits);
            let mut indices = vec![0u64; total.div_ceil(per_word)];
            for (linear, value) in flat.iter().enumerate() {
                let Some(&index) = index_by_value.get(value) else {
                    continue;
                };
                let shift = (linear % per_word) * usize::from(bits);
                indices[linear / per_word] |= (index as u64) << shift;
            }
            Self::Heterogeneous(HeterogeneousPalette {
                palette,
                index_by_value,
                indices: indices.into_boxed_slice(),
                bits,
                _dim: PhantomData,
            })
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

    /// Gets the value at a y,z,x linear index.
    ///
    /// The index layout is `x + z * DIM + y * DIM * DIM`, matching the flat
    /// order used when serializing palette data and by `ScalableLux` light propagation.
    pub fn get_at_index(&self, index: usize) -> V {
        debug_assert!(index < Self::VOLUME);

        match self {
            Self::Homogeneous(value) => *value,
            Self::Heterogeneous(data) => data.get_at_index(index),
            Self::Building(cube) => {
                let y = index / (DIM * DIM);
                let z = (index / DIM) % DIM;
                let x = index % DIM;
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
                    *slot = data.get(x, y, z);
                }
            }
            Self::Building(cube) => {
                for (y, slot) in out[..DIM].iter_mut().enumerate() {
                    *slot = cube[y][z][x];
                }
            }
        }
    }

    /// Returns whether this container's palette may contain a matching value.
    ///
    /// This checks palette entries instead of every cell, matching vanilla and
    /// `ScalableLux`'s fast pre-scan before doing a full section pass.
    #[must_use]
    pub fn maybe_has(&self, mut predicate: impl FnMut(V) -> bool) -> bool {
        match self {
            Self::Homogeneous(value) => predicate(*value),
            Self::Heterogeneous(data) => data.palette.iter().any(|(value, _)| predicate(*value)),
            Self::Building(cube) => cube
                .iter()
                .flatten()
                .flatten()
                .any(|value| predicate(*value)),
        }
    }

    /// Collects all values in the container in y, z, x order.
    #[must_use]
    pub fn collect_values(&self) -> Vec<V> {
        match self {
            Self::Homogeneous(value) => vec![*value; Self::VOLUME],
            Self::Heterogeneous(data) => data.iter_values().collect(),
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
            Self::Heterogeneous(data) => {
                let mut cube: Box<Cube<V, DIM>> = Box::new([[[V::default(); DIM]; DIM]; DIM]);
                for (y, slab) in cube.iter_mut().enumerate() {
                    for (z, row) in slab.iter_mut().enumerate() {
                        for (x, slot) in row.iter_mut().enumerate() {
                            *slot = data.get(x, y, z);
                        }
                    }
                }
                *self = Self::Building(cube);
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

    /// Writes a bad palette index for tests.
    #[cfg(test)]
    pub(crate) fn set_without_palette_for_test(&mut self, x: usize, y: usize, z: usize, _value: V) {
        if let Self::Heterogeneous(data) = self {
            // Widen so the bad index is visible to write().
            data.repack(16);
            let linear = HeterogeneousPalette::<V, DIM>::linear(x, y, z);
            data.set_index_at(linear, data.palette.len());
        }
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
    /// - If the container holds a value its palette cannot encode: one missing from its
    ///   section palette, or a global id too wide for the global palette. Nothing is
    ///   written then.
    ///
    /// # Panics
    /// - If the container is in [`Self::Building`] mode.
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
                // Packed first, so a broken palette fails before anything is written.
                let mut buffer = [0; encode::MAX_PACKED_BYTES];
                let packed = match mode {
                    PaletteMode::Global => {
                        let global_ids = data
                            .palette
                            .iter()
                            .map(|(value, _)| value.to_global_id())
                            .collect::<Vec<_>>();
                        let ids = data
                            .indices_iter()
                            .map(|index| global_ids.get(index).copied().unwrap_or(u32::MAX));
                        encode::pack_u32_iter(ids, bits, 1u32 << bits, &mut buffer)?
                    }
                    PaletteMode::Linear | PaletteMode::Hash => encode::pack_u32_iter(
                        data.indices_iter().map(|index| index as u32),
                        bits,
                        data.palette.len() as u32,
                        &mut buffer,
                    )?,
                };

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

                // writeFixedSizeLongArray: raw longs, no VarInt length prefix
                writer.write_all(packed)?;
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
                _ => (block_state_global_bits(), PaletteMode::Global),
            }
        } else {
            // Biomes
            match count {
                0..=1 => unreachable!("Homogeneous handled separately"),
                2 => (1, PaletteMode::Linear),
                3..=4 => (2, PaletteMode::Linear),
                5..=8 => (3, PaletteMode::Hash),
                _ => (biome_global_bits(), PaletteMode::Global),
            }
        }
    }
}

/// Bits per entry for the block-state global palette.
///
/// Derived from the registered block-state count and cached on first use, so a
/// version bump that changes the state count doesn't need a matching hardcoded
/// constant here. Matches vanilla, which sizes the global palette from the
/// registry rather than a fixed width.
fn block_state_global_bits() -> u8 {
    ceil_log2(usize::from(REGISTRY.blocks.next_state_id))
}

/// Bits per entry for the biome global palette. See [`block_state_global_bits`].
fn biome_global_bits() -> u8 {
    static BITS: OnceLock<u8> = OnceLock::new();
    *BITS.get_or_init(|| ceil_log2(REGISTRY.biomes.len()))
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
                for slab in cube {
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
    use std::io::{Cursor, ErrorKind};

    use super::{
        BiomePalette, BlockPalette, ToGlobalId, biome_global_bits, block_state_global_bits,
    };
    use steel_registry::init_vanilla_registry;
    use steel_utils::{BlockStateId, codec::VarInt, serial::ReadFrom as _};

    fn assert_column_matches_get(container: &BlockPalette, x: usize, z: usize) {
        let mut column = [BlockStateId::default(); 16];
        container.copy_column_into(x, z, &mut column);
        for (y, state) in column.into_iter().enumerate() {
            assert_eq!(state, container.get(x, y, z));
        }
    }

    #[test]
    fn get_at_index_reads_homogeneous_palette_values() {
        let container = BlockPalette::Homogeneous(BlockStateId(42));

        assert_eq!(container.get_at_index(0), BlockStateId(42));
        assert_eq!(
            container.get_at_index(BlockPalette::VOLUME - 1),
            BlockStateId(42)
        );
    }

    #[test]
    fn get_at_index_uses_y_z_x_linear_order_for_blocks() {
        let mut container = BlockPalette::Homogeneous(BlockStateId(0));

        container.set(3, 4, 5, BlockStateId(99));

        assert_eq!(
            container.get_at_index(3 + 5 * 16 + 4 * 16 * 16),
            BlockStateId(99)
        );
    }

    #[test]
    fn set_grows_and_collapses_the_palette_across_bit_widths() {
        init_vanilla_registry();
        let position = |index: usize| (index % 16, (index / 16) % 16, (index / 256) % 16);
        let mut container = BlockPalette::Homogeneous(BlockStateId(1));

        for index in 1..=300usize {
            let (x, y, z) = position(index);
            container.set(x, y, z, BlockStateId(index as u16));
        }
        for index in 1..=300usize {
            let (x, y, z) = position(index);
            assert_eq!(container.get(x, y, z), BlockStateId(index as u16));
        }
        for index in 1..=300usize {
            let (x, y, z) = position(index);
            container.set(x, y, z, BlockStateId(1));
        }
        for index in 1..=300usize {
            let (x, y, z) = position(index);
            assert_eq!(container.get(x, y, z), BlockStateId(1), "index {index}");
        }
        assert!(
            matches!(container, BlockPalette::Homogeneous(BlockStateId(1))),
            "final container: {container:?}"
        );
    }

    #[test]
    fn maybe_has_checks_palette_values_without_scanning_cells() {
        let mut container = BlockPalette::Homogeneous(BlockStateId(0));
        assert!(!container.maybe_has(|state| state == BlockStateId(7)));

        container.set(1, 2, 3, BlockStateId(7));
        assert!(container.maybe_has(|state| state == BlockStateId(7)));
        assert!(!container.maybe_has(|state| state == BlockStateId(9)));
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

    /// The written long array has no length prefix, so a wrong bit width
    /// desyncs every section parsed after this one.
    #[test]
    fn block_global_palette_uses_registry_derived_bit_width_and_long_count() {
        init_vanilla_registry();
        let mut cube = Box::new([[[BlockStateId::default(); 16]; 16]; 16]);
        let mut next_id = 1u16;
        for slab in &mut cube {
            for row in slab.iter_mut() {
                for value in row.iter_mut() {
                    *value = BlockStateId(next_id);
                    next_id = next_id.wrapping_add(1);
                }
            }
        }
        let container = BlockPalette::from_cube(cube);

        let mut written = Vec::new();
        container
            .write(&mut written)
            .expect("writing a global-palette container should not fail");

        let expected_bits = block_state_global_bits();
        assert_eq!(written[0], expected_bits);
        let values_per_long = 64 / usize::from(expected_bits);
        let expected_longs = BlockPalette::VOLUME.div_ceil(values_per_long);
        assert_eq!(written.len(), 1 + expected_longs * 8);
    }

    #[test]
    fn biome_global_palette_uses_registry_derived_bit_width_and_long_count() {
        init_vanilla_registry();
        let mut cube = Box::new([[[0u16; 4]; 4]; 4]);
        let mut next_id = 1u16;
        for slab in &mut cube {
            for row in slab.iter_mut() {
                for value in row.iter_mut() {
                    *value = next_id;
                    next_id += 1;
                }
            }
        }
        let container = BiomePalette::from_cube(cube);

        let mut written = Vec::new();
        container
            .write(&mut written)
            .expect("writing a global-palette container should not fail");

        let expected_bits = biome_global_bits();
        assert_eq!(written[0], expected_bits);
        let values_per_long = 64 / usize::from(expected_bits);
        let expected_longs = BiomePalette::VOLUME.div_ceil(values_per_long);
        assert_eq!(written.len(), 1 + expected_longs * 8);
    }

    /// Reads `write` output the way the vanilla client does: a bits byte, the palette
    /// (absent for the global palette), then `64 / bits` entries per big-endian long.
    /// Returns each entry's global id.
    fn decode_written(bytes: &[u8], volume: usize, max_paletted_bits: u8) -> Vec<u32> {
        let bits = bytes[0];
        let mut cursor = Cursor::new(bytes);
        cursor.set_position(1);
        if bits == 0 {
            let value = VarInt::read(&mut cursor).expect("single value").0 as u32;
            assert_eq!(cursor.position() as usize, bytes.len());
            return vec![value; volume];
        }
        let palette = (bits <= max_paletted_bits).then(|| {
            let len = VarInt::read(&mut cursor).expect("palette length").0 as u32;
            (0..len)
                .map(|_| VarInt::read(&mut cursor).expect("palette entry").0 as u32)
                .collect::<Vec<_>>()
        });
        let bits = usize::from(bits);
        let values_per_long = 64 / bits;
        let longs: Vec<u64> = bytes[cursor.position() as usize..]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|long| u64::from_be_bytes(*long))
            .collect();
        assert_eq!(longs.len(), volume.div_ceil(values_per_long));
        (0..volume)
            .map(|index| {
                let long = longs[index / values_per_long];
                let entry = (long >> ((index % values_per_long) * bits)) & ((1 << bits) - 1);
                match &palette {
                    Some(palette) => palette[entry as usize],
                    None => entry as u32,
                }
            })
            .collect()
    }

    fn xorshift(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    /// Covers each block palette strategy, including the widths where a long holds a
    /// partial final group (5, 6 and 7 bits).
    #[test]
    fn written_block_sections_decode_to_their_values_for_every_strategy() {
        init_vanilla_registry();
        let mut rng = 0x9E37_79B9_7F4A_7C15_u64;
        for distinct in [2u16, 16, 17, 33, 65, 129, 256, 257] {
            let mut cube = Box::new([[[BlockStateId::default(); 16]; 16]; 16]);
            for (index, value) in cube.iter_mut().flatten().flatten().enumerate() {
                // Every state appears at least once, the rest are random.
                let state = if index < usize::from(distinct) {
                    index as u16
                } else {
                    (xorshift(&mut rng) % u64::from(distinct)) as u16
                };
                *value = BlockStateId(1 + state);
            }
            let container = BlockPalette::from_cube(cube);
            let mut written = Vec::new();
            container
                .write(&mut written)
                .expect("a valid container should encode");

            let expected: Vec<u32> = container
                .collect_values()
                .iter()
                .map(ToGlobalId::to_global_id)
                .collect();
            assert_eq!(
                decode_written(&written, BlockPalette::VOLUME, 8),
                expected,
                "{distinct} distinct states"
            );
        }
    }

    #[test]
    fn written_biome_sections_decode_to_their_values_for_every_strategy() {
        init_vanilla_registry();
        for distinct in [2u16, 3, 5, 9] {
            let mut cube = Box::new([[[0u16; 4]; 4]; 4]);
            for (index, value) in cube.iter_mut().flatten().flatten().enumerate() {
                *value = index as u16 % distinct;
            }
            let container = BiomePalette::from_cube(cube);
            let mut written = Vec::new();
            container
                .write(&mut written)
                .expect("a valid container should encode");

            let expected: Vec<u32> = container
                .collect_values()
                .iter()
                .map(|v| u32::from(*v))
                .collect();
            assert_eq!(
                decode_written(&written, BiomePalette::VOLUME, 3),
                expected,
                "{distinct} distinct biomes"
            );
        }
    }

    /// Global-palette ids are written without a section palette, so an id wider than the
    /// global width would spill into the neighboring entry instead of being rejected.
    #[test]
    fn write_rejects_global_id_too_wide_for_the_global_palette() {
        init_vanilla_registry();
        let global_bits = biome_global_bits();
        let mut cube = Box::new([[[0u16; 4]; 4]; 4]);
        for (index, value) in cube.iter_mut().flatten().flatten().enumerate() {
            *value = index as u16;
        }
        cube[0][0][0] = 1 << global_bits;
        let container = BiomePalette::from_cube(cube);

        let mut written = Vec::new();
        let error = container
            .write(&mut written)
            .expect_err("an id too wide for the global palette must not encode");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(written.is_empty(), "nothing should be written on failure");
    }

    /// The encoder reuses a per-thread index table. A section holding a value missing from
    /// its palette must be rejected even when an earlier section indexed that value.
    #[test]
    fn write_rejects_value_missing_from_palette_after_it_was_indexed_elsewhere() {
        let stray = BlockStateId(7);
        let mut primer_cube = Box::new([[[BlockStateId(1); 16]; 16]; 16]);
        primer_cube[0][0][1] = stray;
        let mut written = Vec::new();
        BlockPalette::from_cube(primer_cube)
            .write(&mut written)
            .expect("a valid container should encode");

        let mut cube = Box::new([[[BlockStateId(1); 16]; 16]; 16]);
        cube[0][0][1] = BlockStateId(2);
        let mut corrupt = BlockPalette::from_cube(cube);
        corrupt.set_without_palette_for_test(0, 0, 0, stray);

        let mut written = Vec::new();
        let error = corrupt
            .write(&mut written)
            .expect_err("a value missing from the palette must not encode");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(written.is_empty(), "nothing should be written on failure");
    }
}
