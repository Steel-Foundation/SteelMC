use std::iter::FusedIterator;

use steel_utils::{BlockPos, ChunkPos, Direction, SectionPos};

use super::LightSectionRange;

/// Horizontal cache radius used by `ScalableLux` light propagation.
pub const LIGHT_CACHE_RADIUS: i32 = 2;
/// Horizontal radius where `ScalableLux` populates section and nibble cache data.
pub const LIGHT_CACHE_SECTION_RADIUS: i32 = 1;
/// Horizontal cache width and depth used by `ScalableLux` light propagation.
pub const LIGHT_CACHE_DIAMETER: usize = LIGHT_CACHE_RADIUS as usize * 2 + 1;
/// Number of chunk columns in one light-engine cache window.
pub const LIGHT_CACHE_CHUNK_SLOTS: usize = LIGHT_CACHE_DIAMETER * LIGHT_CACHE_DIAMETER;

const LIGHT_CACHE_DIAMETER_I64: i64 = LIGHT_CACHE_DIAMETER as i64;
const LIGHT_CACHE_CHUNK_SLOTS_I64: i64 = LIGHT_CACHE_CHUNK_SLOTS as i64;
const LIGHT_CACHE_SECTION_RADIUS_I64: i64 = LIGHT_CACHE_SECTION_RADIUS as i64;
const LIGHT_LOCAL_BLOCK_MASK: usize = 15;
const LIGHT_LOCAL_BLOCK_Z_SHIFT: usize = 4;
const LIGHT_LOCAL_BLOCK_Y_SHIFT: usize = 8;
const LIGHT_ENCODED_HORIZONTAL_BITS: i64 = 6;
const LIGHT_ENCODED_VERTICAL_BITS: i64 = 16;
const LIGHT_ENCODED_HORIZONTAL_MASK: i64 = (1 << LIGHT_ENCODED_HORIZONTAL_BITS) - 1;
const LIGHT_ENCODED_VERTICAL_MASK: i64 = (1 << LIGHT_ENCODED_VERTICAL_BITS) - 1;
const LIGHT_ENCODED_POSITION_MASK: u32 =
    (1 << (LIGHT_ENCODED_HORIZONTAL_BITS * 2 + LIGHT_ENCODED_VERTICAL_BITS)) - 1;
const LIGHT_ENCODED_Z_SHIFT: u32 = LIGHT_ENCODED_HORIZONTAL_BITS as u32;
const LIGHT_ENCODED_Y_SHIFT: u32 = (LIGHT_ENCODED_HORIZONTAL_BITS * 2) as u32;

/// `ScalableLux` packed block position used in light propagation queue entries.
///
/// The lower 28 bits store `x | (z << 6) | (y << 12)`. X and Z are encoded in
/// a 64-block window around the active chunk; Y is encoded relative to the
/// section below the vanilla light-section range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedLightBlockPos(u32);

impl PackedLightBlockPos {
    /// Creates a packed light block position from raw queue bits.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw & LIGHT_ENCODED_POSITION_MASK)
    }

    /// Returns the raw lower 28 queue-position bits.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns the encoded 6-bit X coordinate.
    #[must_use]
    pub const fn encoded_x(self) -> u8 {
        (self.0 & LIGHT_ENCODED_HORIZONTAL_MASK as u32) as u8
    }

    /// Returns the encoded 6-bit Z coordinate.
    #[must_use]
    pub const fn encoded_z(self) -> u8 {
        ((self.0 >> LIGHT_ENCODED_Z_SHIFT) & LIGHT_ENCODED_HORIZONTAL_MASK as u32) as u8
    }

    /// Returns the encoded 16-bit Y coordinate.
    #[must_use]
    pub const fn encoded_y(self) -> u16 {
        ((self.0 >> LIGHT_ENCODED_Y_SHIFT) & LIGHT_ENCODED_VERTICAL_MASK as u32) as u16
    }
}

/// `ScalableLux` cache role for a cached chunk column.
///
/// During two-radius setup `ScalableLux` may cache chunk and emptiness-map data
/// for the full 5x5 window, but section and nibble arrays are normally
/// populated only for the inner 3x3 chunk window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightCacheChunkScope {
    /// Chunk is within the inner 3x3 window and may populate sections/nibbles.
    Inner,
    /// Chunk is in the outer ring and is cached for chunk/emptiness lookups only.
    Outer,
}

/// `ScalableLux` setup-cache radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightCacheSetupRadius {
    /// Scan the inner 3x3 chunk window.
    Inner,
    /// Scan the full 5x5 chunk window.
    Full,
}

impl LightCacheSetupRadius {
    const fn chunk_radius(self) -> i32 {
        match self {
            Self::Inner => LIGHT_CACHE_SECTION_RADIUS,
            Self::Full => LIGHT_CACHE_RADIUS,
        }
    }

    const fn chunk_count(self) -> usize {
        let diameter = self.chunk_radius() as usize * 2 + 1;
        diameter * diameter
    }
}

/// Cached chunk slot plus its `ScalableLux` cache role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedLightChunk {
    /// World chunk position for this cached chunk.
    pub chunk_pos: ChunkPos,
    /// Slot into `ScalableLux`'s 5x5 chunk cache arrays.
    pub chunk_slot: usize,
    /// Whether this chunk is in the inner section/nibble radius or outer ring.
    pub scope: LightCacheChunkScope,
}

/// Cached section slot for one section position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedLightSection {
    /// World section position for this cached section.
    pub section_pos: SectionPos,
    /// Slot into `ScalableLux`'s section/nibble cache arrays.
    pub section_slot: usize,
}

/// Cached section slot and local nibble index for one block.
///
/// `ScalableLux` propagation uses `sectionIndex` plus local index
/// `x | (z << 4) | (y << 8)` instead of repeatedly materializing section
/// positions and local coordinates inside the hot propagation loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedLightBlock {
    /// World block position for this cached block.
    pub block_pos: BlockPos,
    /// Slot into `ScalableLux`'s section/nibble cache arrays.
    pub section_slot: usize,
    /// Local block index inside the 16x16x16 light section.
    pub local_index: usize,
}

/// Iterator over chunks scanned by `ScalableLux` cache setup.
#[derive(Debug, Clone)]
pub struct LightCacheSetupChunks {
    layout: LightCacheLayout,
    radius: i32,
    next_dx: i32,
    next_dz: i32,
    remaining: usize,
}

impl Iterator for LightCacheSetupChunks {
    type Item = CachedLightChunk;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let dx = self.next_dx;
        let dz = self.next_dz;
        self.remaining -= 1;

        self.next_dx += 1;
        if self.next_dx > self.radius {
            self.next_dx = -self.radius;
            self.next_dz += 1;
        }

        let chunk_x = self.layout.center_chunk.0.x + dx;
        let chunk_z = self.layout.center_chunk.0.y + dz;
        let chunk = self.layout.cached_chunk_by_coords(chunk_x, chunk_z);
        if chunk.is_none() {
            self.remaining = 0;
        }
        chunk
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for LightCacheSetupChunks {}

impl FusedIterator for LightCacheSetupChunks {}

/// Optional value slots for `ScalableLux`'s 5x5 chunk cache arrays.
#[derive(Debug, Clone)]
pub struct LightChunkSlotArray<T> {
    values: Box<[Option<T>]>,
}

impl<T> LightChunkSlotArray<T> {
    /// Creates an empty 5x5 chunk slot array.
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: empty_option_slots(LIGHT_CACHE_CHUNK_SLOTS),
        }
    }

    /// Returns the number of chunk slots.
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.values.len()
    }

    /// Returns true when no chunk slots hold values.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.values.iter().all(Option::is_none)
    }

    /// Clears every chunk slot.
    pub fn clear(&mut self) {
        for value in &mut self.values {
            *value = None;
        }
    }

    /// Inserts a value at a cached chunk slot.
    pub fn insert(&mut self, chunk: CachedLightChunk, value: T) -> Option<T> {
        self.insert_slot(chunk.chunk_slot, value)
    }

    /// Inserts a value at a raw chunk slot.
    pub fn insert_slot(&mut self, chunk_slot: usize, value: T) -> Option<T> {
        self.values
            .get_mut(chunk_slot)
            .and_then(|slot| slot.replace(value))
    }

    /// Returns the value at a cached chunk slot.
    #[must_use]
    pub fn get(&self, chunk: CachedLightChunk) -> Option<&T> {
        self.get_slot(chunk.chunk_slot)
    }

    /// Returns the mutable value at a cached chunk slot.
    pub fn get_mut(&mut self, chunk: CachedLightChunk) -> Option<&mut T> {
        self.get_mut_slot(chunk.chunk_slot)
    }

    /// Returns the value at a raw chunk slot.
    #[must_use]
    pub fn get_slot(&self, chunk_slot: usize) -> Option<&T> {
        self.values.get(chunk_slot).and_then(Option::as_ref)
    }

    /// Returns the mutable value at a raw chunk slot.
    pub fn get_mut_slot(&mut self, chunk_slot: usize) -> Option<&mut T> {
        self.values.get_mut(chunk_slot).and_then(Option::as_mut)
    }
}

impl<T> Default for LightChunkSlotArray<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator over one inner cached chunk's vanilla light-section slots.
#[derive(Debug, Clone)]
pub struct LightChunkSectionSlots {
    layout: LightCacheLayout,
    chunk_pos: ChunkPos,
    next_section_y: i32,
    end_section_y: i32,
}

impl Iterator for LightChunkSectionSlots {
    type Item = CachedLightSection;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_section_y >= self.end_section_y {
            return None;
        }

        let section_y = self.next_section_y;
        self.next_section_y += 1;
        let section = self.layout.cached_section(SectionPos::new(
            self.chunk_pos.0.x,
            section_y,
            self.chunk_pos.0.y,
        ));
        if section.is_none() {
            self.next_section_y = self.end_section_y;
        }
        section
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.end_section_y - self.next_section_y).max(0) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for LightChunkSectionSlots {}

impl FusedIterator for LightChunkSectionSlots {}

/// Optional value slots for `ScalableLux`'s section/nibble cache arrays.
#[derive(Debug, Clone)]
pub struct LightSectionSlotArray<T> {
    layout: LightCacheLayout,
    values: Box<[Option<T>]>,
}

impl<T> LightSectionSlotArray<T> {
    /// Creates an empty section slot array sized for `layout`.
    #[must_use]
    pub fn new(layout: LightCacheLayout) -> Self {
        Self {
            layout,
            values: empty_option_slots(layout.section_slot_count()),
        }
    }

    /// Returns the layout this array is indexed by.
    #[must_use]
    pub const fn layout(&self) -> LightCacheLayout {
        self.layout
    }

    /// Returns the number of section slots.
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.values.len()
    }

    /// Returns true when no section slots hold values.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.values.iter().all(Option::is_none)
    }

    /// Clears every section slot.
    pub fn clear(&mut self) {
        for value in &mut self.values {
            *value = None;
        }
    }

    /// Inserts a value at a cached section slot.
    pub fn insert(&mut self, section: CachedLightSection, value: T) -> Option<T> {
        self.insert_slot(section.section_slot, value)
    }

    /// Inserts a value at a raw section slot.
    pub fn insert_slot(&mut self, section_slot: usize, value: T) -> Option<T> {
        self.values
            .get_mut(section_slot)
            .and_then(|slot| slot.replace(value))
    }

    /// Removes and returns the value at a raw section slot.
    pub fn take_slot(&mut self, section_slot: usize) -> Option<T> {
        self.values.get_mut(section_slot).and_then(Option::take)
    }

    /// Returns the value at a cached section slot.
    #[must_use]
    pub fn get(&self, section: CachedLightSection) -> Option<&T> {
        self.get_slot(section.section_slot)
    }

    /// Returns the mutable value at a cached section slot.
    pub fn get_mut(&mut self, section: CachedLightSection) -> Option<&mut T> {
        self.get_mut_slot(section.section_slot)
    }

    /// Returns the value at a raw section slot.
    #[must_use]
    pub fn get_slot(&self, section_slot: usize) -> Option<&T> {
        self.values.get(section_slot).and_then(Option::as_ref)
    }

    /// Returns the mutable value at a raw section slot.
    pub fn get_mut_slot(&mut self, section_slot: usize) -> Option<&mut T> {
        self.values.get_mut(section_slot).and_then(Option::as_mut)
    }
}

/// Section-slot notification flags used while publishing visible light updates.
///
/// `ScalableLux` keeps `notifyUpdateCache` beside its nibble cache and marks the
/// cached sections touched by a block's one-block lighting neighborhood. The
/// light engine later scans the same section slots while publishing dirty
/// nibbles and notifying clients.
#[derive(Debug, Clone)]
pub struct LightUpdateNotificationCache {
    layout: LightCacheLayout,
    marked: Box<[bool]>,
}

impl LightUpdateNotificationCache {
    /// Creates an empty notification cache for a light-engine cache window.
    #[must_use]
    pub fn new(layout: LightCacheLayout) -> Self {
        Self {
            layout,
            marked: vec![false; layout.section_slot_count()].into_boxed_slice(),
        }
    }

    /// Returns the layout this notification cache is indexed by.
    #[must_use]
    pub const fn layout(&self) -> LightCacheLayout {
        self.layout
    }

    /// Returns true when no section slots are marked for notification.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marked.iter().all(|marked| !marked)
    }

    /// Removes every pending section notification.
    pub fn clear(&mut self) {
        self.marked.fill(false);
    }

    /// Marks a cached section, returning true only when it was newly marked.
    pub fn mark_section(&mut self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };

        self.mark_section_slot(section_slot)
    }

    /// Marks every cached section touched by a block's lighting neighborhood.
    ///
    /// Returns `None` if the full one-block neighborhood is not inside this
    /// cache window, so callers do not accidentally publish a partial update.
    pub fn mark_block_neighborhood(&mut self, block_pos: BlockPos) -> Option<usize> {
        let mut contained = true;
        sections_around_and_at_block_pos(block_pos, |section_pos| {
            contained &= self.layout.section_slot(section_pos).is_some();
        });
        if !contained {
            return None;
        }

        let mut newly_marked = 0;
        sections_around_and_at_block_pos(block_pos, |section_pos| {
            if self.mark_section(section_pos) {
                newly_marked += 1;
            }
        });
        Some(newly_marked)
    }

    /// Returns whether a cached section is marked for notification.
    #[must_use]
    pub fn is_marked_section(&self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };

        self.is_marked_section_slot(section_slot)
    }

    /// Returns whether a section slot is marked for notification.
    #[must_use]
    pub fn is_marked_section_slot(&self, section_slot: usize) -> bool {
        self.marked.get(section_slot).copied().unwrap_or(false)
    }

    /// Iterates marked section positions in cache-slot order.
    pub fn marked_section_positions(&self) -> impl Iterator<Item = SectionPos> + '_ {
        self.marked
            .iter()
            .enumerate()
            .filter_map(move |(section_slot, marked)| {
                if *marked {
                    self.layout.section_pos_for_slot(section_slot)
                } else {
                    None
                }
            })
    }

    fn mark_section_slot(&mut self, section_slot: usize) -> bool {
        let Some(marked) = self.marked.get_mut(section_slot) else {
            return false;
        };

        let newly_marked = !*marked;
        *marked = true;
        newly_marked
    }
}

/// `ScalableLux` cache-window layout for chunk, section, and nibble arrays.
///
/// `ScalableLux` keeps a 5x5 chunk window around the active chunk and stores
/// light sections in flat arrays with one extra cached section below and above
/// the vanilla light-section range. This type owns that index math so the
/// light engine can share the same slots for chunk sections, nibbles, and
/// update notifications without repeating coordinate transforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightCacheLayout {
    center_chunk: ChunkPos,
    range: LightSectionRange,
    cached_min_section_y: i32,
    cached_section_count: usize,
    chunk_index_offset: i64,
    chunk_section_index_offset: i64,
    encode_offset_x: i64,
    encode_offset_y: i64,
    encode_offset_z: i64,
    encoded_min_block_x: i64,
    encoded_min_block_z: i64,
}

impl LightCacheLayout {
    /// Creates a cache layout centered on one chunk.
    #[must_use]
    pub fn new(center_chunk: ChunkPos, range: LightSectionRange) -> Self {
        let cached_min_section_y = range.min_section_y() - 1;
        let chunk_offset_x = i64::from(LIGHT_CACHE_RADIUS) - i64::from(center_chunk.0.x);
        let chunk_offset_z = i64::from(LIGHT_CACHE_RADIUS) - i64::from(center_chunk.0.y);
        let chunk_index_offset = chunk_offset_x + LIGHT_CACHE_DIAMETER_I64 * chunk_offset_z;
        let chunk_offset_y = -i64::from(cached_min_section_y);
        let chunk_section_index_offset =
            chunk_index_offset + LIGHT_CACHE_CHUNK_SLOTS_I64 * chunk_offset_y;
        let center_block_x = i64::from(center_chunk.0.x) * 16 + 7;
        let center_block_z = i64::from(center_chunk.0.y) * 16 + 7;
        let encode_offset_x = 31 - center_block_x;
        let encode_offset_y = -(i64::from(cached_min_section_y) * 16);
        let encode_offset_z = 31 - center_block_z;

        Self {
            center_chunk,
            range,
            cached_min_section_y,
            cached_section_count: range.section_count() + 2,
            chunk_index_offset,
            chunk_section_index_offset,
            encode_offset_x,
            encode_offset_y,
            encode_offset_z,
            encoded_min_block_x: center_block_x - 31,
            encoded_min_block_z: center_block_z - 31,
        }
    }

    /// Returns the chunk at the center of this cache window.
    #[must_use]
    pub const fn center_chunk(self) -> ChunkPos {
        self.center_chunk
    }

    /// Returns the vanilla padded light-section range.
    #[must_use]
    pub const fn range(self) -> LightSectionRange {
        self.range
    }

    /// Returns the first cached section Y coordinate, including the lower buffer.
    #[must_use]
    pub const fn cached_min_section_y(self) -> i32 {
        self.cached_min_section_y
    }

    /// Returns the section Y coordinate one past the last cached section.
    #[must_use]
    pub const fn cached_max_section_y_exclusive(self) -> i32 {
        self.cached_min_section_y + self.cached_section_count as i32
    }

    /// Returns the number of cached vertical sections, including both buffers.
    #[must_use]
    pub const fn cached_section_count(self) -> usize {
        self.cached_section_count
    }

    /// Returns the number of section/nibble slots in this cache window.
    #[must_use]
    pub const fn section_slot_count(self) -> usize {
        LIGHT_CACHE_CHUNK_SLOTS * self.cached_section_count
    }

    /// Iterates chunks in `ScalableLux` `setupCaches` scan order.
    #[must_use]
    pub const fn setup_chunks(self, radius: LightCacheSetupRadius) -> LightCacheSetupChunks {
        let remaining = radius.chunk_count();
        let radius = radius.chunk_radius();
        LightCacheSetupChunks {
            layout: self,
            radius,
            next_dx: -radius,
            next_dz: -radius,
            remaining,
        }
    }

    /// Returns cached chunk slot data for a chunk column.
    #[must_use]
    pub fn cached_chunk(self, chunk_pos: ChunkPos) -> Option<CachedLightChunk> {
        self.cached_chunk_by_coords(chunk_pos.0.x, chunk_pos.0.y)
    }

    /// Returns cached chunk slot data for chunk coordinates.
    #[must_use]
    pub fn cached_chunk_by_coords(self, chunk_x: i32, chunk_z: i32) -> Option<CachedLightChunk> {
        let dx = i64::from(chunk_x) - i64::from(self.center_chunk.0.x);
        let dz = i64::from(chunk_z) - i64::from(self.center_chunk.0.y);
        let distance = dx.abs().max(dz.abs());
        if distance > i64::from(LIGHT_CACHE_RADIUS) {
            return None;
        }

        let scope = if distance <= LIGHT_CACHE_SECTION_RADIUS_I64 {
            LightCacheChunkScope::Inner
        } else {
            LightCacheChunkScope::Outer
        };

        Some(CachedLightChunk {
            chunk_pos: ChunkPos::new(chunk_x, chunk_z),
            chunk_slot: self.chunk_slot_by_coords(chunk_x, chunk_z)?,
            scope,
        })
    }

    /// Returns the slot for a cached chunk column.
    #[must_use]
    pub fn chunk_slot(self, chunk_pos: ChunkPos) -> Option<usize> {
        self.chunk_slot_by_coords(chunk_pos.0.x, chunk_pos.0.y)
    }

    /// Returns the slot for a cached chunk column by chunk coordinates.
    #[must_use]
    pub fn chunk_slot_by_coords(self, chunk_x: i32, chunk_z: i32) -> Option<usize> {
        if !self.contains_chunk_coords(chunk_x, chunk_z) {
            return None;
        }

        let slot = i64::from(chunk_x)
            + LIGHT_CACHE_DIAMETER_I64 * i64::from(chunk_z)
            + self.chunk_index_offset;
        usize::try_from(slot).ok()
    }

    /// Converts a chunk slot back to its cached chunk position.
    #[must_use]
    pub const fn chunk_pos_for_slot(self, chunk_slot: usize) -> Option<ChunkPos> {
        if chunk_slot >= LIGHT_CACHE_CHUNK_SLOTS {
            return None;
        }

        Some(ChunkPos::new(
            self.center_chunk.0.x - LIGHT_CACHE_RADIUS + (chunk_slot % LIGHT_CACHE_DIAMETER) as i32,
            self.center_chunk.0.y - LIGHT_CACHE_RADIUS + (chunk_slot / LIGHT_CACHE_DIAMETER) as i32,
        ))
    }

    /// Returns the section/nibble slot for a section position.
    #[must_use]
    pub fn section_slot(self, section_pos: SectionPos) -> Option<usize> {
        self.section_slot_by_coords(section_pos.x(), section_pos.y(), section_pos.z())
    }

    /// Returns cached section slot data for a section position.
    #[must_use]
    pub fn cached_section(self, section_pos: SectionPos) -> Option<CachedLightSection> {
        Some(CachedLightSection {
            section_pos,
            section_slot: self.section_slot(section_pos)?,
        })
    }

    /// Returns the section/nibble slot for the section containing a block.
    #[must_use]
    pub fn section_slot_for_block(self, block_pos: BlockPos) -> Option<usize> {
        self.section_slot(SectionPos::from_block_pos(block_pos))
    }

    /// Converts a section/nibble slot back to its cached section position.
    #[must_use]
    pub const fn section_pos_for_slot(self, section_slot: usize) -> Option<SectionPos> {
        if section_slot >= self.section_slot_count() {
            return None;
        }

        let section_x = self.center_chunk.0.x - LIGHT_CACHE_RADIUS
            + (section_slot % LIGHT_CACHE_DIAMETER) as i32;
        let section_z = self.center_chunk.0.y - LIGHT_CACHE_RADIUS
            + ((section_slot / LIGHT_CACHE_DIAMETER) % LIGHT_CACHE_DIAMETER) as i32;
        let section_y = self.cached_min_section_y + (section_slot / LIGHT_CACHE_CHUNK_SLOTS) as i32;

        Some(SectionPos::new(section_x, section_y, section_z))
    }

    /// Iterates the vanilla light-section slots for an inner cached chunk.
    ///
    /// Returns `None` for the outer radius-2 ring because `ScalableLux` keeps
    /// those chunks available for chunk/emptiness lookups but does not
    /// populate section or nibble cache data for them during normal setup.
    #[must_use]
    pub fn inner_light_section_slots_for_chunk(
        self,
        chunk_pos: ChunkPos,
    ) -> Option<LightChunkSectionSlots> {
        if self.cached_chunk(chunk_pos)?.scope != LightCacheChunkScope::Inner {
            return None;
        }

        Some(LightChunkSectionSlots {
            layout: self,
            chunk_pos,
            next_section_y: self.range.min_section_y(),
            end_section_y: self.range.max_section_y_exclusive(),
        })
    }

    /// Returns cache slot data for a block position.
    #[must_use]
    pub fn cached_block(self, block_pos: BlockPos) -> Option<CachedLightBlock> {
        self.cached_block_by_coords(block_pos.x(), block_pos.y(), block_pos.z())
    }

    /// Returns cache slot data for block coordinates.
    #[must_use]
    pub fn cached_block_by_coords(
        self,
        block_x: i32,
        block_y: i32,
        block_z: i32,
    ) -> Option<CachedLightBlock> {
        let section_slot = self.section_slot_by_coords(
            SectionPos::block_to_section_coord(block_x),
            SectionPos::block_to_section_coord(block_y),
            SectionPos::block_to_section_coord(block_z),
        )?;

        Some(CachedLightBlock {
            block_pos: BlockPos::new(block_x, block_y, block_z),
            section_slot,
            local_index: Self::local_block_index_by_coords(block_x, block_y, block_z),
        })
    }

    /// Returns cache slot data for a cached block's neighboring block.
    #[must_use]
    pub fn cached_neighbor(
        self,
        cached_block: CachedLightBlock,
        direction: Direction,
    ) -> Option<CachedLightBlock> {
        let (dx, dy, dz) = direction.offset();
        self.cached_block_by_coords(
            cached_block.block_pos.x().checked_add(dx)?,
            cached_block.block_pos.y().checked_add(dy)?,
            cached_block.block_pos.z().checked_add(dz)?,
        )
    }

    /// Decodes a packed queue position and returns its cache slot data.
    #[must_use]
    pub fn cached_block_from_packed(self, packed: PackedLightBlockPos) -> Option<CachedLightBlock> {
        self.cached_block(self.decode_block_pos(packed)?)
    }

    /// Returns the local light-section index for a block position.
    #[must_use]
    pub const fn local_block_index(block_pos: BlockPos) -> usize {
        Self::local_block_index_by_coords(block_pos.x(), block_pos.y(), block_pos.z())
    }

    /// Returns the local light-section index for block coordinates.
    #[must_use]
    pub const fn local_block_index_by_coords(block_x: i32, block_y: i32, block_z: i32) -> usize {
        (block_x as usize & LIGHT_LOCAL_BLOCK_MASK)
            | ((block_z as usize & LIGHT_LOCAL_BLOCK_MASK) << LIGHT_LOCAL_BLOCK_Z_SHIFT)
            | ((block_y as usize & LIGHT_LOCAL_BLOCK_MASK) << LIGHT_LOCAL_BLOCK_Y_SHIFT)
    }

    /// Returns the first block X coordinate that can be packed into queue entries.
    #[must_use]
    pub const fn encoded_min_block_x(self) -> i32 {
        self.encoded_min_block_x as i32
    }

    /// Returns the block X coordinate one past the packed queue window.
    #[must_use]
    pub const fn encoded_max_block_x_exclusive(self) -> i32 {
        (self.encoded_min_block_x + LIGHT_ENCODED_HORIZONTAL_MASK + 1) as i32
    }

    /// Returns the first block Z coordinate that can be packed into queue entries.
    #[must_use]
    pub const fn encoded_min_block_z(self) -> i32 {
        self.encoded_min_block_z as i32
    }

    /// Returns the block Z coordinate one past the packed queue window.
    #[must_use]
    pub const fn encoded_max_block_z_exclusive(self) -> i32 {
        (self.encoded_min_block_z + LIGHT_ENCODED_HORIZONTAL_MASK + 1) as i32
    }

    /// Packs a block position for `ScalableLux` queue storage.
    #[must_use]
    pub fn encode_block_pos(self, block_pos: BlockPos) -> Option<PackedLightBlockPos> {
        if !self.contains_encoded_block_pos(block_pos) {
            return None;
        }

        let encoded_x =
            (i64::from(block_pos.x()) + self.encode_offset_x) & LIGHT_ENCODED_HORIZONTAL_MASK;
        let encoded_y =
            (i64::from(block_pos.y()) + self.encode_offset_y) & LIGHT_ENCODED_VERTICAL_MASK;
        let encoded_z =
            (i64::from(block_pos.z()) + self.encode_offset_z) & LIGHT_ENCODED_HORIZONTAL_MASK;

        Some(PackedLightBlockPos::from_raw(
            encoded_x as u32 | (encoded_z as u32) << 6 | (encoded_y as u32) << 12,
        ))
    }

    /// Decodes `ScalableLux` queue-position bits back to a world block position.
    #[must_use]
    pub fn decode_block_pos(self, packed: PackedLightBlockPos) -> Option<BlockPos> {
        let x = i64::from(packed.encoded_x()) - self.encode_offset_x;
        let y = i64::from(packed.encoded_y()) - self.encode_offset_y;
        let z = i64::from(packed.encoded_z()) - self.encode_offset_z;

        Some(BlockPos::new(
            i32::try_from(x).ok()?,
            i32::try_from(y).ok()?,
            i32::try_from(z).ok()?,
        ))
    }

    /// Returns true if a block position is inside the packed queue-coordinate window.
    #[must_use]
    pub fn contains_encoded_block_pos(self, block_pos: BlockPos) -> bool {
        let x = i64::from(block_pos.x());
        let z = i64::from(block_pos.z());
        x >= self.encoded_min_block_x
            && x <= self.encoded_min_block_x + LIGHT_ENCODED_HORIZONTAL_MASK
            && z >= self.encoded_min_block_z
            && z <= self.encoded_min_block_z + LIGHT_ENCODED_HORIZONTAL_MASK
            && self.contains_section_y(SectionPos::block_to_section_coord(block_pos.y()))
    }

    /// Returns the section/nibble slot for section coordinates.
    #[must_use]
    pub fn section_slot_by_coords(
        self,
        section_x: i32,
        section_y: i32,
        section_z: i32,
    ) -> Option<usize> {
        if !self.contains_chunk_coords(section_x, section_z) || !self.contains_section_y(section_y)
        {
            return None;
        }

        let slot = i64::from(section_x)
            + LIGHT_CACHE_DIAMETER_I64 * i64::from(section_z)
            + LIGHT_CACHE_CHUNK_SLOTS_I64 * i64::from(section_y)
            + self.chunk_section_index_offset;
        usize::try_from(slot).ok()
    }

    /// Returns the cached vertical index for a section Y coordinate.
    #[must_use]
    pub fn cached_section_index(self, section_y: i32) -> Option<usize> {
        if !self.contains_section_y(section_y) {
            return None;
        }

        usize::try_from(section_y - self.cached_min_section_y).ok()
    }

    /// Converts a cached vertical index back to section Y.
    #[must_use]
    pub const fn cached_section_y(self, index: usize) -> Option<i32> {
        if index >= self.cached_section_count {
            return None;
        }

        Some(self.cached_min_section_y + index as i32)
    }

    /// Returns true if a chunk coordinate is inside the 5x5 cache window.
    #[must_use]
    pub fn contains_chunk_coords(self, chunk_x: i32, chunk_z: i32) -> bool {
        let dx = i64::from(chunk_x) - i64::from(self.center_chunk.0.x);
        let dz = i64::from(chunk_z) - i64::from(self.center_chunk.0.y);
        dx.abs().max(dz.abs()) <= i64::from(LIGHT_CACHE_RADIUS)
    }

    /// Returns true if a chunk coordinate is inside the inner section/nibble cache radius.
    #[must_use]
    pub fn contains_inner_chunk_coords(self, chunk_x: i32, chunk_z: i32) -> bool {
        let dx = i64::from(chunk_x) - i64::from(self.center_chunk.0.x);
        let dz = i64::from(chunk_z) - i64::from(self.center_chunk.0.y);
        dx.abs().max(dz.abs()) <= LIGHT_CACHE_SECTION_RADIUS_I64
    }

    /// Returns true if a section Y is inside vanilla's padded light-section range.
    #[must_use]
    pub const fn contains_light_section_y(self, section_y: i32) -> bool {
        self.range.section_index(section_y).is_some()
    }

    /// Returns true if a section Y coordinate is inside the cached vertical range.
    #[must_use]
    pub const fn contains_section_y(self, section_y: i32) -> bool {
        section_y >= self.cached_min_section_y && section_y < self.cached_max_section_y_exclusive()
    }
}

fn sections_around_and_at_block_pos(
    block_pos: BlockPos,
    mut section_consumer: impl FnMut(SectionPos),
) {
    let min_section_x = SectionPos::block_to_section_coord(block_pos.x().wrapping_sub(1));
    let max_section_x = SectionPos::block_to_section_coord(block_pos.x().wrapping_add(1));
    let min_section_y = SectionPos::block_to_section_coord(block_pos.y().wrapping_sub(1));
    let max_section_y = SectionPos::block_to_section_coord(block_pos.y().wrapping_add(1));
    let min_section_z = SectionPos::block_to_section_coord(block_pos.z().wrapping_sub(1));
    let max_section_z = SectionPos::block_to_section_coord(block_pos.z().wrapping_add(1));

    if min_section_x == max_section_x
        && min_section_y == max_section_y
        && min_section_z == max_section_z
    {
        section_consumer(SectionPos::new(min_section_x, min_section_y, min_section_z));
        return;
    }

    for section_x in min_section_x..=max_section_x {
        for section_y in min_section_y..=max_section_y {
            for section_z in min_section_z..=max_section_z {
                section_consumer(SectionPos::new(section_x, section_y, section_z));
            }
        }
    }
}

fn empty_option_slots<T>(len: usize) -> Box<[Option<T>]> {
    let mut values = Vec::with_capacity(len);
    values.resize_with(len, || None);
    values.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min_y: i32, height: i32) -> LightSectionRange {
        let Ok(range) = LightSectionRange::from_world_height(min_y, height) else {
            panic!("test world height should create a light range");
        };
        range
    }

    #[test]
    fn cache_layout_matches_scalable_lux_chunk_indexing() {
        let layout = LightCacheLayout::new(ChunkPos::new(10, -20), range(0, 16));

        assert_eq!(layout.center_chunk(), ChunkPos::new(10, -20));
        assert_eq!(layout.range(), range(0, 16));
        assert_eq!(layout.chunk_slot(ChunkPos::new(10, -20)), Some(12));
        assert_eq!(layout.chunk_slot(ChunkPos::new(8, -22)), Some(0));
        assert_eq!(layout.chunk_slot(ChunkPos::new(12, -18)), Some(24));
        assert_eq!(layout.chunk_slot(ChunkPos::new(9, -20)), Some(11));
        assert_eq!(layout.chunk_slot(ChunkPos::new(13, -20)), None);
        assert_eq!(layout.chunk_slot(ChunkPos::new(10, -23)), None);
    }

    #[test]
    fn cache_layout_classifies_inner_and_outer_cached_chunks() {
        let layout = LightCacheLayout::new(ChunkPos::new(10, -20), range(0, 16));

        assert_eq!(
            layout.cached_chunk(ChunkPos::new(10, -20)),
            Some(CachedLightChunk {
                chunk_pos: ChunkPos::new(10, -20),
                chunk_slot: 12,
                scope: LightCacheChunkScope::Inner,
            })
        );
        assert_eq!(
            layout.cached_chunk(ChunkPos::new(9, -21)),
            Some(CachedLightChunk {
                chunk_pos: ChunkPos::new(9, -21),
                chunk_slot: 6,
                scope: LightCacheChunkScope::Inner,
            })
        );
        assert_eq!(
            layout.cached_chunk(ChunkPos::new(8, -22)),
            Some(CachedLightChunk {
                chunk_pos: ChunkPos::new(8, -22),
                chunk_slot: 0,
                scope: LightCacheChunkScope::Outer,
            })
        );
        assert_eq!(layout.cached_chunk(ChunkPos::new(13, -20)), None);

        assert!(layout.contains_inner_chunk_coords(11, -19));
        assert!(!layout.contains_inner_chunk_coords(12, -20));
        assert!(layout.contains_chunk_coords(12, -20));
    }

    #[test]
    fn cache_layout_decodes_chunk_slots() {
        let layout = LightCacheLayout::new(ChunkPos::new(10, -20), range(0, 16));

        assert_eq!(layout.chunk_pos_for_slot(0), Some(ChunkPos::new(8, -22)));
        assert_eq!(layout.chunk_pos_for_slot(12), Some(ChunkPos::new(10, -20)));
        assert_eq!(layout.chunk_pos_for_slot(24), Some(ChunkPos::new(12, -18)));
        assert_eq!(layout.chunk_pos_for_slot(25), None);
    }

    #[test]
    fn cache_layout_iterates_full_setup_chunks_in_scalable_lux_order() {
        let layout = LightCacheLayout::new(ChunkPos::new(10, -20), range(0, 16));
        let chunks = layout
            .setup_chunks(LightCacheSetupRadius::Full)
            .collect::<Vec<_>>();

        assert_eq!(chunks.len(), LIGHT_CACHE_CHUNK_SLOTS);
        assert_eq!(
            chunks.first(),
            Some(&CachedLightChunk {
                chunk_pos: ChunkPos::new(8, -22),
                chunk_slot: 0,
                scope: LightCacheChunkScope::Outer,
            })
        );
        assert_eq!(
            chunks.get(12),
            Some(&CachedLightChunk {
                chunk_pos: ChunkPos::new(10, -20),
                chunk_slot: 12,
                scope: LightCacheChunkScope::Inner,
            })
        );
        assert_eq!(
            chunks.last(),
            Some(&CachedLightChunk {
                chunk_pos: ChunkPos::new(12, -18),
                chunk_slot: 24,
                scope: LightCacheChunkScope::Outer,
            })
        );
    }

    #[test]
    fn cache_layout_adds_vertical_buffer_around_light_range() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));

        assert_eq!(layout.cached_min_section_y(), -2);
        assert_eq!(layout.cached_max_section_y_exclusive(), 3);
        assert_eq!(layout.cached_section_count(), 5);
        assert_eq!(layout.section_slot_count(), 125);

        assert_eq!(layout.cached_section_index(-2), Some(0));
        assert_eq!(layout.cached_section_index(-1), Some(1));
        assert_eq!(layout.cached_section_index(2), Some(4));
        assert_eq!(layout.cached_section_index(3), None);

        assert_eq!(layout.cached_section_y(0), Some(-2));
        assert_eq!(layout.cached_section_y(4), Some(2));
        assert_eq!(layout.cached_section_y(5), None);
    }

    #[test]
    fn cache_layout_matches_scalable_lux_section_indexing() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));

        assert_eq!(layout.section_slot_by_coords(0, -2, 0), Some(12));
        assert_eq!(layout.section_slot_by_coords(0, -1, 0), Some(37));
        assert_eq!(layout.section_slot_by_coords(0, 1, 0), Some(87));
        assert_eq!(layout.section_slot_by_coords(0, 2, 0), Some(112));
        assert_eq!(layout.section_slot_by_coords(0, 3, 0), None);
    }

    #[test]
    fn cache_layout_iterates_inner_chunk_light_section_slots() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));

        let Some(slots) = layout.inner_light_section_slots_for_chunk(ChunkPos::new(1, 0)) else {
            panic!("inner chunk should have light section slots");
        };
        assert_eq!(slots.len(), 3);
        assert_eq!(
            slots.collect::<Vec<_>>(),
            vec![
                CachedLightSection {
                    section_pos: SectionPos::new(1, -1, 0),
                    section_slot: 38,
                },
                CachedLightSection {
                    section_pos: SectionPos::new(1, 0, 0),
                    section_slot: 63,
                },
                CachedLightSection {
                    section_pos: SectionPos::new(1, 1, 0),
                    section_slot: 88,
                },
            ]
        );

        assert!(layout.contains_light_section_y(-1));
        assert!(layout.contains_light_section_y(1));
        assert!(!layout.contains_light_section_y(-2));
        assert!(!layout.contains_light_section_y(2));
    }

    #[test]
    fn cache_layout_uses_scalable_lux_local_block_indices() {
        assert_eq!(
            LightCacheLayout::local_block_index(BlockPos::new(0, 0, 0)),
            0
        );
        assert_eq!(
            LightCacheLayout::local_block_index(BlockPos::new(15, 15, 15)),
            15 | (15 << 4) | (15 << 8)
        );
        assert_eq!(
            LightCacheLayout::local_block_index(BlockPos::new(-1, -1, -1)),
            15 | (15 << 4) | (15 << 8)
        );
        assert_eq!(
            LightCacheLayout::local_block_index(BlockPos::new(16, 16, 16)),
            0
        );
    }

    #[test]
    fn cache_layout_maps_block_positions_to_cached_blocks() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));

        assert_eq!(
            layout.cached_block(BlockPos::new(31, 0, -32)),
            Some(CachedLightBlock {
                block_pos: BlockPos::new(31, 0, -32),
                section_slot: 53,
                local_index: 15,
            })
        );
        assert_eq!(
            layout.cached_block(BlockPos::new(-1, -1, -1)),
            Some(CachedLightBlock {
                block_pos: BlockPos::new(-1, -1, -1),
                section_slot: 31,
                local_index: 4095,
            })
        );
        assert_eq!(layout.cached_block(BlockPos::new(48, 0, 0)), None);
    }

    #[test]
    fn cache_layout_maps_cached_neighbors_across_section_edges() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));
        let Some(block) = layout.cached_block(BlockPos::new(15, 15, 15)) else {
            panic!("test block should be cached");
        };

        assert_eq!(
            layout.cached_neighbor(block, Direction::East),
            Some(CachedLightBlock {
                block_pos: BlockPos::new(16, 15, 15),
                section_slot: 63,
                local_index: (15 << 4) | (15 << 8),
            })
        );
        assert_eq!(
            layout.cached_neighbor(block, Direction::Up),
            Some(CachedLightBlock {
                block_pos: BlockPos::new(15, 16, 15),
                section_slot: 87,
                local_index: 15 | (15 << 4),
            })
        );
    }

    #[test]
    fn packed_light_block_pos_masks_to_scalable_lux_position_bits() {
        let packed = PackedLightBlockPos::from_raw(u32::MAX);

        assert_eq!(packed.raw(), (1 << 28) - 1);
        assert_eq!(packed.encoded_x(), 63);
        assert_eq!(packed.encoded_z(), 63);
        assert_eq!(packed.encoded_y(), u16::MAX);
    }

    #[test]
    fn cache_layout_encodes_scalable_lux_queue_position_window() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));

        assert_eq!(layout.encoded_min_block_x(), -24);
        assert_eq!(layout.encoded_max_block_x_exclusive(), 40);
        assert_eq!(layout.encoded_min_block_z(), -24);
        assert_eq!(layout.encoded_max_block_z_exclusive(), 40);

        let Some(center) = layout.encode_block_pos(BlockPos::new(7, 0, 7)) else {
            panic!("center chunk block should encode");
        };
        assert_eq!(center.encoded_x(), 31);
        assert_eq!(center.encoded_z(), 31);
        assert_eq!(center.encoded_y(), 32);
        assert_eq!(
            layout.decode_block_pos(center),
            Some(BlockPos::new(7, 0, 7))
        );

        let Some(min) = layout.encode_block_pos(BlockPos::new(-24, -32, -24)) else {
            panic!("minimum queue block should encode");
        };
        assert_eq!(min.raw(), 0);
        assert_eq!(
            layout.decode_block_pos(min),
            Some(BlockPos::new(-24, -32, -24))
        );

        let Some(max) = layout.encode_block_pos(BlockPos::new(39, 47, 39)) else {
            panic!("maximum queue block should encode");
        };
        assert_eq!(max.encoded_x(), 63);
        assert_eq!(max.encoded_z(), 63);
        assert_eq!(max.encoded_y(), 79);
        assert_eq!(
            layout.decode_block_pos(max),
            Some(BlockPos::new(39, 47, 39))
        );

        assert_eq!(layout.encode_block_pos(BlockPos::new(40, 0, 0)), None);
        assert_eq!(layout.encode_block_pos(BlockPos::new(0, 0, 40)), None);
        assert_eq!(layout.encode_block_pos(BlockPos::new(0, 48, 0)), None);
    }

    #[test]
    fn cache_layout_maps_packed_positions_to_cached_blocks() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));
        let Some(packed) = layout.encode_block_pos(BlockPos::new(7, 0, 7)) else {
            panic!("center block should encode");
        };

        assert_eq!(
            layout.cached_block_from_packed(packed),
            Some(CachedLightBlock {
                block_pos: BlockPos::new(7, 0, 7),
                section_slot: 62,
                local_index: 7 | (7 << 4),
            })
        );
        assert_eq!(
            layout.cached_block_from_packed(PackedLightBlockPos::from_raw(u32::MAX)),
            None
        );
    }

    #[test]
    fn slot_arrays_store_values_by_cached_slots() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));
        let Some(chunk) = layout.cached_chunk(ChunkPos::new(0, 0)) else {
            panic!("center chunk should be cached");
        };
        let Some(section) = layout.cached_section(SectionPos::new(0, 0, 0)) else {
            panic!("section should be cached");
        };
        let mut chunks = LightChunkSlotArray::new();
        let mut sections = LightSectionSlotArray::new(layout);

        assert_eq!(chunks.slot_count(), LIGHT_CACHE_CHUNK_SLOTS);
        assert!(chunks.is_clear());
        assert_eq!(chunks.insert(chunk, 5), None);
        assert_eq!(chunks.get(chunk), Some(&5));
        assert_eq!(chunks.insert_slot(chunk.chunk_slot, 11), Some(5));
        assert_eq!(chunks.get_mut(chunk), Some(&mut 11));
        assert_eq!(chunks.insert_slot(LIGHT_CACHE_CHUNK_SLOTS, 4), None);

        assert_eq!(sections.layout(), layout);
        assert_eq!(sections.slot_count(), layout.section_slot_count());
        assert!(sections.is_clear());
        assert_eq!(sections.insert(section, "sky"), None);
        assert_eq!(sections.get(section), Some(&"sky"));
        assert_eq!(
            sections.insert_slot(section.section_slot, "block"),
            Some("sky")
        );
        assert_eq!(sections.take_slot(section.section_slot), Some("block"));
        assert_eq!(sections.get_slot(section.section_slot), None);

        chunks.clear();
        sections.clear();
        assert!(chunks.is_clear());
        assert!(sections.is_clear());
    }

    #[test]
    fn notification_cache_marks_one_block_light_neighborhood() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));
        let mut notifications = LightUpdateNotificationCache::new(layout);

        assert_eq!(notifications.layout(), layout);
        assert!(notifications.is_empty());
        assert_eq!(
            notifications.mark_block_neighborhood(BlockPos::new(8, 8, 8)),
            Some(1)
        );
        assert_eq!(
            notifications.marked_section_positions().collect::<Vec<_>>(),
            vec![SectionPos::new(0, 0, 0)]
        );
        assert!(notifications.is_marked_section(SectionPos::new(0, 0, 0)));
        assert!(notifications.is_marked_section_slot(62));

        notifications.clear();
        assert_eq!(
            notifications.mark_block_neighborhood(BlockPos::new(16, 16, 16)),
            Some(8)
        );

        let marked = notifications.marked_section_positions().collect::<Vec<_>>();
        assert_eq!(marked.len(), 8);
        assert!(marked.contains(&SectionPos::new(0, 0, 0)));
        assert!(marked.contains(&SectionPos::new(1, 0, 0)));
        assert!(marked.contains(&SectionPos::new(0, 1, 0)));
        assert!(marked.contains(&SectionPos::new(1, 1, 1)));
    }

    #[test]
    fn notification_cache_rejects_partial_block_neighborhoods() {
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range(0, 16));
        let mut notifications = LightUpdateNotificationCache::new(layout);

        assert_eq!(
            notifications.mark_block_neighborhood(BlockPos::new(48, 8, 8)),
            None
        );
        assert!(notifications.is_empty());
    }
}
