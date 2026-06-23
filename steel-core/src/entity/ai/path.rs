//! Path type, cache, and malus state used by vanilla mob pathfinding.

use steel_utils::{BlockPos, BlockStateId, PackedBlockPos};

use crate::entity::ai::node::Node;
use crate::entity::ai::walk::WalkPathEvaluator;
use crate::world::LevelReader;

const PATH_TYPE_CACHE_SIZE: usize = 4096;
const PATH_TYPE_CACHE_MASK: usize = PATH_TYPE_CACHE_SIZE - 1;

/// Vanilla `PathType`.
///
/// Steel stores per-mob overrides in a fixed array keyed by this enum instead
/// of Java's enum map. The observable path cost result is the same, while the
/// hot path remains cache-local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PathType {
    Blocked,
    Open,
    Walkable,
    WalkableDoor,
    Trapdoor,
    PowderSnow,
    OnTopOfPowderSnow,
    Fence,
    Lava,
    Water,
    WaterBorder,
    Rail,
    UnpassableRail,
    FireInNeighbor,
    Fire,
    DamagingInNeighbor,
    Damaging,
    DoorOpen,
    DoorWoodClosed,
    DoorIronClosed,
    Breach,
    Leaves,
    StickyHoney,
    Cocoa,
    DamageCautious,
    OnTopOfTrapdoor,
    BigMobsCloseToDanger,
}

/// Vanilla `PathComputationType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathComputationType {
    Land,
    Water,
    Air,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    nodes: Vec<Node>,
    next_node_index: usize,
    target: BlockPos,
    dist_to_target: f32,
    reached: bool,
}

impl Path {
    #[must_use]
    pub fn new(nodes: Vec<Node>, target: BlockPos, reached: bool) -> Self {
        let dist_to_target = nodes
            .last()
            .map_or(f32::MAX, |node| node.distance_manhattan_to_pos(target));
        Self {
            nodes,
            next_node_index: 0,
            target,
            dist_to_target,
            reached,
        }
    }

    pub const fn advance(&mut self) {
        self.next_node_index += 1;
    }

    #[must_use]
    pub const fn not_started(&self) -> bool {
        self.next_node_index == 0
    }

    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.next_node_index >= self.nodes.len()
    }

    #[must_use]
    pub fn end_node(&self) -> Option<&Node> {
        self.nodes.last()
    }

    #[must_use]
    pub fn node(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }

    pub fn truncate_nodes(&mut self, index: usize) {
        if self.nodes.len() > index {
            self.nodes.truncate(index);
        }
    }

    pub fn replace_node(&mut self, index: usize, replace_with: Node) -> bool {
        let Some(node) = self.nodes.get_mut(index) else {
            return false;
        };
        *node = replace_with;
        true
    }

    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub const fn next_node_index(&self) -> usize {
        self.next_node_index
    }

    pub const fn set_next_node_index(&mut self, next_node_index: usize) {
        self.next_node_index = next_node_index;
    }

    #[must_use]
    pub fn node_pos(&self, index: usize) -> Option<BlockPos> {
        self.node(index).map(Node::as_block_pos)
    }

    #[must_use]
    pub fn next_node_pos(&self) -> Option<BlockPos> {
        self.node_pos(self.next_node_index)
    }

    #[must_use]
    pub fn next_node(&self) -> Option<&Node> {
        self.node(self.next_node_index)
    }

    #[must_use]
    pub fn previous_node(&self) -> Option<&Node> {
        self.next_node_index
            .checked_sub(1)
            .and_then(|index| self.node(index))
    }

    #[must_use]
    pub fn same_as(&self, path: &Self) -> bool {
        self.nodes.len() == path.nodes.len()
            && self
                .nodes
                .iter()
                .zip(path.nodes.iter())
                .all(|(left, right)| left.hash() == right.hash())
    }

    #[must_use]
    pub const fn can_reach(&self) -> bool {
        self.reached
    }

    #[must_use]
    pub const fn target(&self) -> BlockPos {
        self.target
    }

    #[must_use]
    pub const fn dist_to_target(&self) -> f32 {
        self.dist_to_target
    }

    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
}

impl PathType {
    pub const ALL: [Self; Self::COUNT] = [
        Self::Blocked,
        Self::Open,
        Self::Walkable,
        Self::WalkableDoor,
        Self::Trapdoor,
        Self::PowderSnow,
        Self::OnTopOfPowderSnow,
        Self::Fence,
        Self::Lava,
        Self::Water,
        Self::WaterBorder,
        Self::Rail,
        Self::UnpassableRail,
        Self::FireInNeighbor,
        Self::Fire,
        Self::DamagingInNeighbor,
        Self::Damaging,
        Self::DoorOpen,
        Self::DoorWoodClosed,
        Self::DoorIronClosed,
        Self::Breach,
        Self::Leaves,
        Self::StickyHoney,
        Self::Cocoa,
        Self::DamageCautious,
        Self::OnTopOfTrapdoor,
        Self::BigMobsCloseToDanger,
    ];
    pub const COUNT: usize = 27;

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "one arm per vanilla PathType keeps the default table auditable"
    )]
    pub const fn default_malus(self) -> f32 {
        match self {
            Self::Blocked => -1.0,
            Self::Open => 0.0,
            Self::Walkable => 0.0,
            Self::WalkableDoor => 0.0,
            Self::Trapdoor => 0.0,
            Self::PowderSnow => -1.0,
            Self::OnTopOfPowderSnow => 0.0,
            Self::Fence => -1.0,
            Self::Lava => -1.0,
            Self::Water => 8.0,
            Self::WaterBorder => 8.0,
            Self::Rail => 0.0,
            Self::UnpassableRail => -1.0,
            Self::FireInNeighbor => 8.0,
            Self::Fire => 16.0,
            Self::DamagingInNeighbor => 8.0,
            Self::Damaging => -1.0,
            Self::DoorOpen => 0.0,
            Self::DoorWoodClosed => -1.0,
            Self::DoorIronClosed => -1.0,
            Self::Breach => 4.0,
            Self::Leaves => -1.0,
            Self::StickyHoney => 8.0,
            Self::Cocoa => 0.0,
            Self::DamageCautious => 0.0,
            Self::OnTopOfTrapdoor => 0.0,
            Self::BigMobsCloseToDanger => 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PathTypeSet {
    bits: u32,
}

impl PathTypeSet {
    #[must_use]
    pub const fn new() -> Self {
        Self { bits: 0 }
    }

    #[must_use]
    pub const fn from_path_type(path_type: PathType) -> Self {
        Self {
            bits: Self::bit(path_type),
        }
    }

    pub const fn insert(&mut self, path_type: PathType) {
        self.bits |= Self::bit(path_type);
    }

    #[must_use]
    pub const fn contains(self, path_type: PathType) -> bool {
        self.bits & Self::bit(path_type) != 0
    }

    #[must_use]
    pub const fn len(self) -> u32 {
        self.bits.count_ones()
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    #[must_use]
    pub const fn single(self) -> Option<PathType> {
        if self.bits.is_power_of_two() {
            Some(PathType::ALL[self.bits.trailing_zeros() as usize])
        } else {
            None
        }
    }

    pub fn iter(self) -> impl Iterator<Item = PathType> {
        PathType::ALL
            .into_iter()
            .filter(move |path_type| self.contains(*path_type))
    }

    const fn bit(path_type: PathType) -> u32 {
        1 << path_type.index()
    }
}

#[derive(Debug, Clone)]
pub struct PathfindingMalus {
    overrides: [Option<f32>; PathType::COUNT],
}

impl PathfindingMalus {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            overrides: [None; PathType::COUNT],
        }
    }

    #[must_use]
    pub fn get(&self, path_type: PathType) -> f32 {
        self.overrides[path_type.index()].unwrap_or_else(|| path_type.default_malus())
    }

    pub const fn set(&mut self, path_type: PathType, malus: f32) {
        self.overrides[path_type.index()] = Some(malus);
    }
}

impl Default for PathfindingMalus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathTypeCache {
    positions: Box<[i64]>,
    path_types: Box<[Option<PathType>]>,
}

impl PathTypeCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            positions: vec![0; PATH_TYPE_CACHE_SIZE].into_boxed_slice(),
            path_types: vec![None; PATH_TYPE_CACHE_SIZE].into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn get_or_compute(&mut self, level: &dyn LevelReader, pos: BlockPos) -> PathType {
        let key = PackedBlockPos::from(pos).as_raw();
        let index = Self::index(key);
        if self.positions[index] == key
            && let Some(path_type) = self.path_types[index]
        {
            return path_type;
        }

        let path_type = WalkPathEvaluator::path_type_from_state(level, pos);
        self.positions[index] = key;
        self.path_types[index] = Some(path_type);
        path_type
    }

    pub fn invalidate(&mut self, pos: BlockPos) {
        let key = PackedBlockPos::from(pos).as_raw();
        let index = Self::index(key);
        if self.positions[index] == key {
            self.path_types[index] = None;
        }
    }

    const fn index(pos: i64) -> usize {
        (fastutil_mix(pos as u64) as usize) & PATH_TYPE_CACHE_MASK
    }
}

impl Default for PathTypeCache {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PathfindingContext<'a> {
    level: &'a dyn LevelReader,
    cache: Option<&'a mut PathTypeCache>,
    mob_position: BlockPos,
}

impl<'a> PathfindingContext<'a> {
    #[must_use]
    pub const fn new(level: &'a dyn LevelReader, mob_position: BlockPos) -> Self {
        Self {
            level,
            cache: None,
            mob_position,
        }
    }

    #[must_use]
    pub fn with_cache(
        level: &'a dyn LevelReader,
        mob_position: BlockPos,
        cache: &'a mut PathTypeCache,
    ) -> Self {
        Self {
            level,
            cache: Some(cache),
            mob_position,
        }
    }

    #[must_use]
    pub fn get_path_type_from_state(&mut self, x: i32, y: i32, z: i32) -> PathType {
        let pos = BlockPos::new(x, y, z);
        match self.cache.as_deref_mut() {
            Some(cache) => cache.get_or_compute(self.level, pos),
            None => WalkPathEvaluator::path_type_from_state(self.level, pos),
        }
    }

    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.level.get_block_state(pos)
    }

    #[must_use]
    pub const fn level(&self) -> &dyn LevelReader {
        self.level
    }

    #[must_use]
    pub const fn mob_position(&self) -> BlockPos {
        self.mob_position
    }
}

const fn fastutil_mix(value: u64) -> u64 {
    let mixed = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mixed = mixed ^ (mixed >> 32);
    mixed ^ (mixed >> 16)
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{BlockPos, BlockStateId};

    use super::{
        PATH_TYPE_CACHE_MASK, Path, PathType, PathTypeCache, PathTypeSet, PathfindingContext,
        PathfindingMalus,
    };
    use crate::entity::ai::node::Node;
    use crate::world::LevelReader;

    struct SingleBlockLevel {
        state: BlockStateId,
    }

    impl LevelReader for SingleBlockLevel {
        fn get_block_state(&self, _pos: steel_utils::BlockPos) -> BlockStateId {
            self.state
        }

        fn raw_brightness(&self, _pos: steel_utils::BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    #[test]
    fn default_malus_matches_vanilla_path_types() {
        assert_eq!(
            PathType::Blocked.default_malus().to_bits(),
            (-1.0_f32).to_bits()
        );
        assert_eq!(
            PathType::Walkable.default_malus().to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(PathType::Water.default_malus().to_bits(), 8.0_f32.to_bits());
        assert_eq!(PathType::Fire.default_malus().to_bits(), 16.0_f32.to_bits());
        assert_eq!(
            PathType::BigMobsCloseToDanger.default_malus().to_bits(),
            4.0_f32.to_bits()
        );
    }

    #[test]
    fn malus_overrides_are_indexed_by_path_type() {
        let mut malus = PathfindingMalus::new();
        assert_eq!(malus.get(PathType::Fire).to_bits(), 16.0_f32.to_bits());

        malus.set(PathType::Fire, -1.0);

        assert_eq!(malus.get(PathType::Fire).to_bits(), (-1.0_f32).to_bits());
        assert_eq!(malus.get(PathType::Water).to_bits(), 8.0_f32.to_bits());
    }

    #[test]
    fn path_type_set_is_bit_indexed_by_vanilla_path_type_order() {
        let mut set = PathTypeSet::new();
        assert!(set.is_empty());
        set.insert(PathType::Water);
        set.insert(PathType::Fence);

        assert_eq!(set.len(), 2);
        assert!(set.contains(PathType::Water));
        assert!(set.contains(PathType::Fence));
        assert!(!set.contains(PathType::Open));
        assert_eq!(
            PathTypeSet::from_path_type(PathType::Rail).single(),
            Some(PathType::Rail)
        );
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![PathType::Fence, PathType::Water]
        );
    }

    #[test]
    fn path_type_cache_uses_vanilla_direct_mapped_size() {
        assert_eq!(PATH_TYPE_CACHE_MASK, 4095);
    }

    #[test]
    fn path_tracks_progress_and_target_distance() {
        let mut path = Path::new(
            vec![Node::new(0, 64, 0), Node::new(2, 64, 1)],
            BlockPos::new(4, 64, 1),
            false,
        );

        assert!(path.not_started());
        assert!(!path.is_done());
        assert_eq!(path.node_count(), 2);
        assert_eq!(path.next_node_pos(), Some(BlockPos::new(0, 64, 0)));
        assert_eq!(path.dist_to_target().to_bits(), 2.0_f32.to_bits());
        assert!(!path.can_reach());

        path.advance();

        assert_eq!(
            path.previous_node().map(Node::as_block_pos),
            Some(BlockPos::new(0, 64, 0))
        );
        assert_eq!(path.next_node_pos(), Some(BlockPos::new(2, 64, 1)));
    }

    #[test]
    fn path_same_as_compares_vanilla_node_identity() {
        let left = Path::new(vec![Node::new(0, 64, 0)], BlockPos::new(1, 64, 0), false);
        let mut right_node = Node::new(0, 64, 0);
        right_node.cost_malus = 8.0;
        let right = Path::new(vec![right_node], BlockPos::new(2, 64, 0), true);

        assert!(left.same_as(&right));
    }

    #[test]
    fn path_type_cache_invalidates_matching_position() {
        init_test_registry();

        let level = SingleBlockLevel {
            state: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR),
        };
        let pos = steel_utils::BlockPos::new(1, 64, 1);
        let mut cache = PathTypeCache::new();

        assert_eq!(cache.get_or_compute(&level, pos), PathType::Open);
        cache.invalidate(pos);
        assert_eq!(cache.get_or_compute(&level, pos), PathType::Open);
    }

    #[test]
    fn pathfinding_context_uses_cache_when_supplied() {
        init_test_registry();

        let level = SingleBlockLevel {
            state: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR),
        };
        let mut cache = PathTypeCache::new();
        let mut context = PathfindingContext::with_cache(
            &level,
            steel_utils::BlockPos::new(0, 64, 0),
            &mut cache,
        );

        assert_eq!(context.get_path_type_from_state(0, 64, 0), PathType::Open);
    }
}
