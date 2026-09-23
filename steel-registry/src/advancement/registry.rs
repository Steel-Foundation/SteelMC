use crate::REGISTRY;
use crate::advancement::{Advancement, positioner};
use rustc_hash::FxHashMap;
use std::fmt::{Debug, Display};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::mem::take;
use steel_utils::Identifier;
use steel_utils::serial::WriteTo;

pub type AdvancementRef = &'static Advancement;

impl WriteTo for AdvancementRef {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        (*self).write(writer)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct AdvancementNode {
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub value: AdvancementRef,
}

pub type AdvancementNodeRef = &'static AdvancementNode;

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

    #[must_use]
    pub fn root(&self) -> AdvancementNodeRef {
        let mut advancement_node = self;
        while let Some(parent) = &advancement_node.parent {
            advancement_node = &REGISTRY.advancements.adv_nodes[*parent];
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
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.value.key)
    }
}
impl Hash for AdvancementNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.key.hash(state);
    }
}

/// equivalent of the `AdvancementTree` of the minecraft source code
#[derive(Default)]
pub struct AdvancementRegistry {
    pub adv_nodes: Vec<AdvancementNode>,
    pub unloaded_advancement: Vec<AdvancementRef>,
    pub by_key: FxHashMap<Identifier, usize>,
    pub roots: Vec<usize>,
    pub tasks: Vec<usize>,
}

impl AdvancementRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn register_with_parent(&mut self, advancement: AdvancementRef, parent_idx: Option<usize>) {
        let id = advancement.key.clone();
        let node_idx = self.adv_nodes.len();
        self.adv_nodes
            .push(AdvancementNode::new(advancement, parent_idx));
        self.by_key.insert(id, node_idx);
        if let Some(parent) = parent_idx {
            let parent_node = self
                .adv_nodes
                .get_mut(parent)
                .expect("unable to get the parent node");
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
        self.register_with_parent(advancement, parent_idx);
        None
    }

    pub(crate) fn register_all(&mut self, mut advancements_to_add: Vec<AdvancementRef>) {
        while !advancements_to_add.is_empty() {
            let len_before = advancements_to_add.len();

            advancements_to_add = advancements_to_add
                .into_iter()
                .filter_map(|advancement| self.try_register(advancement))
                .collect();

            if advancements_to_add.len() == len_before && !advancements_to_add.is_empty() {
                eprintln!(
                    "Couldn't load advancements: {:?}",
                    advancements_to_add
                        .iter()
                        .map(|a| &a.key)
                        .collect::<Vec<_>>()
                );
                break;
            }
        }
    }

    pub fn register(&mut self, advancement: AdvancementRef) {
        self.unloaded_advancement.push(advancement);
    }

    #[cfg(test)]
    pub fn register_without_load(&mut self, advancements: &[AdvancementRef]) {
        self.register_all(advancements.to_vec());
    }

    #[must_use]
    #[inline]
    pub fn get_value_by_key(&self, key: &Identifier) -> Option<AdvancementRef> {
        self.by_key.get(key).map(|idx| self.adv_nodes[*idx].value)
    }

    #[must_use]
    #[inline]
    pub fn get_by_key(&self, key: &Identifier) -> Option<&AdvancementNode> {
        self.by_key
            .get(key)
            .and_then(|idx| self.adv_nodes.get(*idx))
    }

    #[must_use]
    #[inline]
    pub fn get_by_idx(&self, idx: usize) -> Option<&AdvancementNode> {
        self.adv_nodes.get(idx)
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &AdvancementNode)> + '_ {
        self.adv_nodes.iter().enumerate()
    }

    pub fn update_tree(&mut self) {
        let advancements = take(&mut self.unloaded_advancement);
        self.register_all(advancements);
        for advancement_idx in self.roots.clone() {
            let node = self.adv_nodes.get(advancement_idx);
            let Some(node) = node else {
                eprintln!("unable to get the root node with index {advancement_idx}");
                return;
            };
            if node.has_display() {
                let res = positioner::run(self, advancement_idx);
                if let Err(e) = res {
                    eprintln!("{}", e.get_message());
                }
            }
        }
    }
}
