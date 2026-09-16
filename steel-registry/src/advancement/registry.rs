use crate::advancement::{Advancement, positioner};
use rustc_hash::FxHashMap;
use std::fmt::Display;
use std::hash::{Hash, Hasher};
use steel_utils::Identifier;

pub type AdvancementRef = &'static Advancement;

pub struct AdvancementNode {
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub value: AdvancementRef,
}

impl AdvancementNode {
    pub fn add_child(&mut self, child: usize) {
        self.children.push(child);
    }

    #[must_use]
    pub const fn new(value: AdvancementRef, parent: Option<usize>) -> Self {
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

    #[inline]
    pub fn set_location(&mut self, x: f32, y: f32) {
        if let Some(display) = &self.value.display {
            *display.location.write() = (x, y);
        }
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
        self.value.key.hash(state);
    }
}

/// equivalent of the `AdvancementTree` of the minecraft source code
pub struct AdvancementRegistry {
    pub adv_nodes: Vec<AdvancementNode>,
    pub by_key: FxHashMap<Identifier, usize>,
    pub roots: Vec<usize>,
    pub tasks: Vec<usize>,
}

impl Default for AdvancementRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AdvancementRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            adv_nodes: Vec::new(),
            by_key: FxHashMap::default(),
            roots: Vec::new(),
            tasks: Vec::new(),
        }
    }

    fn register(&mut self, advancement: AdvancementRef, parent_idx: Option<usize>) {
        let id = advancement.key.clone();
        self.adv_nodes.push(AdvancementNode::new(advancement, parent_idx));
        let node_idx = self.adv_nodes.len();
        self.by_key.insert(id, node_idx);
        if let Some(parent) = parent_idx {
            let parent_node = self.adv_nodes.get_mut(parent).expect("unable to get the parent node");
            parent_node.add_child(node_idx);
            self.tasks.push(node_idx);
        } else {
            self.roots.push(node_idx);
        }
    }

    fn try_register(&mut self, advancement: AdvancementRef) -> Option<AdvancementRef> {
        let parent_id = &advancement.parent;
        let parent_idx: Option<usize> = match parent_id {
            Some(id) => match self.by_key.get(id) {
                Some(node) => Some(*node),
                None => return Some(advancement),
            },
            None => None,
        };
        self.register(advancement, parent_idx);
        None
    }

    fn register_all(&mut self, advancements: &[AdvancementRef]) {
        let mut advancements_to_add: Vec<AdvancementRef> = advancements.to_vec();

        while !advancements_to_add.is_empty() {
            let len_before = advancements_to_add.len();

            advancements_to_add = advancements_to_add
                .into_iter()
                .filter_map(|advancement| self.try_register(advancement))
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

    #[must_use]
    #[inline]
    pub fn get_value_by_key(&self, key: &Identifier) -> Option<AdvancementRef> {
        self.by_key.get(key).map(|idx| self.adv_nodes[*idx].value)
    }

    #[must_use]
    #[inline]
    pub fn get_by_key(&self, key: &Identifier) -> Option<&AdvancementNode> {
        self.by_key.get(key).map(|idx| &self.adv_nodes[*idx])
    }

    pub fn iter(&self) -> impl Iterator<Item=(usize, &AdvancementNode)> + '_ {
        self.adv_nodes
            .iter()
            .enumerate()
    }

    pub fn load(&mut self, advancements: &[AdvancementRef]) {
        self.register_all(advancements);
        for advancement in self.roots.clone() {
            let node = self.adv_nodes.get(advancement).expect("unable to get the node");
            if node.has_display() {
                positioner::run(self, advancement);
            }
        }
    }
}