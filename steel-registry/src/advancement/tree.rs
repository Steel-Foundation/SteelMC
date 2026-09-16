use crate::advancement::Advancement;
use crate::advancement::registry::{ADVANCEMENT_TREE, AdvancementRef};
use rustc_hash::FxHashMap;
use std::fmt::Display;
use std::hash::{Hash, Hasher};
use steel_utils::Identifier;

pub struct AdvancementNode {
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub value: &'static Advancement,
}

impl AdvancementNode {
    pub fn add_child(&mut self, child: usize) {
        self.children.push(child);
    }

    #[must_use]
    pub fn new(value: &'static Advancement, parent: Option<usize>) -> Self {
        Self {
            value,
            parent,
            children: Vec::new(),
        }
    }

    #[inline]
    #[must_use]
    pub const fn has_display(&self) -> bool {
        self.value.display.is_some()
    }

    pub fn root(&self) -> &AdvancementNode {
        let mut advancement_node = self;
        while let Some(parent) = &advancement_node.parent {
            advancement_node = &ADVANCEMENT_TREE.read().unwrap().nodes_vector[*parent];
        }
        advancement_node
    }
}

impl PartialEq<Self> for AdvancementNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for AdvancementNode {}

impl Display for AdvancementNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value.key)
    }
}
impl Hash for AdvancementNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

#[derive(Default)]
pub struct AdvancementTree {
    pub nodes: FxHashMap<Identifier, usize>,
    pub nodes_vector: Vec<AdvancementNode>,
    pub roots: Vec<usize>,
    pub tasks: Vec<usize>,
}

impl AdvancementTree {
    pub fn get_node_from_id(&self, id: &Identifier) -> Option<&AdvancementNode> {
        if let Some(idx) = self.nodes.get(id) {
            self.nodes_vector.get(*idx)
        } else {
            None
        }
    }

    pub fn get_node_from_idx(&self, idx: usize) -> Option<&AdvancementNode> {
        self.nodes_vector.get(idx)
    }

    pub fn get_idx(&self, id: &Identifier) -> Option<usize> {
        self.nodes.get(id).copied()
    }

    pub fn add_all(&mut self, advancements: Vec<AdvancementRef>) {
        let mut advancements_to_add: Vec<AdvancementRef> = advancements;

        while !advancements_to_add.is_empty() {
            let len_before = advancements_to_add.len();

            advancements_to_add = advancements_to_add
                .into_iter()
                .filter_map(|advancement| self.try_insert(advancement))
                .collect();

            if advancements_to_add.len() == len_before && !advancements_to_add.is_empty() {
                eprintln!(
                    "Couldn't load advancements: {:?}",
                    advancements_to_add.iter().map(|a| &a.key).collect::<Vec<_>>()
                );
                break;
            }
        }
    }

    pub fn try_insert(&mut self, advancement: AdvancementRef) -> Option<AdvancementRef> {
        let parent_id = &advancement.parent;
        let parent_idx: Option<usize> = match parent_id {
            Some(id) => match self.nodes.get(id) {
                Some(node) => Some(*node),
                None => return Some(advancement),
            },
            None => None,
        };
        let id = advancement.key.clone();
        let node = AdvancementNode::new(advancement, parent_idx);
        let node_idx = self.nodes_vector.len();
        self.nodes.insert(id, node_idx);
        if let Some(parent) = parent_idx {
            let parent_node = self.nodes_vector.get_mut(parent).unwrap();
            parent_node.add_child(node_idx);
            self.tasks.push(node_idx);
        } else {
            self.roots.push(node_idx);
        }
        self.nodes_vector.push(node);
        None
    }
}