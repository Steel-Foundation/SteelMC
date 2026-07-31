use crate::structure::StructurePiece;
use core::slice;
use rustc_hash::FxHashMap;
use std::vec;
use steel_registry::structure::TerrainAdjustment;
use steel_utils::{BlockPos, BoundingBox, ChunkPos, Identifier};

/// A structure start placed in a chunk. Vanilla's `StructureStart` — invalid (empty)
/// starts are not stored.
#[derive(Debug, Clone)]
pub struct StructureStart {
    /// Structure id (e.g., `minecraft:village`).
    pub structure: Identifier,
    /// Origin chunk.
    pub chunk_pos: ChunkPos,
    /// Vanilla's map/locate reference counter. This is distinct from
    /// [`StructureReferenceMap`]; generating per-chunk structure references does
    /// not increment this counter.
    pub references: i32,
    /// Pieces composing this structure.
    pub pieces: Vec<StructurePiece>,
    /// Bounding-box inflation applied at construction. Vanilla inflates by 12
    /// when `terrain_adaptation != NONE`. Stored for serialization parity; the
    /// inflation is already baked into [`bounding_box`](Self::bounding_box).
    pub bb_inflate: i32,
    /// Terrain adaptation mode from the structure registry. Used by Beardifier.
    pub terrain_adjustment: TerrainAdjustment,
    /// Cached bounding box matching vanilla's `StructureStart.getBoundingBox()`:
    /// the union of piece bounding boxes, then `inflatedBy(bb_inflate)`.
    /// `None` iff `pieces` is empty.
    pub bounding_box: Option<BoundingBox>,
}

impl StructureStart {
    /// Creates a start, computing the inflated piece-union bounding box up-front.
    #[must_use]
    pub fn new(
        structure: Identifier,
        chunk_pos: ChunkPos,
        pieces: Vec<StructurePiece>,
        terrain_adjustment: TerrainAdjustment,
    ) -> Self {
        let bb_inflate = terrain_adjustment.bb_inflate();
        let bounding_box = Self::compute_bounding_box(&pieces, bb_inflate);
        Self {
            structure,
            chunk_pos,
            references: 0,
            pieces,
            bb_inflate,
            terrain_adjustment,
            bounding_box,
        }
    }

    /// Union of all pieces' bounding boxes, inflated by `bb_inflate` on every
    /// axis. Returns `None` if `pieces` is empty. Mirrors vanilla's
    /// `StructureStart.getBoundingBox()` (= `adjustBoundingBox(union)`).
    #[must_use]
    pub fn compute_bounding_box(pieces: &[StructurePiece], bb_inflate: i32) -> Option<BoundingBox> {
        let (first, rest) = pieces.split_first()?;
        let mut bb = first.bounding_box;
        for piece in rest {
            bb = BoundingBox::encapsulating(&bb, &piece.bounding_box);
        }
        Some(bb.inflate_xyz(bb_inflate, bb_inflate, bb_inflate))
    }

    /// Vanilla `StructureStart.placeInChunk` reference position: the first
    /// piece center X/Z and first piece minimum Y.
    #[must_use]
    pub fn placement_reference_pos(&self) -> Option<BlockPos> {
        let first_piece = self.pieces.first()?;
        let center = first_piece.bounding_box.center();
        Some(BlockPos::new(
            center.x,
            first_piece.bounding_box.min_y(),
            center.z,
        ))
    }
}

/// Structure starts keyed by structure id.
pub type StructureStartMap = FxHashMap<Identifier, StructureStart>;

/// Structure references → origin chunk positions.
///
/// Vanilla stores these as a fastutil `LongOpenHashSet`, so duplicates are
/// ignored and feature-stage iteration follows that table order.
pub type StructureReferenceMap = FxHashMap<Identifier, StructureReferenceSet>;

/// Set of structure-start chunk positions with vanilla iteration order.
///
/// Reference generation discovers sources in a stable scan order, but vanilla
/// stores the packed chunk longs in fastutil's `LongOpenHashSet`. Feature-stage
/// placement consumes the set through that table iteration order, so Steel keeps
/// the insertion order for persistence and exposes the vanilla iteration order
/// for worldgen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructureReferenceSet {
    insertion_order: Vec<ChunkPos>,
    iteration_order: Vec<ChunkPos>,
}

impl StructureReferenceSet {
    /// Inserts a chunk position if it was not already present.
    pub fn insert(&mut self, pos: ChunkPos) -> bool {
        if self.insertion_order.contains(&pos) {
            return false;
        }
        self.insertion_order.push(pos);
        self.rebuild_iteration_order();
        true
    }

    /// Extends this set with insertion-order duplicate removal.
    pub fn extend(&mut self, positions: impl IntoIterator<Item = ChunkPos>) {
        for pos in positions {
            self.insert(pos);
        }
    }

    /// Returns an iterator over positions in vanilla `LongOpenHashSet` order.
    pub fn iter(&self) -> slice::Iter<'_, ChunkPos> {
        self.iteration_order.iter()
    }

    /// Returns an iterator over positions in discovery order.
    pub fn insertion_order_iter(&self) -> slice::Iter<'_, ChunkPos> {
        self.insertion_order.iter()
    }

    /// Returns `true` when no positions are stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.insertion_order.is_empty()
    }

    fn rebuild_iteration_order(&mut self) {
        self.iteration_order = Self::vanilla_long_open_hash_set_order(&self.insertion_order);
    }

    fn vanilla_long_open_hash_set_order(insertion_order: &[ChunkPos]) -> Vec<ChunkPos> {
        let Some(table_size) = Self::vanilla_long_open_hash_set_table_size(insertion_order.len())
        else {
            return Vec::new();
        };
        let mask = (table_size - 1) as u64;
        let mut table = vec![None; table_size];
        let mut zero_key = None;

        for &pos in insertion_order {
            let packed = Self::pack_chunk_pos(pos);
            if packed == 0 {
                zero_key = Some(pos);
                continue;
            }

            let mut slot = (Self::fastutil_mix(packed) & mask) as usize;
            loop {
                if table[slot].is_none() {
                    table[slot] = Some(pos);
                    break;
                }
                slot = (slot + 1) & (table_size - 1);
            }
        }

        let mut ordered = Vec::with_capacity(insertion_order.len());
        if let Some(pos) = zero_key {
            ordered.push(pos);
        }
        for slot in (0..table_size).rev() {
            if let Some(pos) = table[slot] {
                ordered.push(pos);
            }
        }
        ordered
    }

    fn vanilla_long_open_hash_set_table_size(len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }

        let mut table_size = Self::fastutil_array_size(16);
        let mut max_fill = Self::fastutil_max_fill(table_size);
        let mut size = 0;
        for _ in 0..len {
            let old_size = size;
            size += 1;
            if old_size >= max_fill {
                table_size = Self::fastutil_array_size(size + 1);
                max_fill = Self::fastutil_max_fill(table_size);
            }
        }
        Some(table_size)
    }

    fn fastutil_array_size(expected: usize) -> usize {
        let needed = ((expected as f64) / 0.75).ceil() as usize;
        needed.max(2).next_power_of_two()
    }

    const fn fastutil_max_fill(table_size: usize) -> usize {
        let fill = table_size - table_size / 4;
        if fill < table_size {
            fill
        } else {
            table_size - 1
        }
    }

    const fn pack_chunk_pos(pos: ChunkPos) -> u64 {
        (pos.0.x as u32 as u64) | ((pos.0.y as u32 as u64) << 32)
    }

    const fn fastutil_mix(value: u64) -> u64 {
        let mixed = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mixed = mixed ^ (mixed >> 32);
        mixed ^ (mixed >> 16)
    }
}

impl FromIterator<ChunkPos> for StructureReferenceSet {
    fn from_iter<T: IntoIterator<Item = ChunkPos>>(iter: T) -> Self {
        let mut set = Self::default();
        set.extend(iter);
        set
    }
}

impl<'a> IntoIterator for &'a StructureReferenceSet {
    type IntoIter = slice::Iter<'a, ChunkPos>;
    type Item = &'a ChunkPos;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for StructureReferenceSet {
    type IntoIter = vec::IntoIter<ChunkPos>;
    type Item = ChunkPos;

    fn into_iter(self) -> Self::IntoIter {
        self.iteration_order.into_iter()
    }
}
