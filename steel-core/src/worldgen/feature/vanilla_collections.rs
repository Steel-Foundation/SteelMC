use std::{cmp::Ordering, mem};

use smallvec::SmallVec;

use super::prelude::*;

const INLINE_CAPACITY: usize = 32;
const INITIAL_TABLE_CAPACITY: usize = 16;
const LOAD_FACTOR_NUMERATOR: usize = 3;
const LOAD_FACTOR_DENOMINATOR: usize = 4;
const TREEIFY_THRESHOLD: usize = 8;
const UNTREEIFY_THRESHOLD: usize = 6;
const MIN_TREEIFY_CAPACITY: usize = 64;

/// Block-position set for feature code that vanilla models as `HashSet<BlockPos>`.
///
/// This mirrors the target JDK 25 `HashMap` bucket, resize, and tree-bin behavior. Its iteration
/// order affects feature RNG and leaf-distance propagation.
#[derive(Default)]
pub(super) struct JavaBlockPosSet {
    entries: SmallVec<[BlockPos; INLINE_CAPACITY]>,
    table_capacity: usize,
    tree_bins: Vec<TreeBin>,
}

impl JavaBlockPosSet {
    pub(super) fn insert(&mut self, pos: BlockPos) -> bool {
        if self.entries.contains(&pos) {
            return false;
        }

        if self.table_capacity == 0 {
            self.table_capacity = INITIAL_TABLE_CAPACITY;
        }

        let bucket = Self::bucket(pos, self.table_capacity);
        if let Some(tree_bin_index) = self.tree_bins.iter().position(|bin| bin.bucket == bucket) {
            let (parent, root) = self.tree_bins[tree_bin_index].insert(pos);
            let Some(parent_index) = self.entries.iter().position(|&entry| entry == parent) else {
                unreachable!("tree-bin parent must be present in the iteration order");
            };
            self.entries.insert(parent_index + 1, pos);
            self.move_position_to_bucket_front(root, bucket);
        } else {
            let insert_at = self
                .entries
                .iter()
                .position(|&entry| Self::bucket(entry, self.table_capacity) > bucket)
                .unwrap_or(self.entries.len());
            self.entries.insert(insert_at, pos);

            let bucket_len = self
                .entries
                .iter()
                .filter(|&&entry| Self::bucket(entry, self.table_capacity) == bucket)
                .count();
            if bucket_len > TREEIFY_THRESHOLD {
                if self.table_capacity < MIN_TREEIFY_CAPACITY {
                    self.resize();
                } else {
                    self.treeify_bin(bucket);
                }
            }
        }
        if self.entries.len()
            > self.table_capacity * LOAD_FACTOR_NUMERATOR / LOAD_FACTOR_DENOMINATOR
        {
            self.resize();
        }
        true
    }

    pub(super) fn contains(&self, pos: BlockPos) -> bool {
        self.entries.contains(&pos)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn java_order(&self) -> impl Iterator<Item = &BlockPos> {
        self.entries.iter()
    }

    pub(super) fn java_ordered_positions(&self) -> Vec<BlockPos> {
        self.entries.to_vec()
    }

    pub(super) fn pop_java_ordered_position(&mut self) -> Option<BlockPos> {
        if self.entries.is_empty() {
            return None;
        }

        let pos = self.entries.remove(0);
        let bucket = Self::bucket(pos, self.table_capacity);
        if let Some(tree_bin_index) = self.tree_bins.iter().position(|bin| bin.bucket == bucket) {
            if self
                .entries
                .iter()
                .any(|&entry| Self::bucket(entry, self.table_capacity) == bucket)
            {
                self.tree_bins[tree_bin_index].remove(pos);
            } else {
                self.tree_bins.remove(tree_bin_index);
            }
        }
        Some(pos)
    }

    const fn bucket(pos: BlockPos, table_capacity: usize) -> usize {
        Self::spread_hash(pos) as usize & (table_capacity - 1)
    }

    const fn spread_hash(pos: BlockPos) -> u32 {
        let hash = pos
            .z()
            .wrapping_mul(31)
            .wrapping_add(pos.y())
            .wrapping_mul(31)
            .wrapping_add(pos.x()) as u32;
        hash ^ (hash >> 16)
    }

    fn resize(&mut self) {
        let old_capacity = self.table_capacity;
        self.table_capacity *= 2;
        let table_capacity = self.table_capacity;
        self.entries
            .sort_by_key(|&entry| Self::bucket(entry, table_capacity));

        let old_tree_bins = mem::take(&mut self.tree_bins);
        for mut tree_bin in old_tree_bins {
            let low = self
                .entries
                .iter()
                .copied()
                .filter(|&entry| Self::bucket(entry, table_capacity) == tree_bin.bucket)
                .collect::<SmallVec<[BlockPos; INLINE_CAPACITY]>>();
            let high_bucket = tree_bin.bucket + old_capacity;
            let high = self
                .entries
                .iter()
                .copied()
                .filter(|&entry| Self::bucket(entry, table_capacity) == high_bucket)
                .collect::<SmallVec<[BlockPos; INLINE_CAPACITY]>>();

            if low.is_empty() || high.is_empty() {
                tree_bin.bucket = if low.is_empty() {
                    high_bucket
                } else {
                    tree_bin.bucket
                };
                self.tree_bins.push(tree_bin);
                continue;
            }

            self.retreeify_split_bin(tree_bin.bucket, &low);
            self.retreeify_split_bin(high_bucket, &high);
        }
    }

    fn treeify_bin(&mut self, bucket: usize) {
        let positions = self
            .entries
            .iter()
            .copied()
            .filter(|&entry| Self::bucket(entry, self.table_capacity) == bucket)
            .collect::<SmallVec<[BlockPos; INLINE_CAPACITY]>>();
        let tree_bin = TreeBin::new(bucket, &positions);
        let root = tree_bin.root_position();
        self.tree_bins.push(tree_bin);
        self.move_position_to_bucket_front(root, bucket);
    }

    fn retreeify_split_bin(&mut self, bucket: usize, positions: &[BlockPos]) {
        if positions.len() <= UNTREEIFY_THRESHOLD {
            return;
        }
        let tree_bin = TreeBin::new(bucket, positions);
        let root = tree_bin.root_position();
        self.tree_bins.push(tree_bin);
        self.move_position_to_bucket_front(root, bucket);
    }

    fn move_position_to_bucket_front(&mut self, pos: BlockPos, bucket: usize) {
        let Some(root_index) = self.entries.iter().position(|&entry| entry == pos) else {
            return;
        };
        let Some(bucket_start) = self
            .entries
            .iter()
            .position(|&entry| Self::bucket(entry, self.table_capacity) == bucket)
        else {
            return;
        };
        if root_index != bucket_start {
            let pos = self.entries.remove(root_index);
            self.entries.insert(bucket_start, pos);
        }
    }
}

struct TreeBin {
    bucket: usize,
    nodes: Vec<TreeNode>,
    root: usize,
}

impl TreeBin {
    fn new(bucket: usize, positions: &[BlockPos]) -> Self {
        let mut tree = Self {
            bucket,
            nodes: Vec::new(),
            root: 0,
        };
        let Some((&first, remaining)) = positions.split_first() else {
            return tree;
        };
        tree.nodes.push(TreeNode::new(first));
        for &pos in remaining {
            tree.insert(pos);
        }
        tree
    }

    fn root_position(&self) -> BlockPos {
        self.nodes[self.root].pos
    }

    fn insert(&mut self, pos: BlockPos) -> (BlockPos, BlockPos) {
        let mut parent = self.root;
        loop {
            let ordering = tree_compare(pos, self.nodes[parent].pos);
            let next = if ordering.is_lt() {
                self.nodes[parent].left
            } else {
                self.nodes[parent].right
            };
            let Some(next) = next else {
                let index = self.nodes.len();
                self.nodes.push(TreeNode {
                    pos,
                    parent: Some(parent),
                    left: None,
                    right: None,
                    red: true,
                });
                if ordering.is_lt() {
                    self.nodes[parent].left = Some(index);
                } else {
                    self.nodes[parent].right = Some(index);
                }
                self.root = balance_insertion(&mut self.nodes, self.root, index);
                return (self.nodes[parent].pos, self.root_position());
            };
            parent = next;
        }
    }

    fn remove(&mut self, pos: BlockPos) {
        let Some(node) = self.find(pos) else {
            return;
        };
        let mut root = self.root;
        let left = self.nodes[node].left;
        let right = self.nodes[node].right;

        let replacement = if let (Some(left), Some(right)) = (left, right) {
            let mut successor = right;
            while let Some(next) = self.nodes[successor].left {
                successor = next;
            }
            let successor_was_red = self.nodes[successor].red;
            self.nodes[successor].red = self.nodes[node].red;
            self.nodes[node].red = successor_was_red;
            let successor_right = self.nodes[successor].right;
            let node_parent = self.nodes[node].parent;

            if successor == right {
                self.nodes[node].parent = Some(successor);
                self.nodes[successor].right = Some(node);
            } else {
                let successor_parent = self.nodes[successor].parent;
                self.nodes[node].parent = successor_parent;
                if let Some(successor_parent) = successor_parent {
                    if self.nodes[successor_parent].left == Some(successor) {
                        self.nodes[successor_parent].left = Some(node);
                    } else {
                        self.nodes[successor_parent].right = Some(node);
                    }
                }
                self.nodes[successor].right = Some(right);
                self.nodes[right].parent = Some(successor);
            }

            self.nodes[node].left = None;
            self.nodes[node].right = successor_right;
            if let Some(successor_right) = successor_right {
                self.nodes[successor_right].parent = Some(node);
            }
            self.nodes[successor].left = Some(left);
            self.nodes[left].parent = Some(successor);
            self.nodes[successor].parent = node_parent;
            if let Some(node_parent) = node_parent {
                if self.nodes[node_parent].left == Some(node) {
                    self.nodes[node_parent].left = Some(successor);
                } else {
                    self.nodes[node_parent].right = Some(successor);
                }
            } else {
                root = successor;
            }

            successor_right.unwrap_or(node)
        } else {
            left.or(right).unwrap_or(node)
        };

        if replacement != node {
            let node_parent = self.nodes[node].parent;
            self.nodes[replacement].parent = node_parent;
            if let Some(node_parent) = node_parent {
                if self.nodes[node_parent].left == Some(node) {
                    self.nodes[node_parent].left = Some(replacement);
                } else {
                    self.nodes[node_parent].right = Some(replacement);
                }
            } else {
                root = replacement;
                self.nodes[replacement].red = false;
            }
            self.nodes[node].parent = None;
            self.nodes[node].left = None;
            self.nodes[node].right = None;
        }

        root = if self.nodes[node].red {
            root
        } else {
            balance_deletion(&mut self.nodes, root, replacement)
        };

        if replacement == node
            && let Some(parent) = self.nodes[node].parent
        {
            self.nodes[node].parent = None;
            if self.nodes[parent].left == Some(node) {
                self.nodes[parent].left = None;
            } else if self.nodes[parent].right == Some(node) {
                self.nodes[parent].right = None;
            }
        }
        self.root = root;
    }

    fn find(&self, pos: BlockPos) -> Option<usize> {
        let mut node = Some(self.root);
        while let Some(index) = node {
            if self.nodes[index].pos == pos {
                return Some(index);
            }
            node = if tree_compare(pos, self.nodes[index].pos).is_lt() {
                self.nodes[index].left
            } else {
                self.nodes[index].right
            };
        }
        None
    }
}

#[derive(Clone, Copy)]
struct TreeNode {
    pos: BlockPos,
    parent: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
    red: bool,
}

impl TreeNode {
    const fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            parent: None,
            left: None,
            right: None,
            red: false,
        }
    }
}

fn tree_compare(left: BlockPos, right: BlockPos) -> Ordering {
    let left_hash = JavaBlockPosSet::spread_hash(left) as i32;
    let right_hash = JavaBlockPosSet::spread_hash(right) as i32;
    left_hash
        .cmp(&right_hash)
        .then_with(|| left.y().cmp(&right.y()))
        .then_with(|| left.z().cmp(&right.z()))
        .then_with(|| left.x().cmp(&right.x()))
}

fn balance_insertion(nodes: &mut [TreeNode], mut root: usize, mut node: usize) -> usize {
    loop {
        let Some(parent) = nodes[node].parent else {
            nodes[node].red = false;
            return node;
        };
        if !nodes[parent].red {
            return root;
        }
        let Some(grandparent) = nodes[parent].parent else {
            return root;
        };

        if nodes[grandparent].left == Some(parent) {
            let uncle = nodes[grandparent].right;
            if let Some(uncle) = uncle.filter(|&uncle| nodes[uncle].red) {
                nodes[uncle].red = false;
                nodes[parent].red = false;
                nodes[grandparent].red = true;
                node = grandparent;
                continue;
            }
            if nodes[parent].right == Some(node) {
                root = rotate_left(nodes, root, parent);
                node = parent;
            }
            let Some(parent) = nodes[node].parent else {
                return root;
            };
            let Some(grandparent) = nodes[parent].parent else {
                return root;
            };
            nodes[parent].red = false;
            nodes[grandparent].red = true;
            return rotate_right(nodes, root, grandparent);
        }

        let uncle = nodes[grandparent].left;
        if let Some(uncle) = uncle.filter(|&uncle| nodes[uncle].red) {
            nodes[uncle].red = false;
            nodes[parent].red = false;
            nodes[grandparent].red = true;
            node = grandparent;
            continue;
        }
        if nodes[parent].left == Some(node) {
            root = rotate_right(nodes, root, parent);
            node = parent;
        }
        let Some(parent) = nodes[node].parent else {
            return root;
        };
        let Some(grandparent) = nodes[parent].parent else {
            return root;
        };
        nodes[parent].red = false;
        nodes[grandparent].red = true;
        return rotate_left(nodes, root, grandparent);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "keeping both symmetric branches together makes the OpenJDK red-black deletion port auditable"
)]
fn balance_deletion(nodes: &mut [TreeNode], mut root: usize, mut node: usize) -> usize {
    loop {
        if node == root {
            return root;
        }
        let Some(mut parent) = nodes[node].parent else {
            nodes[node].red = false;
            return node;
        };
        if nodes[node].red {
            nodes[node].red = false;
            return root;
        }

        if nodes[parent].left == Some(node) {
            let mut sibling = nodes[parent].right;
            if is_red(nodes, sibling) {
                let sibling_index = sibling.unwrap_or(parent);
                nodes[sibling_index].red = false;
                nodes[parent].red = true;
                root = rotate_left(nodes, root, parent);
                let Some(new_parent) = nodes[node].parent else {
                    return root;
                };
                parent = new_parent;
                sibling = nodes[parent].right;
            }
            let Some(sibling_index) = sibling else {
                node = parent;
                continue;
            };
            let sibling_left = nodes[sibling_index].left;
            let mut sibling_right = nodes[sibling_index].right;
            if !is_red(nodes, sibling_right) && !is_red(nodes, sibling_left) {
                nodes[sibling_index].red = true;
                node = parent;
                continue;
            }
            if !is_red(nodes, sibling_right) {
                if let Some(sibling_left) = sibling_left {
                    nodes[sibling_left].red = false;
                }
                nodes[sibling_index].red = true;
                root = rotate_right(nodes, root, sibling_index);
                let Some(new_parent) = nodes[node].parent else {
                    return root;
                };
                parent = new_parent;
                sibling = nodes[parent].right;
            }
            if let Some(sibling) = sibling {
                nodes[sibling].red = nodes[parent].red;
                sibling_right = nodes[sibling].right;
                if let Some(sibling_right) = sibling_right {
                    nodes[sibling_right].red = false;
                }
            }
            nodes[parent].red = false;
            root = rotate_left(nodes, root, parent);
            node = root;
            continue;
        }

        let mut sibling = nodes[parent].left;
        if is_red(nodes, sibling) {
            let sibling_index = sibling.unwrap_or(parent);
            nodes[sibling_index].red = false;
            nodes[parent].red = true;
            root = rotate_right(nodes, root, parent);
            let Some(new_parent) = nodes[node].parent else {
                return root;
            };
            parent = new_parent;
            sibling = nodes[parent].left;
        }
        let Some(sibling_index) = sibling else {
            node = parent;
            continue;
        };
        let mut sibling_left = nodes[sibling_index].left;
        let sibling_right = nodes[sibling_index].right;
        if !is_red(nodes, sibling_left) && !is_red(nodes, sibling_right) {
            nodes[sibling_index].red = true;
            node = parent;
            continue;
        }
        if !is_red(nodes, sibling_left) {
            if let Some(sibling_right) = sibling_right {
                nodes[sibling_right].red = false;
            }
            nodes[sibling_index].red = true;
            root = rotate_left(nodes, root, sibling_index);
            let Some(new_parent) = nodes[node].parent else {
                return root;
            };
            parent = new_parent;
            sibling = nodes[parent].left;
        }
        if let Some(sibling) = sibling {
            nodes[sibling].red = nodes[parent].red;
            sibling_left = nodes[sibling].left;
            if let Some(sibling_left) = sibling_left {
                nodes[sibling_left].red = false;
            }
        }
        nodes[parent].red = false;
        root = rotate_right(nodes, root, parent);
        node = root;
    }
}

fn is_red(nodes: &[TreeNode], node: Option<usize>) -> bool {
    node.is_some_and(|node| nodes[node].red)
}

fn rotate_left(nodes: &mut [TreeNode], mut root: usize, node: usize) -> usize {
    let Some(right) = nodes[node].right else {
        return root;
    };
    nodes[node].right = nodes[right].left;
    if let Some(right_left) = nodes[right].left {
        nodes[right_left].parent = Some(node);
    }
    nodes[right].parent = nodes[node].parent;
    if let Some(parent) = nodes[node].parent {
        if nodes[parent].left == Some(node) {
            nodes[parent].left = Some(right);
        } else {
            nodes[parent].right = Some(right);
        }
    } else {
        root = right;
        nodes[root].red = false;
    }
    nodes[right].left = Some(node);
    nodes[node].parent = Some(right);
    root
}

fn rotate_right(nodes: &mut [TreeNode], mut root: usize, node: usize) -> usize {
    let Some(left) = nodes[node].left else {
        return root;
    };
    nodes[node].left = nodes[left].right;
    if let Some(left_right) = nodes[left].right {
        nodes[left_right].parent = Some(node);
    }
    nodes[left].parent = nodes[node].parent;
    if let Some(parent) = nodes[node].parent {
        if nodes[parent].right == Some(node) {
            nodes[parent].right = Some(left);
        } else {
            nodes[parent].left = Some(left);
        }
    } else {
        root = left;
        nodes[root].red = false;
    }
    nodes[left].right = Some(node);
    nodes[node].parent = Some(left);
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_positions_are_not_inserted_twice() {
        let mut set = JavaBlockPosSet::default();
        assert!(set.insert(BlockPos::new(1, 2, 3)));
        assert!(!set.insert(BlockPos::new(1, 2, 3)));
        assert_eq!(set.java_ordered_positions(), [BlockPos::new(1, 2, 3)]);
    }

    #[test]
    fn iteration_matches_java_hash_set_after_resize() {
        let mut set = JavaBlockPosSet::default();
        for x in 0..20 {
            assert!(set.insert(BlockPos::new(x, 64, x % 5)));
        }

        assert_eq!(
            set.java_ordered_positions(),
            [
                (0, 0),
                (1, 1),
                (2, 2),
                (5, 0),
                (3, 3),
                (6, 1),
                (4, 4),
                (7, 2),
                (10, 0),
                (8, 3),
                (11, 1),
                (9, 4),
                (12, 2),
                (15, 0),
                (13, 3),
                (16, 1),
                (14, 4),
                (17, 2),
                (18, 3),
                (19, 4),
            ]
            .map(|(x, z)| BlockPos::new(x, 64, z))
        );
    }

    #[test]
    fn collision_chain_triggers_java_hash_map_resize() {
        let mut set = JavaBlockPosSet::default();
        for x in (0..=128).step_by(16) {
            assert!(set.insert(BlockPos::new(x, 0, 0)));
        }

        assert_eq!(
            set.java_ordered_positions(),
            [0, 32, 64, 96, 128, 16, 48, 80, 112].map(|x| BlockPos::new(x, 0, 0))
        );
    }

    #[test]
    fn reinserted_position_returns_to_its_bucket_tail() {
        let mut set = JavaBlockPosSet::default();
        let first = BlockPos::new(0, 0, 0);
        let second = BlockPos::new(16, 0, 0);

        assert!(set.insert(first));
        assert!(set.insert(second));
        assert_eq!(set.pop_java_ordered_position(), Some(first));
        assert!(set.insert(first));

        assert_eq!(set.java_ordered_positions(), [second, first]);
    }

    #[test]
    fn pop_uses_java_bucket_order() {
        let mut set = JavaBlockPosSet::default();
        let first = BlockPos::new(15, 0, 0);
        let second = BlockPos::new(1, 0, 0);
        assert!(set.insert(first));
        assert!(set.insert(second));

        assert_eq!(set.pop_java_ordered_position(), Some(second));
        assert_eq!(set.pop_java_ordered_position(), Some(first));
        assert_eq!(set.pop_java_ordered_position(), None);
    }

    #[test]
    fn tree_bin_insertion_and_iterator_removal_match_java_hash_set() {
        let mut set = JavaBlockPosSet::default();
        for i in [
            17, 3, 29, 1, 11, 23, 7, 31, 5, 19, 13, 2, 37, 0, 41, 9, 27, 15, 35, 21, 6, 33, 25, 39,
            4, 43, 8, 45, 10, 47,
        ] {
            assert!(set.insert(BlockPos::new(i * 64, 0, 0)));
        }

        assert_eq!(
            set.java_order().map(BlockPos::x).collect::<Vec<_>>(),
            [
                1088, 192, 1856, 64, 0, 128, 704, 576, 640, 512, 1472, 1728, 1600, 448, 1984, 2240,
                2112, 2368, 2624, 2752, 2880, 3008, 2496, 320, 256, 384, 1216, 1344, 832, 960,
            ]
        );

        for expected in [1088, 192, 1856, 64, 0] {
            assert_eq!(
                set.pop_java_ordered_position(),
                Some(BlockPos::new(expected, 0, 0))
            );
        }
        assert!(set.insert(BlockPos::new(1088, 0, 0)));
        for i in [49, 51, 53, 55, 57] {
            assert!(set.insert(BlockPos::new(i * 64, 0, 0)));
        }

        assert_eq!(
            set.java_order().map(BlockPos::x).collect::<Vec<_>>(),
            [
                1984, 1216, 128, 704, 576, 640, 512, 1472, 1728, 1600, 448, 2240, 2112, 2368, 2624,
                2752, 2880, 3008, 3136, 3264, 3392, 3520, 3648, 2496, 320, 256, 384, 1344, 832,
                960, 1088,
            ]
        );
    }

    #[test]
    fn tree_bin_resize_matches_java_hash_set() {
        let mut set = JavaBlockPosSet::default();
        for i in [
            34, 23, 3, 8, 44, 25, 11, 52, 36, 10, 21, 18, 20, 9, 54, 41, 40, 42, 12, 16, 14, 47,
            35, 6, 46, 31, 53, 5, 13, 7, 51, 29, 33, 0, 24, 32, 22, 1, 27, 37, 30, 50, 48, 43, 4,
            15, 26, 2, 45, 28, 39, 38, 17, 49, 19, 55,
        ] {
            assert!(set.insert(BlockPos::new(i * 64, 0, 0)));
        }

        assert_eq!(
            set.java_order().map(BlockPos::x).collect::<Vec<_>>(),
            [
                1408, 2176, 128, 256, 0, 384, 512, 2816, 1664, 1536, 1920, 1792, 2048, 3328, 3200,
                3072, 2944, 3456, 2304, 2688, 2560, 2432, 640, 1152, 768, 896, 1024, 1280, 1344,
                448, 1472, 192, 64, 320, 1600, 1728, 1984, 2112, 1856, 704, 3008, 3264, 3136, 2880,
                3392, 3520, 2368, 2240, 2624, 2496, 2752, 576, 960, 1088, 1216, 832,
            ]
        );
    }
}
