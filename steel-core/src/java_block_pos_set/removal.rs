use super::{EMPTY, JavaBlockPosSet};

impl JavaBlockPosSet {
    pub(super) fn remove_from_tree(&mut self, node: u32) {
        let mut root = self.tree_root(node);
        let left = self.links(node).left;
        let right = self.links(node).right;
        let replacement = if left != EMPTY && right != EMPTY {
            let mut successor = right;
            while self.links(successor).left != EMPTY {
                successor = self.links(successor).left;
            }
            let successor_was_red = self.links(successor).red;
            self.links_mut(successor).red = self.links(node).red;
            self.links_mut(node).red = successor_was_red;
            let successor_right = self.links(successor).right;
            let node_parent = self.links(node).parent;

            if successor == right {
                self.links_mut(node).parent = successor;
                self.links_mut(successor).right = node;
            } else {
                let successor_parent = self.links(successor).parent;
                self.links_mut(node).parent = successor_parent;
                if successor_parent != EMPTY {
                    if self.links(successor_parent).left == successor {
                        self.links_mut(successor_parent).left = node;
                    } else {
                        self.links_mut(successor_parent).right = node;
                    }
                }
                self.links_mut(successor).right = right;
                self.links_mut(right).parent = successor;
            }

            self.links_mut(node).left = EMPTY;
            self.links_mut(node).right = successor_right;
            if successor_right != EMPTY {
                self.links_mut(successor_right).parent = node;
            }
            self.links_mut(successor).left = left;
            self.links_mut(left).parent = successor;
            self.links_mut(successor).parent = node_parent;
            if node_parent == EMPTY {
                root = successor;
            } else if self.links(node_parent).left == node {
                self.links_mut(node_parent).left = successor;
            } else {
                self.links_mut(node_parent).right = successor;
            }

            if successor_right == EMPTY {
                node
            } else {
                successor_right
            }
        } else if left != EMPTY {
            left
        } else if right != EMPTY {
            right
        } else {
            node
        };

        if replacement != node {
            let parent = self.links(node).parent;
            self.links_mut(replacement).parent = parent;
            if parent == EMPTY {
                root = replacement;
                self.links_mut(replacement).red = false;
            } else if self.links(parent).left == node {
                self.links_mut(parent).left = replacement;
            } else {
                self.links_mut(parent).right = replacement;
            }
            self.links_mut(node).parent = EMPTY;
            self.links_mut(node).left = EMPTY;
            self.links_mut(node).right = EMPTY;
        }

        if !self.links(node).red {
            self.balance_deletion(root, replacement);
        }

        let parent = self.links(node).parent;
        if replacement == node && parent != EMPTY {
            self.links_mut(node).parent = EMPTY;
            if self.links(parent).left == node {
                self.links_mut(parent).left = EMPTY;
            } else if self.links(parent).right == node {
                self.links_mut(parent).right = EMPTY;
            }
        }
    }

    fn is_red(&self, node: u32) -> bool {
        node != EMPTY && self.links(node).red
    }

    fn child(&self, node: u32, left: bool) -> u32 {
        if left {
            self.links(node).left
        } else {
            self.links(node).right
        }
    }

    fn balance_deletion(&mut self, mut root: u32, mut node: u32) {
        loop {
            if node == root {
                return;
            }
            let mut parent = self.links(node).parent;
            if parent == EMPTY {
                self.links_mut(node).red = false;
                return;
            }
            if self.links(node).red {
                self.links_mut(node).red = false;
                return;
            }

            let left = self.links(parent).left == node;
            let mut sibling = self.child(parent, !left);
            if self.is_red(sibling) {
                self.links_mut(sibling).red = false;
                self.links_mut(parent).red = true;
                root = self.rotate(root, parent, left);
                parent = self.links(node).parent;
                sibling = self.child(parent, !left);
            }
            if sibling == EMPTY {
                node = parent;
                continue;
            }
            let inner = self.child(sibling, left);
            let outer = self.child(sibling, !left);
            if !self.is_red(inner) && !self.is_red(outer) {
                self.links_mut(sibling).red = true;
                node = parent;
                continue;
            }
            if !self.is_red(outer) {
                if inner != EMPTY {
                    self.links_mut(inner).red = false;
                }
                self.links_mut(sibling).red = true;
                root = self.rotate(root, sibling, !left);
                parent = self.links(node).parent;
                sibling = self.child(parent, !left);
            }
            if sibling != EMPTY {
                self.links_mut(sibling).red = self.links(parent).red;
                let outer = self.child(sibling, !left);
                if outer != EMPTY {
                    self.links_mut(outer).red = false;
                }
            }
            self.links_mut(parent).red = false;
            root = self.rotate(root, parent, left);
            node = root;
        }
    }
}
