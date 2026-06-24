use std::iter::FusedIterator;

use steel_utils::{BlockPos, Direction};

use super::{MAX_LIGHT_LEVEL, PackedLightBlockPos};

const QUEUE_ENTRY_LEVEL_MASK: u64 = 0b1111;
const QUEUE_ENTRY_DIRECTIONS_MASK: u64 = 0b11_1111_0000;
const QUEUE_ENTRY_FLAG_FROM_EMPTY_SHAPE: u64 = 1 << 10;
const QUEUE_ENTRY_FLAG_INCREASE_FROM_EMISSION: u64 = 1 << 11;
const LIGHT_QUEUE_MIN_CAPACITY: usize = 512;
const PACKED_LIGHT_QUEUE_MIN_CAPACITY: usize = 16 * 16 * 16;
const PACKED_LIGHT_QUEUE_POSITION_BITS: u64 = 28;
const PACKED_LIGHT_QUEUE_LEVEL_BITS: u64 = 4;
const PACKED_LIGHT_QUEUE_DIRECTION_BITS: u64 = 6;
const PACKED_LIGHT_QUEUE_LEVEL_MASK: u64 = (1 << PACKED_LIGHT_QUEUE_LEVEL_BITS) - 1;
const PACKED_LIGHT_QUEUE_DIRECTION_MASK: u8 = (1 << PACKED_LIGHT_QUEUE_DIRECTION_BITS) - 1;
const PACKED_LIGHT_QUEUE_LEVEL_SHIFT: u64 = PACKED_LIGHT_QUEUE_POSITION_BITS;
const PACKED_LIGHT_QUEUE_DIRECTIONS_SHIFT: u64 =
    PACKED_LIGHT_QUEUE_LEVEL_SHIFT + PACKED_LIGHT_QUEUE_LEVEL_BITS;
const PACKED_LIGHT_QUEUE_POSITION_MASK: u64 = (1_u64 << PACKED_LIGHT_QUEUE_POSITION_BITS) - 1;
const PACKED_LIGHT_QUEUE_FLAGS_MASK: u64 = (1_u64 << 61) | (1_u64 << 62) | (1_u64 << 63);

/// `ScalableLux` axis direction order used by packed light propagation queues.
///
/// This intentionally differs from vanilla's `Direction.ordinal()` order.
/// `ScalableLux` stores direction bitsets as +X, -X, +Z, -Z, +Y, -Y and relies on
/// positive directions being even so `index ^ 1` gives the opposite direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightAxisDirection {
    /// Positive X / east.
    PositiveX,
    /// Negative X / west.
    NegativeX,
    /// Positive Z / south.
    PositiveZ,
    /// Negative Z / north.
    NegativeZ,
    /// Positive Y / up.
    PositiveY,
    /// Negative Y / down.
    NegativeY,
}

impl LightAxisDirection {
    /// All directions in `ScalableLux` propagation order.
    pub const ALL: [Self; 6] = [
        Self::PositiveX,
        Self::NegativeX,
        Self::PositiveZ,
        Self::NegativeZ,
        Self::PositiveY,
        Self::NegativeY,
    ];

    /// Horizontal directions in `ScalableLux` propagation order.
    pub const HORIZONTAL: [Self; 4] = [
        Self::PositiveX,
        Self::NegativeX,
        Self::PositiveZ,
        Self::NegativeZ,
    ];

    /// Converts a vanilla direction into `ScalableLux`'s axis-direction order.
    #[must_use]
    pub const fn from_direction(direction: Direction) -> Self {
        match direction {
            Direction::East => Self::PositiveX,
            Direction::West => Self::NegativeX,
            Direction::South => Self::PositiveZ,
            Direction::North => Self::NegativeZ,
            Direction::Up => Self::PositiveY,
            Direction::Down => Self::NegativeY,
        }
    }

    /// Returns the `ScalableLux` axis direction for a direction-bit index.
    #[must_use]
    pub const fn from_bit_index(bit_index: u8) -> Option<Self> {
        match bit_index {
            0 => Some(Self::PositiveX),
            1 => Some(Self::NegativeX),
            2 => Some(Self::PositiveZ),
            3 => Some(Self::NegativeZ),
            4 => Some(Self::PositiveY),
            5 => Some(Self::NegativeY),
            _ => None,
        }
    }

    /// Returns the vanilla direction represented by this axis direction.
    #[must_use]
    pub const fn direction(self) -> Direction {
        match self {
            Self::PositiveX => Direction::East,
            Self::NegativeX => Direction::West,
            Self::PositiveZ => Direction::South,
            Self::NegativeZ => Direction::North,
            Self::PositiveY => Direction::Up,
            Self::NegativeY => Direction::Down,
        }
    }

    /// Returns the block-coordinate offset for this axis direction.
    #[must_use]
    pub const fn offset(self) -> (i32, i32, i32) {
        match self {
            Self::PositiveX => (1, 0, 0),
            Self::NegativeX => (-1, 0, 0),
            Self::PositiveZ => (0, 0, 1),
            Self::NegativeZ => (0, 0, -1),
            Self::PositiveY => (0, 1, 0),
            Self::NegativeY => (0, -1, 0),
        }
    }

    /// Returns the opposite `ScalableLux` axis direction.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::PositiveX => Self::NegativeX,
            Self::NegativeX => Self::PositiveX,
            Self::PositiveZ => Self::NegativeZ,
            Self::NegativeZ => Self::PositiveZ,
            Self::PositiveY => Self::NegativeY,
            Self::NegativeY => Self::PositiveY,
        }
    }

    /// Returns this direction's `ScalableLux` bit index.
    #[must_use]
    pub const fn bit_index(self) -> u8 {
        match self {
            Self::PositiveX => 0,
            Self::NegativeX => 1,
            Self::PositiveZ => 2,
            Self::NegativeZ => 3,
            Self::PositiveY => 4,
            Self::NegativeY => 5,
        }
    }

    const fn bit(self) -> u8 {
        1 << self.bit_index()
    }
}

/// `ScalableLux` propagation direction bitset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LightDirectionSet(u8);

impl LightDirectionSet {
    /// Creates an empty direction set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates a direction set containing all six axis directions.
    #[must_use]
    pub const fn all() -> Self {
        Self(PACKED_LIGHT_QUEUE_DIRECTION_MASK)
    }

    /// Creates a direction set from raw `ScalableLux` direction bits.
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        Self(raw & PACKED_LIGHT_QUEUE_DIRECTION_MASK)
    }

    /// Creates a direction set containing exactly one axis direction.
    #[must_use]
    pub const fn only(direction: LightAxisDirection) -> Self {
        Self(direction.bit())
    }

    /// Returns this set with one additional direction.
    #[must_use]
    pub const fn with(self, direction: LightAxisDirection) -> Self {
        Self(self.0 | direction.bit())
    }

    /// Creates a direction set containing all directions except one.
    #[must_use]
    pub const fn all_except(direction: LightAxisDirection) -> Self {
        Self(PACKED_LIGHT_QUEUE_DIRECTION_MASK & !direction.bit())
    }

    /// Creates a direction set containing all directions except the opposite of one direction.
    #[must_use]
    pub const fn all_except_opposite(direction: LightAxisDirection) -> Self {
        Self::all_except(direction.opposite())
    }

    /// Returns the raw `ScalableLux` direction bits.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    /// Returns true when this set contains the selected axis direction.
    #[must_use]
    pub const fn contains(self, direction: LightAxisDirection) -> bool {
        self.0 & direction.bit() != 0
    }

    /// Iterates selected directions in `ScalableLux`'s propagation order.
    #[must_use]
    pub const fn directions(self) -> LightDirectionSetIter {
        LightDirectionSetIter { remaining: self.0 }
    }
}

/// Iterator over a `LightDirectionSet` in `ScalableLux` propagation order.
#[derive(Debug, Clone)]
pub struct LightDirectionSetIter {
    remaining: u8,
}

impl Iterator for LightDirectionSetIter {
    type Item = LightAxisDirection;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let bit_index = self.remaining.trailing_zeros() as u8;
        self.remaining &= self.remaining - 1;
        LightAxisDirection::from_bit_index(bit_index)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.remaining.count_ones() as usize;
        (len, Some(len))
    }
}

impl ExactSizeIterator for LightDirectionSetIter {}

impl FusedIterator for LightDirectionSetIter {}

/// `ScalableLux` state flags stored in the top three queue-entry bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LightQueueFlags(u64);

impl LightQueueFlags {
    /// No queue state flags.
    pub const EMPTY: Self = Self(0);
    /// The increase pass should write the entry's level before propagating.
    pub const WRITE_LEVEL: Self = Self(1_u64 << 61);
    /// The increase pass should confirm the current level still matches.
    pub const RECHECK_LEVEL: Self = Self(1_u64 << 62);
    /// Propagation must account for sided transparent block shapes.
    pub const HAS_SIDED_TRANSPARENT_BLOCKS: Self = Self(1_u64 << 63);

    /// Creates flags from raw queue bits.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw & PACKED_LIGHT_QUEUE_FLAGS_MASK)
    }

    /// Returns the raw queue flag bits.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Returns a set with `flag` included.
    #[must_use]
    pub const fn with(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }

    /// Returns true when all bits in `flag` are present.
    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
}

/// `ScalableLux` packed light-propagation queue entry.
///
/// The lower 28 bits store `PackedLightBlockPos`, followed by a 4-bit light
/// level and a 6-bit `LightDirectionSet`. Bits 61, 62, and 63 carry
/// `LightQueueFlags`; the middle 23 bits are intentionally unused to preserve
/// `ScalableLux`'s layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedLightQueueEntry(u64);

impl PackedLightQueueEntry {
    /// Creates a packed queue entry from typed `ScalableLux` parts.
    #[must_use]
    pub const fn from_parts(
        block_pos: PackedLightBlockPos,
        level: u8,
        directions: LightDirectionSet,
        flags: LightQueueFlags,
    ) -> Self {
        Self(
            block_pos.raw() as u64
                | ((level as u64 & PACKED_LIGHT_QUEUE_LEVEL_MASK)
                    << PACKED_LIGHT_QUEUE_LEVEL_SHIFT)
                | ((directions.raw() as u64) << PACKED_LIGHT_QUEUE_DIRECTIONS_SHIFT)
                | flags.raw(),
        )
    }

    /// Creates a packed queue entry from raw `ScalableLux` queue bits.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw `ScalableLux` queue entry.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Returns the packed block position stored in this queue entry.
    #[must_use]
    pub const fn block_pos(self) -> PackedLightBlockPos {
        PackedLightBlockPos::from_raw((self.0 & PACKED_LIGHT_QUEUE_POSITION_MASK) as u32)
    }

    /// Returns the propagated light level.
    #[must_use]
    pub const fn level(self) -> u8 {
        ((self.0 >> PACKED_LIGHT_QUEUE_LEVEL_SHIFT) & PACKED_LIGHT_QUEUE_LEVEL_MASK) as u8
    }

    /// Returns the propagation direction set.
    #[must_use]
    pub const fn directions(self) -> LightDirectionSet {
        LightDirectionSet::from_raw(
            ((self.0 >> PACKED_LIGHT_QUEUE_DIRECTIONS_SHIFT)
                & PACKED_LIGHT_QUEUE_DIRECTION_MASK as u64) as u8,
        )
    }

    /// Returns the top-bit state flags.
    #[must_use]
    pub const fn flags(self) -> LightQueueFlags {
        LightQueueFlags::from_raw(self.0)
    }

    /// Returns true when the increase pass should write this entry's level.
    #[must_use]
    pub const fn should_write_level(self) -> bool {
        self.flags().contains(LightQueueFlags::WRITE_LEVEL)
    }

    /// Returns true when the increase pass should confirm the current level.
    #[must_use]
    pub const fn should_recheck_level(self) -> bool {
        self.flags().contains(LightQueueFlags::RECHECK_LEVEL)
    }

    /// Returns true when propagation must account for sided transparent shapes.
    #[must_use]
    pub const fn has_sided_transparent_blocks(self) -> bool {
        self.flags()
            .contains(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS)
    }
}

/// Array-backed FIFO used for `ScalableLux` packed light propagation entries.
#[derive(Debug)]
pub struct PackedLightPropagationQueue {
    entries: Vec<PackedLightQueueEntry>,
    read_index: usize,
}

impl PackedLightPropagationQueue {
    /// Creates an empty `ScalableLux` packed propagation queue.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::with_capacity(PACKED_LIGHT_QUEUE_MIN_CAPACITY),
            read_index: 0,
        }
    }

    /// Returns true when no packed queued work remains.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.read_index >= self.entries.len()
    }

    /// Returns the number of packed entries that have not been dequeued yet.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len() - self.read_index
    }

    /// Adds packed propagation work to the back of the queue.
    pub fn enqueue(&mut self, entry: PackedLightQueueEntry) {
        self.entries.push(entry);
    }

    /// Removes packed propagation work from the front of the queue.
    pub fn dequeue(&mut self) -> Option<PackedLightQueueEntry> {
        if self.is_empty() {
            self.clear();
            return None;
        }

        let entry = self.entries[self.read_index];
        self.read_index += 1;
        if self.is_empty() {
            self.clear();
        }

        Some(entry)
    }

    /// Removes all queued packed work while keeping allocated storage for reuse.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.read_index = 0;
    }
}

impl Default for PackedLightPropagationQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// `ScalableLux`'s separate packed increase and decrease propagation queues.
#[derive(Debug, Default)]
pub struct PackedLightPropagationQueues {
    increase: PackedLightPropagationQueue,
    decrease: PackedLightPropagationQueue,
}

impl PackedLightPropagationQueues {
    /// Creates empty packed increase and decrease queues.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when either packed propagation queue contains work.
    #[must_use]
    pub const fn has_work(&self) -> bool {
        !self.increase.is_empty() || !self.decrease.is_empty()
    }

    /// Enqueues packed decrease propagation work.
    pub fn enqueue_decrease(&mut self, entry: PackedLightQueueEntry) {
        self.decrease.enqueue(entry);
    }

    /// Enqueues packed increase propagation work.
    pub fn enqueue_increase(&mut self, entry: PackedLightQueueEntry) {
        self.increase.enqueue(entry);
    }

    /// Dequeues packed decrease propagation work.
    pub fn dequeue_decrease(&mut self) -> Option<PackedLightQueueEntry> {
        self.decrease.dequeue()
    }

    /// Dequeues packed increase propagation work.
    pub fn dequeue_increase(&mut self) -> Option<PackedLightQueueEntry> {
        self.increase.dequeue()
    }

    /// Removes all packed increase and decrease work.
    pub fn clear(&mut self) {
        self.increase.clear();
        self.decrease.clear();
    }
}

/// Vanilla's packed light-propagation queue entry.
///
/// `LightEngine.QueueEntry` stores the source level in bits 0..3, one
/// propagation bit per vanilla `Direction.ordinal()` in bits 4..9, and two
/// increase flags in bits 10 and 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightQueueEntry(u64);

impl LightQueueEntry {
    /// Creates a decrease entry that propagates to all directions except one.
    #[must_use]
    pub const fn decrease_skip_one_direction(
        old_from_level: u8,
        skip_direction: Direction,
    ) -> Self {
        Self::with_level(
            Self::without_direction(QUEUE_ENTRY_DIRECTIONS_MASK, skip_direction),
            old_from_level,
        )
    }

    /// Creates a decrease entry that propagates to all directions.
    #[must_use]
    pub const fn decrease_all_directions(old_from_level: u8) -> Self {
        Self::with_level(QUEUE_ENTRY_DIRECTIONS_MASK, old_from_level)
    }

    /// Creates an increase entry sourced from a block's light emission.
    #[must_use]
    pub const fn increase_light_from_emission(new_from_level: u8, from_empty_shape: bool) -> Self {
        let mut entry = QUEUE_ENTRY_DIRECTIONS_MASK | QUEUE_ENTRY_FLAG_INCREASE_FROM_EMISSION;
        if from_empty_shape {
            entry |= QUEUE_ENTRY_FLAG_FROM_EMPTY_SHAPE;
        }

        Self::with_level(entry, new_from_level)
    }

    /// Creates an increase entry that propagates to all directions except one.
    #[must_use]
    pub const fn increase_skip_one_direction(
        new_from_level: u8,
        from_empty_shape: bool,
        skip_direction: Direction,
    ) -> Self {
        let mut entry = Self::without_direction(QUEUE_ENTRY_DIRECTIONS_MASK, skip_direction);
        if from_empty_shape {
            entry |= QUEUE_ENTRY_FLAG_FROM_EMPTY_SHAPE;
        }

        Self::with_level(entry, new_from_level)
    }

    /// Creates an increase entry that propagates to exactly one direction.
    #[must_use]
    pub const fn increase_only_one_direction(
        new_from_level: u8,
        from_empty_shape: bool,
        direction: Direction,
    ) -> Self {
        let mut entry = 0;
        if from_empty_shape {
            entry |= QUEUE_ENTRY_FLAG_FROM_EMPTY_SHAPE;
        }

        Self::with_level(Self::with_direction(entry, direction), new_from_level)
    }

    /// Creates a sky-source increase entry for selected directions.
    #[must_use]
    pub fn increase_sky_source_in_directions(directions: &[Direction]) -> Self {
        let mut entry = u64::from(MAX_LIGHT_LEVEL);
        for &direction in directions {
            entry = Self::with_direction(entry, direction);
        }

        Self(entry)
    }

    /// Creates a queue entry from vanilla's packed representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns vanilla's packed representation.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Returns the source light level stored in this entry.
    #[must_use]
    pub const fn level(self) -> u8 {
        (self.0 & QUEUE_ENTRY_LEVEL_MASK) as u8
    }

    /// Returns true if propagation starts from an empty occlusion shape.
    #[must_use]
    pub const fn is_from_empty_shape(self) -> bool {
        self.0 & QUEUE_ENTRY_FLAG_FROM_EMPTY_SHAPE != 0
    }

    /// Returns true if this increase came from block light emission.
    #[must_use]
    pub const fn is_increase_from_emission(self) -> bool {
        self.0 & QUEUE_ENTRY_FLAG_INCREASE_FROM_EMISSION != 0
    }

    /// Returns true if this entry propagates in `direction`.
    #[must_use]
    pub const fn should_propagate_in_direction(self, direction: Direction) -> bool {
        self.0 & Self::direction_bit(direction) != 0
    }

    const fn with_level(entry: u64, level: u8) -> Self {
        Self(entry & !QUEUE_ENTRY_LEVEL_MASK | (level as u64 & QUEUE_ENTRY_LEVEL_MASK))
    }

    const fn with_direction(entry: u64, direction: Direction) -> u64 {
        entry | Self::direction_bit(direction)
    }

    const fn without_direction(entry: u64, direction: Direction) -> u64 {
        entry & !Self::direction_bit(direction)
    }

    const fn direction_bit(direction: Direction) -> u64 {
        1 << (Self::vanilla_direction_index(direction) + 4)
    }

    const fn vanilla_direction_index(direction: Direction) -> u64 {
        match direction {
            Direction::Down => 0,
            Direction::Up => 1,
            Direction::North => 2,
            Direction::South => 3,
            Direction::West => 4,
            Direction::East => 5,
        }
    }
}

/// One typed light propagation queue item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueuedLightUpdate {
    /// Block position whose light should propagate.
    pub block_pos: BlockPos,
    /// Packed vanilla propagation metadata.
    pub entry: LightQueueEntry,
}

/// Array-backed FIFO used for vanilla light propagation work.
///
/// Vanilla stores alternating packed block positions and `QueueEntry` longs in
/// `LongArrayFIFOQueue`. Steel keeps typed records instead, while preserving
/// the FIFO ordering and packed queue-entry semantics that propagation depends
/// on.
#[derive(Debug)]
pub struct LightPropagationQueue {
    entries: Vec<QueuedLightUpdate>,
    read_index: usize,
}

impl LightPropagationQueue {
    /// Creates an empty propagation queue.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::with_capacity(LIGHT_QUEUE_MIN_CAPACITY),
            read_index: 0,
        }
    }

    /// Returns true when no queued work remains.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.read_index >= self.entries.len()
    }

    /// Returns the number of queued items that have not been dequeued yet.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len() - self.read_index
    }

    /// Adds propagation work to the back of the queue.
    pub fn enqueue(&mut self, block_pos: BlockPos, entry: LightQueueEntry) {
        self.entries.push(QueuedLightUpdate { block_pos, entry });
    }

    /// Removes propagation work from the front of the queue.
    pub fn dequeue(&mut self) -> Option<QueuedLightUpdate> {
        if self.is_empty() {
            self.clear();
            return None;
        }

        let update = self.entries[self.read_index];
        self.read_index += 1;
        if self.is_empty() {
            self.clear();
        }

        Some(update)
    }

    /// Removes all queued work while keeping allocated storage for reuse.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.read_index = 0;
    }
}

impl Default for LightPropagationQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla's separate increase and decrease propagation queues.
#[derive(Debug, Default)]
pub struct LightPropagationQueues {
    increase: LightPropagationQueue,
    decrease: LightPropagationQueue,
}

impl LightPropagationQueues {
    /// Creates empty increase and decrease queues.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when either propagation queue contains work.
    #[must_use]
    pub const fn has_work(&self) -> bool {
        !self.increase.is_empty() || !self.decrease.is_empty()
    }

    /// Enqueues decrease propagation work.
    pub fn enqueue_decrease(&mut self, block_pos: BlockPos, entry: LightQueueEntry) {
        self.decrease.enqueue(block_pos, entry);
    }

    /// Enqueues increase propagation work.
    pub fn enqueue_increase(&mut self, block_pos: BlockPos, entry: LightQueueEntry) {
        self.increase.enqueue(block_pos, entry);
    }

    /// Dequeues decrease propagation work.
    pub fn dequeue_decrease(&mut self) -> Option<QueuedLightUpdate> {
        self.decrease.dequeue()
    }

    /// Dequeues increase propagation work.
    pub fn dequeue_increase(&mut self) -> Option<QueuedLightUpdate> {
        self.increase.dequeue()
    }

    /// Removes all increase and decrease work.
    pub fn clear(&mut self) {
        self.increase.clear();
        self.decrease.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed_entry(level: u8) -> PackedLightQueueEntry {
        PackedLightQueueEntry::from_parts(
            PackedLightBlockPos::from_raw(u32::from(level)),
            level,
            LightDirectionSet::all(),
            LightQueueFlags::EMPTY,
        )
    }

    #[test]
    fn light_axis_direction_matches_scalable_lux_order() {
        assert_eq!(
            LightAxisDirection::ALL,
            [
                LightAxisDirection::PositiveX,
                LightAxisDirection::NegativeX,
                LightAxisDirection::PositiveZ,
                LightAxisDirection::NegativeZ,
                LightAxisDirection::PositiveY,
                LightAxisDirection::NegativeY,
            ]
        );
        assert_eq!(
            LightAxisDirection::HORIZONTAL,
            [
                LightAxisDirection::PositiveX,
                LightAxisDirection::NegativeX,
                LightAxisDirection::PositiveZ,
                LightAxisDirection::NegativeZ,
            ]
        );

        assert_eq!(LightAxisDirection::PositiveX.bit_index(), 0);
        assert_eq!(LightAxisDirection::NegativeX.bit_index(), 1);
        assert_eq!(LightAxisDirection::PositiveZ.bit_index(), 2);
        assert_eq!(LightAxisDirection::NegativeZ.bit_index(), 3);
        assert_eq!(LightAxisDirection::PositiveY.bit_index(), 4);
        assert_eq!(LightAxisDirection::NegativeY.bit_index(), 5);
    }

    #[test]
    fn light_axis_direction_maps_to_steel_direction() {
        assert_eq!(
            LightAxisDirection::from_direction(Direction::East),
            LightAxisDirection::PositiveX
        );
        assert_eq!(
            LightAxisDirection::from_direction(Direction::West),
            LightAxisDirection::NegativeX
        );
        assert_eq!(
            LightAxisDirection::from_direction(Direction::South),
            LightAxisDirection::PositiveZ
        );
        assert_eq!(
            LightAxisDirection::from_direction(Direction::North),
            LightAxisDirection::NegativeZ
        );
        assert_eq!(
            LightAxisDirection::from_direction(Direction::Up),
            LightAxisDirection::PositiveY
        );
        assert_eq!(
            LightAxisDirection::from_direction(Direction::Down),
            LightAxisDirection::NegativeY
        );

        assert_eq!(LightAxisDirection::PositiveX.direction(), Direction::East);
        assert_eq!(LightAxisDirection::NegativeX.direction(), Direction::West);
        assert_eq!(LightAxisDirection::PositiveZ.direction(), Direction::South);
        assert_eq!(LightAxisDirection::NegativeZ.direction(), Direction::North);
        assert_eq!(LightAxisDirection::PositiveY.direction(), Direction::Up);
        assert_eq!(LightAxisDirection::NegativeY.direction(), Direction::Down);

        assert_eq!(LightAxisDirection::PositiveX.offset(), (1, 0, 0));
        assert_eq!(LightAxisDirection::NegativeX.offset(), (-1, 0, 0));
        assert_eq!(LightAxisDirection::PositiveZ.offset(), (0, 0, 1));
        assert_eq!(LightAxisDirection::NegativeZ.offset(), (0, 0, -1));
        assert_eq!(LightAxisDirection::PositiveY.offset(), (0, 1, 0));
        assert_eq!(LightAxisDirection::NegativeY.offset(), (0, -1, 0));
    }

    #[test]
    fn light_axis_direction_opposites_flip_low_bit() {
        for direction in LightAxisDirection::ALL {
            assert_eq!(direction.opposite().bit_index(), direction.bit_index() ^ 1);
            assert_eq!(direction.opposite().opposite(), direction);
        }
    }

    #[test]
    fn light_direction_set_matches_scalable_lux_masks() {
        assert_eq!(LightDirectionSet::empty().raw(), 0);
        assert_eq!(LightDirectionSet::all().raw(), 0b11_1111);
        assert_eq!(LightDirectionSet::from_raw(u8::MAX).raw(), 0b11_1111);
        assert_eq!(
            LightDirectionSet::only(LightAxisDirection::PositiveZ).raw(),
            0b00_0100
        );
        assert_eq!(
            LightDirectionSet::all_except(LightAxisDirection::PositiveZ).raw(),
            0b11_1011
        );
        assert_eq!(
            LightDirectionSet::all_except_opposite(LightAxisDirection::PositiveZ).raw(),
            0b11_0111
        );

        let set = LightDirectionSet::from_raw(0b10_0101);
        assert!(set.contains(LightAxisDirection::PositiveX));
        assert!(set.contains(LightAxisDirection::PositiveZ));
        assert!(set.contains(LightAxisDirection::NegativeY));
        assert!(!set.contains(LightAxisDirection::NegativeX));
        assert!(!set.contains(LightAxisDirection::NegativeZ));
        assert!(!set.contains(LightAxisDirection::PositiveY));
    }

    #[test]
    fn light_direction_set_iterates_in_scalable_lux_order() {
        let mut directions = LightDirectionSet::from_raw(0b10_1101).directions();

        assert_eq!(directions.len(), 4);
        assert_eq!(directions.next(), Some(LightAxisDirection::PositiveX));
        assert_eq!(directions.next(), Some(LightAxisDirection::PositiveZ));
        assert_eq!(directions.len(), 2);
        assert_eq!(directions.next(), Some(LightAxisDirection::NegativeZ));
        assert_eq!(directions.next(), Some(LightAxisDirection::NegativeY));
        assert_eq!(directions.next(), None);
        assert_eq!(directions.next(), None);
    }

    #[test]
    fn light_queue_flags_match_scalable_lux_top_bits() {
        assert_eq!(LightQueueFlags::WRITE_LEVEL.raw(), 1_u64 << 61);
        assert_eq!(LightQueueFlags::RECHECK_LEVEL.raw(), 1_u64 << 62);
        assert_eq!(
            LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS.raw(),
            1_u64 << 63
        );

        let flags = LightQueueFlags::EMPTY
            .with(LightQueueFlags::WRITE_LEVEL)
            .with(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS);
        assert!(flags.contains(LightQueueFlags::WRITE_LEVEL));
        assert!(!flags.contains(LightQueueFlags::RECHECK_LEVEL));
        assert!(flags.contains(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS));
        assert_eq!(
            LightQueueFlags::from_raw(u64::MAX).raw(),
            LightQueueFlags::WRITE_LEVEL
                .with(LightQueueFlags::RECHECK_LEVEL)
                .with(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS)
                .raw()
        );
    }

    #[test]
    fn packed_light_queue_entry_matches_scalable_lux_bit_layout() {
        let position = PackedLightBlockPos::from_raw(0x0abc_def0);
        let directions = LightDirectionSet::from_raw(0b10_1011);
        let flags = LightQueueFlags::EMPTY
            .with(LightQueueFlags::WRITE_LEVEL)
            .with(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS);
        let entry = PackedLightQueueEntry::from_parts(position, 31, directions, flags);

        assert_eq!(entry.block_pos(), position);
        assert_eq!(entry.level(), 15);
        assert_eq!(entry.directions(), directions);
        assert_eq!(entry.flags(), flags);
        assert!(entry.should_write_level());
        assert!(!entry.should_recheck_level());
        assert!(entry.has_sided_transparent_blocks());
        assert_eq!(entry.raw() & ((1_u64 << 28) - 1), u64::from(position.raw()));
        assert_eq!((entry.raw() >> 28) & 0x0f, 15);
        assert_eq!((entry.raw() >> 32) & 0x3f, u64::from(directions.raw()));
        assert_eq!(entry.raw() & (1_u64 << 61), 1_u64 << 61);
        assert_eq!(entry.raw() & (1_u64 << 62), 0);
        assert_eq!(entry.raw() & (1_u64 << 63), 1_u64 << 63);
    }

    #[test]
    fn packed_light_queue_entry_reads_raw_scalable_lux_values() {
        let raw = u64::MAX;
        let entry = PackedLightQueueEntry::from_raw(raw);

        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.block_pos().raw(), (1 << 28) - 1);
        assert_eq!(entry.level(), 15);
        assert_eq!(entry.directions(), LightDirectionSet::all());
        assert!(entry.should_write_level());
        assert!(entry.should_recheck_level());
        assert!(entry.has_sided_transparent_blocks());
    }

    #[test]
    fn packed_light_propagation_queue_preserves_fifo_order() {
        let first = packed_entry(1);
        let second = packed_entry(2);
        let third = packed_entry(3);
        let mut queue = PackedLightPropagationQueue::new();

        assert!(queue.is_empty());
        queue.enqueue(first);
        queue.enqueue(second);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dequeue(), Some(first));

        queue.enqueue(third);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dequeue(), Some(second));
        assert_eq!(queue.dequeue(), Some(third));
        assert_eq!(queue.dequeue(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn packed_light_propagation_queues_keep_increase_and_decrease_work_separate() {
        let decrease_entry = packed_entry(6);
        let increase_entry = packed_entry(7);
        let mut queues = PackedLightPropagationQueues::new();

        assert!(!queues.has_work());
        queues.enqueue_decrease(decrease_entry);
        queues.enqueue_increase(increase_entry);
        assert!(queues.has_work());

        assert_eq!(queues.dequeue_increase(), Some(increase_entry));
        assert_eq!(queues.dequeue_increase(), None);
        assert!(queues.has_work());

        assert_eq!(queues.dequeue_decrease(), Some(decrease_entry));
        assert_eq!(queues.dequeue_decrease(), None);
        assert!(!queues.has_work());
    }

    #[test]
    fn light_queue_entry_decrease_entries_match_vanilla_bits() {
        let all = LightQueueEntry::decrease_all_directions(7);

        assert_eq!(all.raw(), 0b11_1111_0000 | 7);
        assert_eq!(all.level(), 7);
        for direction in Direction::ALL {
            assert!(all.should_propagate_in_direction(direction));
        }
        assert!(!all.is_from_empty_shape());
        assert!(!all.is_increase_from_emission());

        let skip_north = LightQueueEntry::decrease_skip_one_direction(7, Direction::North);
        assert_eq!(skip_north.raw(), 951);
        assert!(!skip_north.should_propagate_in_direction(Direction::North));
        assert!(skip_north.should_propagate_in_direction(Direction::South));
    }

    #[test]
    fn light_queue_entry_increase_entries_match_vanilla_bits() {
        let emission = LightQueueEntry::increase_light_from_emission(15, true);
        assert_eq!(emission.raw(), 4095);
        assert_eq!(emission.level(), 15);
        assert!(emission.is_from_empty_shape());
        assert!(emission.is_increase_from_emission());

        let skip_up = LightQueueEntry::increase_skip_one_direction(10, false, Direction::Up);
        assert_eq!(skip_up.raw(), 986);
        assert!(!skip_up.is_from_empty_shape());
        assert!(!skip_up.is_increase_from_emission());
        assert!(!skip_up.should_propagate_in_direction(Direction::Up));
        assert!(skip_up.should_propagate_in_direction(Direction::Down));

        let east_only = LightQueueEntry::increase_only_one_direction(4, true, Direction::East);
        assert_eq!(east_only.raw(), 1540);
        assert!(east_only.is_from_empty_shape());
        assert!(east_only.should_propagate_in_direction(Direction::East));
        assert!(!east_only.should_propagate_in_direction(Direction::West));
    }

    #[test]
    fn light_queue_entry_sky_source_entry_selects_horizontal_and_down_directions() {
        let entry = LightQueueEntry::increase_sky_source_in_directions(&[
            Direction::Down,
            Direction::North,
            Direction::West,
        ]);

        assert_eq!(entry.raw(), 351);
        assert_eq!(entry.level(), 15);
        assert!(entry.should_propagate_in_direction(Direction::Down));
        assert!(!entry.should_propagate_in_direction(Direction::Up));
        assert!(entry.should_propagate_in_direction(Direction::North));
        assert!(!entry.should_propagate_in_direction(Direction::South));
        assert!(entry.should_propagate_in_direction(Direction::West));
        assert!(!entry.should_propagate_in_direction(Direction::East));
    }

    #[test]
    fn light_queue_entry_masks_levels_like_vanilla() {
        let entry = LightQueueEntry::increase_light_from_emission(31, false);

        assert_eq!(entry.level(), 15);
        assert_eq!(entry.raw(), 0b11_1111_0000 | 0b1000_0000_0000 | 15);
    }

    #[test]
    fn light_propagation_queue_preserves_fifo_order() {
        let first_pos = BlockPos::new(1, 2, 3);
        let second_pos = BlockPos::new(4, 5, 6);
        let first_entry = LightQueueEntry::decrease_all_directions(3);
        let second_entry =
            LightQueueEntry::increase_skip_one_direction(12, false, Direction::North);
        let mut queue = LightPropagationQueue::new();

        queue.enqueue(first_pos, first_entry);
        queue.enqueue(second_pos, second_entry);

        assert_eq!(queue.len(), 2);
        assert_eq!(
            queue.dequeue(),
            Some(QueuedLightUpdate {
                block_pos: first_pos,
                entry: first_entry,
            })
        );
        assert_eq!(
            queue.dequeue(),
            Some(QueuedLightUpdate {
                block_pos: second_pos,
                entry: second_entry,
            })
        );
        assert_eq!(queue.dequeue(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn light_propagation_queues_keep_increase_and_decrease_work_separate() {
        let decrease_pos = BlockPos::new(1, 2, 3);
        let increase_pos = BlockPos::new(4, 5, 6);
        let decrease_entry = LightQueueEntry::decrease_all_directions(4);
        let increase_entry = LightQueueEntry::increase_only_one_direction(9, true, Direction::East);
        let mut queues = LightPropagationQueues::new();

        assert!(!queues.has_work());

        queues.enqueue_decrease(decrease_pos, decrease_entry);
        queues.enqueue_increase(increase_pos, increase_entry);

        assert!(queues.has_work());
        assert_eq!(
            queues.dequeue_increase(),
            Some(QueuedLightUpdate {
                block_pos: increase_pos,
                entry: increase_entry,
            })
        );
        assert!(queues.has_work());
        assert_eq!(
            queues.dequeue_decrease(),
            Some(QueuedLightUpdate {
                block_pos: decrease_pos,
                entry: decrease_entry,
            })
        );
        assert!(!queues.has_work());
    }
}
