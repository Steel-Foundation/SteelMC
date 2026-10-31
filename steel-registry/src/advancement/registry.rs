use crate::advancement::{Advancement, positioner};
use arc_swap::ArcSwap;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cmp::PartialEq;
use std::fmt::{Debug, Display};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::ops::{Deref, Index};
use std::sync::Arc;
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;
use steel_utils::serial::WriteTo;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdvancementRef<'a> {
    Static(&'static Advancement),
    Dynamic(&'a Advancement),
}

impl<'a> AdvancementRef<'a> {
    #[must_use]
    pub const fn consume(self) -> &'a Advancement {
        match self {
            AdvancementRef::Static(adv) => adv,
            AdvancementRef::Dynamic(adv) => adv,
        }
    }

    #[must_use]
    pub const fn get(&self) -> &Advancement {
        match self {
            AdvancementRef::Static(adv) => adv,
            AdvancementRef::Dynamic(adv) => adv,
        }
    }
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        match self {
            AdvancementRef::Static(adv) => &adv.key,
            AdvancementRef::Dynamic(adv) => &adv.key,
        }
    }

    #[must_use]
    pub const fn display(&self) -> Option<&crate::advancement::display::DisplayInfo> {
        match self {
            AdvancementRef::Static(adv) => adv.display.as_ref(),
            AdvancementRef::Dynamic(adv) => adv.display.as_ref(),
        }
    }

    #[must_use]
    pub const fn requirements(&self) -> &crate::advancement::AdvancementRequirement {
        match self {
            AdvancementRef::Static(adv) => &adv.requirements,
            AdvancementRef::Dynamic(adv) => &adv.requirements,
        }
    }

    #[must_use]
    pub const fn send_telemetry_event(&self) -> bool {
        match self {
            AdvancementRef::Static(adv) => adv.send_telemetry_event,
            AdvancementRef::Dynamic(adv) => adv.send_telemetry_event,
        }
    }

    #[must_use]
    pub const fn parent(&self) -> Option<&Identifier> {
        match self {
            AdvancementRef::Static(adv) => adv.parent.as_ref(),
            AdvancementRef::Dynamic(adv) => adv.parent.as_ref(),
        }
    }
}

impl Deref for AdvancementRef<'_> {
    type Target = Advancement;
    fn deref(&self) -> &Advancement {
        (*self).get()
    }
}

impl AsRef<Advancement> for AdvancementRef<'_> {
    fn as_ref(&self) -> &Advancement {
        self
    }
}

impl From<&'static Advancement> for AdvancementRef<'_> {
    fn from(r: &'static Advancement) -> Self {
        Self::Static(r)
    }
}
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum AdvancementHolder {
    Static(&'static Advancement),
    Dynamic(Arc<Advancement>),
}

impl From<&'static Advancement> for AdvancementHolder {
    fn from(r: &'static Advancement) -> Self {
        Self::Static(r)
    }
}

impl From<Arc<Advancement>> for AdvancementHolder {
    fn from(a: Arc<Advancement>) -> Self {
        Self::Dynamic(a)
    }
}

impl Clone for AdvancementHolder {
    fn clone(&self) -> Self {
        match self {
            AdvancementHolder::Static(adv) => AdvancementHolder::Static(adv),
            AdvancementHolder::Dynamic(adv) => AdvancementHolder::Dynamic(Arc::clone(adv)),
        }
    }
}

impl Deref for AdvancementHolder {
    type Target = Advancement;
    fn deref(&self) -> &Advancement {
        match self {
            Self::Static(r) => r,
            Self::Dynamic(a) => a,
        }
    }
}

impl AdvancementHolder {
    /// Returns a reference to the underlying `Advancement`.
    #[must_use]
    pub fn as_ref(&self) -> AdvancementRef<'_> {
        match self {
            Self::Static(r) => AdvancementRef::Static(r), // le 'static est conservé
            Self::Dynamic(b) => AdvancementRef::Dynamic(b), // &Box<T> -> &T par deref coercion
        }
    }
}

impl<'a> From<&'a AdvancementHolder> for AdvancementRef<'a> {
    fn from(h: &'a AdvancementHolder) -> Self {
        h.as_ref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct AdvancementIdx(usize);

impl AdvancementIdx {
    #[must_use]
    #[inline]
    pub(crate) const fn new(idx: usize) -> Self {
        Self(idx)
    }

    #[must_use]
    #[inline]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl std::ops::IndexMut<AdvancementIdx> for Vec<AdvancementNode> {
    fn index_mut(&mut self, i: AdvancementIdx) -> &mut AdvancementNode {
        &mut self[i.get()]
    }
}

impl From<AdvancementIdx> for usize {
    fn from(i: AdvancementIdx) -> usize {
        i.0
    }
}

impl Index<AdvancementIdx> for Vec<AdvancementNode> {
    type Output = AdvancementNode;
    fn index(&self, i: AdvancementIdx) -> &AdvancementNode {
        &self[i.0]
    }
}

impl Display for AdvancementIdx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AdvancementIdx({})", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct AdvancementNode {
    children: Vec<AdvancementIdx>,
    parent: Option<AdvancementIdx>,
    value: AdvancementHolder,
    x: f32,
    y: f32,
}

impl WriteTo for AdvancementNode {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.value.write(writer)?;
        self.x.write(writer)?;
        self.y.write(writer)
    }
}

pub type AdvancementNodeRef<'a> = &'a AdvancementNode;

impl AdvancementNode {
    pub fn add_child(&mut self, child: AdvancementIdx) {
        self.children.push(child);
    }

    #[must_use]
    pub const fn new(value: AdvancementHolder, parent: Option<AdvancementIdx>) -> Self {
        Self {
            value,
            parent,
            children: Vec::new(),
            x: 0f32,
            y: 0f32,
        }
    }

    #[inline]
    #[must_use]
    pub fn has_display(&self) -> bool {
        self.value.display.is_some()
    }

    #[inline]
    pub const fn set_location(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
    }

    #[inline]
    #[must_use]
    pub const fn get_location(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    #[must_use]
    #[inline]
    pub fn value(&self) -> AdvancementRef<'_> {
        self.value.as_ref()
    }

    #[must_use]
    #[inline]
    pub fn clone_value(&self) -> AdvancementHolder {
        self.value.clone()
    }

    #[must_use]
    #[inline]
    pub fn children(&self) -> Vec<AdvancementIdx> {
        self.children.clone()
    }

    #[must_use]
    #[inline]
    pub const fn parent(&self) -> Option<AdvancementIdx> {
        self.parent
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

#[derive(Default, Debug)]
pub struct FrozenAdvancementRegistry {
    pub(crate) adv_nodes: Vec<AdvancementNode>,
    pub(crate) by_key: FxHashMap<Identifier, AdvancementIdx>,
    pub(crate) roots: Vec<AdvancementIdx>,
    pub(crate) tasks: Vec<AdvancementIdx>,
}

impl FrozenAdvancementRegistry {
    fn register_with_parent(
        &mut self,
        advancement: AdvancementHolder,
        parent_idx: Option<AdvancementIdx>,
    ) {
        let id = advancement.key.clone();
        let node_idx = AdvancementIdx::new(self.adv_nodes.len());
        self.adv_nodes
            .push(AdvancementNode::new(advancement, parent_idx));
        self.by_key.insert(id, node_idx);
        if let Some(parent) = parent_idx {
            let parent_node: &mut AdvancementNode = self
                .adv_nodes
                .get_mut(parent.get())
                .expect("unable to get the parent node");
            parent_node.add_child(node_idx);
            self.tasks.push(node_idx);
        } else {
            self.roots.push(node_idx);
        }
    }

    fn try_register(&mut self, advancement: AdvancementHolder) -> Option<AdvancementHolder> {
        let parent_id = &advancement.parent;
        let parent_idx: Option<AdvancementIdx> = match parent_id {
            Some(id) => match self.by_key.get(id) {
                Some(node) => Some(*node),
                None => return Some(advancement),
            },
            None => None,
        };
        self.register_with_parent(advancement, parent_idx);
        None
    }

    #[must_use]
    #[expect(unused, reason = "it can have use cases in the future")]
    pub(crate) const fn is_idx_valid(&self, idx: AdvancementIdx) -> bool {
        idx.get() < self.adv_nodes.len()
    }

    #[must_use]
    pub fn value_by_idx(&self, idx: AdvancementIdx) -> AdvancementRef<'_> {
        self.get_by_idx(idx).value()
    }

    #[must_use]
    #[inline]
    pub fn get_by_idx(&self, idx: AdvancementIdx) -> &AdvancementNode {
        &self.adv_nodes[idx]
    }

    #[must_use]
    pub fn value_by_key(&self, key: &Identifier) -> Option<AdvancementRef<'_>> {
        let &idx = self.by_key.get(key)?;
        Some(self.adv_nodes[idx].value())
    }

    #[must_use]
    pub fn get_by_key(&self, key: &Identifier) -> Option<&AdvancementNode> {
        let &idx = self.by_key.get(key)?;
        self.adv_nodes.get(idx.get())
    }

    #[must_use]
    pub fn get_idx_by_key(&self, key: &Identifier) -> Option<AdvancementIdx> {
        self.by_key.get(key).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (AdvancementIdx, &AdvancementNode)> + '_ {
        self.adv_nodes
            .iter()
            .enumerate()
            .map(|(idx, node)| (AdvancementIdx(idx), node))
    }

    fn register_all(&mut self, mut advancements_to_add: Vec<AdvancementHolder>) {
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

    #[cfg(test)]
    pub fn register_without_load(&mut self, advancements: &[AdvancementHolder]) {
        self.register_all(advancements.to_vec());
    }

    pub fn advancements(&self) -> impl Iterator<Item = AdvancementRef<'_>> + '_ {
        self.adv_nodes.iter().map(|node| node.value())
    }

    #[must_use]
    pub fn root(&self, mut advancement_idx: AdvancementIdx) -> AdvancementIdx {
        let mut advancement_node = self.get_by_idx(advancement_idx);
        while let Some(parent) = &advancement_node.parent {
            advancement_idx = *parent;
            advancement_node = self.get_by_idx(advancement_idx);
        }
        advancement_idx
    }

    #[must_use]
    pub fn get_all_idx(&self) -> Vec<AdvancementIdx> {
        self.adv_nodes
            .iter()
            .enumerate()
            .map(|(idx, _)| AdvancementIdx(idx))
            .collect()
    }

    #[must_use]
    pub fn get_all_key(&self) -> Vec<Identifier> {
        self.adv_nodes
            .iter()
            .map(|adv| adv.value().key().clone())
            .collect()
    }
}

/// equivalent of the `AdvancementTree` of the Minecraft source code
#[derive(Debug, Default)]
pub struct AdvancementRegistry {
    registry: ArcSwap<FrozenAdvancementRegistry>,
    unloaded_advancement: SyncMutex<FxHashSet<AdvancementHolder>>,
}

impl AdvancementRegistry {
    #[must_use]
    #[inline]
    pub fn new(registry: FrozenAdvancementRegistry) -> Self {
        Self {
            registry: ArcSwap::new(Arc::new(registry)),
            unloaded_advancement: SyncMutex::new(FxHashSet::default()),
        }
    }

    #[must_use]
    #[inline]
    pub fn snapshot(&self) -> Arc<FrozenAdvancementRegistry> {
        self.registry.load_full()
    }

    /// Registers an advancement in the registry. Returns true if the advancement was added to the
    /// unloaded advancements set, false if it was already present.
    pub fn register(&mut self, advancement: AdvancementHolder) -> bool {
        self.unloaded_advancement.lock().insert(advancement)
    }
    pub fn with_value_by_key<R>(
        &self,
        key: &Identifier,
        f: impl FnOnce(AdvancementRef<'_>) -> R,
    ) -> Option<R> {
        let reg = self.registry.load();
        reg.value_by_key(key).map(f)
    }

    /// Updates the advancement tree by loading and connecting all unloaded advancements.
    pub(crate) fn update_tree(&mut self) {
        let advancements = self.unloaded_advancement.lock().drain().collect();
        let mut new_registry = FrozenAdvancementRegistry::default();
        new_registry.register_all(advancements);
        for advancement_idx in new_registry.roots.clone() {
            let node = new_registry.adv_nodes.get(advancement_idx.get());
            let Some(node) = node else {
                eprintln!("unable to get the root node with index {advancement_idx}");
                return;
            };
            if node.has_display() {
                let res = positioner::run(&mut new_registry, advancement_idx);
                if let Err(e) = res {
                    eprintln!("{}", e.get_message());
                }
            }
        }
        self.registry.store(Arc::new(new_registry));
    }

    pub fn remove_unloaded_advancement(&mut self, advancement: &Identifier) {
        self.unloaded_advancement
            .lock()
            .retain(|adv| adv.key() != advancement);
    }
}
