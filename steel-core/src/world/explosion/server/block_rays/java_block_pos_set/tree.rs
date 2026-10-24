use std::cmp::Ordering;
use steel_utils::BlockPos;

use super::{EMPTY, JavaBlockPosSet, spread};

pub(super) struct Links {
    pub(super) parent: u32,
    pub(super) left: u32,
    pub(super) right: u32,
    pub(super) prev: u32,
    pub(super) red: bool,
}

impl Links {
    const EMPTY: Self = Self {
        parent: EMPTY,
        left: EMPTY,
        right: EMPTY,
        prev: EMPTY,
        red: false,
    };
}

impl JavaBlockPosSet {
    pub(super) fn links(&self, node: u32) -> &Links {
        &self.trees[&node]
    }

    pub(super) fn links_mut(&mut self, node: u32) -> &mut Links {
        match self.trees.get_mut(&node) {
            Some(links) => links,
            None => panic!("tree bin entry has no tree links"),
        }
    }

    fn reset_links(&mut self, node: u32) {
        self.trees.insert(node, Links::EMPTY);
    }

    pub(super) fn find_in_tree(&self, mut node: u32, hash: i32, pos: BlockPos) -> bool {
        while node != EMPTY {
            let entry = &self.entries[node as usize];
            let links = self.links(node);
            let node_hash = spread(entry.pos);
            match hash.cmp(&node_hash) {
                Ordering::Less => node = links.left,
                Ordering::Greater => node = links.right,
                Ordering::Equal => {
                    if entry.pos == pos || self.find_in_tree(links.right, hash, pos) {
                        return true;
                    }
                    node = links.left;
                }
            }
        }
        false
    }

    fn goes_left(&self, node: u32, parent: u32) -> bool {
        let hash = spread(self.entries[node as usize].pos);
        let parent_hash = spread(self.entries[parent as usize].pos);
        hash < parent_hash || (hash == parent_hash && node < parent)
    }

    fn attach(&mut self, root: u32, node: u32) -> u32 {
        let mut parent = root;
        loop {
            let left = self.goes_left(node, parent);
            let child = if left {
                self.links(parent).left
            } else {
                self.links(parent).right
            };
            if child != EMPTY {
                parent = child;
                continue;
            }
            self.links_mut(node).parent = parent;
            if left {
                self.links_mut(parent).left = node;
            } else {
                self.links_mut(parent).right = node;
            }
            return parent;
        }
    }

    pub(super) fn split_tree(&mut self, index: usize, head: u32, bit: usize) {
        let mut counts = [0; 2];
        let mut current = head;
        while current != EMPTY {
            let next = self.entries[current as usize].next;
            let side =
                usize::from(spread(self.entries[current as usize].pos) as u32 as usize & bit != 0);
            let target = index + side * bit;
            let bucket = self.buckets[target];
            self.links_mut(current).prev = if bucket.head == EMPTY {
                EMPTY
            } else {
                bucket.tail
            };
            self.append(target, current);
            counts[side] += 1;
            current = next;
        }
        for side in 0..2 {
            if counts[side] <= 6 {
                // Untreeification preserves the split list's linked order.
                continue;
            }
            let target = index + side * bit;
            self.buckets[target].tail = EMPTY;
            // Java keeps an unsplit tree's shape.
            if counts[1 - side] != 0 {
                self.treeify(target);
            }
        }
    }

    pub(super) fn treeify(&mut self, bucket: usize) {
        let mut node = self.buckets[bucket].head;
        let mut root = EMPTY;
        let mut prev = EMPTY;
        while node != EMPTY {
            self.reset_links(node);
            self.links_mut(node).prev = prev;
            if root == EMPTY {
                root = node;
            } else {
                self.attach(root, node);
                root = self.balance_insertion(root, node);
            }
            prev = node;
            node = self.entries[node as usize].next;
        }
        self.buckets[bucket].tail = EMPTY;
        self.move_root_to_front(bucket, root);
    }

    pub(super) fn insert_tree(&mut self, bucket: usize, node: u32) {
        self.reset_links(node);
        let root = self.buckets[bucket].head;
        let parent = self.attach(root, node);
        let next = self.entries[parent as usize].next;
        self.entries[parent as usize].next = node;
        self.entries[node as usize].next = next;
        self.links_mut(node).prev = parent;
        if next != EMPTY {
            self.links_mut(next).prev = node;
        }
        let root = self.balance_insertion(root, node);
        self.move_root_to_front(bucket, root);
    }

    fn move_root_to_front(&mut self, bucket: usize, root: u32) {
        debug_assert_eq!(self.links(root).parent, EMPTY);
        let first = self.buckets[bucket].head;
        if root == first {
            return;
        }
        let next = self.entries[root as usize].next;
        let prev = self.links(root).prev;
        if next != EMPTY {
            self.links_mut(next).prev = prev;
        }
        if prev != EMPTY {
            self.entries[prev as usize].next = next;
        }
        self.links_mut(first).prev = root;
        self.entries[root as usize].next = first;
        self.links_mut(root).prev = EMPTY;
        self.buckets[bucket].head = root;
    }

    // Both rotations change only the tree; iteration still follows next/prev.
    fn rotate(&mut self, mut root: u32, node: u32, left: bool) -> u32 {
        let pivot = if left {
            self.links(node).right
        } else {
            self.links(node).left
        };
        let inner = if left {
            self.links(pivot).left
        } else {
            self.links(pivot).right
        };
        if left {
            self.links_mut(node).right = inner;
        } else {
            self.links_mut(node).left = inner;
        }
        if inner != EMPTY {
            self.links_mut(inner).parent = node;
        }
        let parent = self.links(node).parent;
        self.links_mut(pivot).parent = parent;
        if parent == EMPTY {
            root = pivot;
            self.links_mut(pivot).red = false;
        } else if self.links(parent).left == node {
            self.links_mut(parent).left = pivot;
        } else {
            self.links_mut(parent).right = pivot;
        }
        if left {
            self.links_mut(pivot).left = node;
        } else {
            self.links_mut(pivot).right = node;
        }
        self.links_mut(node).parent = pivot;
        root
    }

    fn balance_insertion(&mut self, mut root: u32, mut node: u32) -> u32 {
        self.links_mut(node).red = true;
        loop {
            let mut parent = self.links(node).parent;
            if parent == EMPTY {
                self.links_mut(node).red = false;
                return node;
            }
            let mut grandparent = self.links(parent).parent;
            if !self.links(parent).red || grandparent == EMPTY {
                return root;
            }
            let left = parent == self.links(grandparent).left;
            let uncle = if left {
                self.links(grandparent).right
            } else {
                self.links(grandparent).left
            };
            if uncle != EMPTY && self.links(uncle).red {
                self.links_mut(uncle).red = false;
                self.links_mut(parent).red = false;
                self.links_mut(grandparent).red = true;
                node = grandparent;
                continue;
            }
            let inner = if left {
                self.links(parent).right
            } else {
                self.links(parent).left
            };
            if node == inner {
                node = parent;
                root = self.rotate(root, node, left);
                parent = self.links(node).parent;
                grandparent = self.links(parent).parent;
            }
            self.links_mut(parent).red = false;
            if grandparent != EMPTY {
                self.links_mut(grandparent).red = true;
                root = self.rotate(root, grandparent, !left);
            }
        }
    }
}
