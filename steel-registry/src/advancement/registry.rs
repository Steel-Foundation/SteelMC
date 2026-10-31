use crate::advancement::{Advancement, positioner};
use arc_swap::ArcSwap;
use rustc_hash::FxHashMap;
use std::cmp::PartialEq;
use std::fmt::{Debug, Display};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::ops::{Deref, Index};
use std::sync::Arc;
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;
use steel_utils::serial::WriteTo;

pub type AdvancementRef<'a> = &'a Advancement;
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
            Self::Static(r) => r,
            Self::Dynamic(b) => b.as_ref(),
        }
    }

    #[must_use]
    pub const fn as_static(&self) -> Option<AdvancementRef<'static>> {
        match self {
            Self::Static(r) => Some(r),
            Self::Dynamic(_) => None,
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
        self.adv_nodes.iter().map(AdvancementNode::value)
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

    /// Returns the root advancement indices in tree order.
    pub fn roots(&self) -> impl Iterator<Item = AdvancementIdx> + '_ {
        self.roots.iter().copied()
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
    /// Every advancement registered for the current pipeline.
    ///
    /// Registrations accumulate here until a reload resets the registry to vanilla after its new
    /// tree has been published. Plugins and datapacks can therefore register advancements outside
    /// reload events and still have them included in the next pass.
    registered_advancements: SyncMutex<FxHashMap<Identifier, AdvancementHolder>>,
}

impl AdvancementRegistry {
    #[must_use]
    #[inline]
    pub fn new(registry: FrozenAdvancementRegistry) -> Self {
        let registered_advancements = registry
            .adv_nodes
            .iter()
            .map(|node| (node.value().key().clone(), node.clone_value()))
            .collect();
        Self {
            registry: ArcSwap::new(Arc::new(registry)),
            registered_advancements: SyncMutex::new(registered_advancements),
        }
    }

    #[must_use]
    #[inline]
    pub fn snapshot(&self) -> Arc<FrozenAdvancementRegistry> {
        self.registry.load_full()
    }

    /// Registers an advancement. Returns `true` when the key was not registered yet, or `false`
    /// when an advancement with the same key was replaced.
    pub fn register(&self, advancement: AdvancementHolder) -> bool {
        self.registered_advancements
            .lock()
            .insert(advancement.key().clone(), advancement)
            .is_none()
    }

    /// Registers multiple advancements in the registry.
    pub fn register_all(&self, advancements: &[AdvancementHolder]) {
        let mut registered = self.registered_advancements.lock();
        for advancement in advancements {
            registered.insert(advancement.key().clone(), advancement.clone());
        }
    }

    /// Removes a registered advancement. It disappears from the tree on the next rebuild.
    ///
    /// The next [`Self::reset_to_vanilla`] restores it if it is a vanilla advancement.
    pub fn remove(&self, key: &Identifier) -> bool {
        self.registered_advancements.lock().remove(key).is_some()
    }

    /// Clears every registration and restores the vanilla advancements.
    ///
    /// Call this once a reload pass has published its tree. Registrations made outside reload
    /// events stay valid until then.
    pub fn reset_to_vanilla(&self) {
        self.registered_advancements.lock().clear();
        crate::vanilla_advancements::register_advancements(self);
    }

    pub fn with_value_by_key<R>(
        &self,
        key: &Identifier,
        f: impl FnOnce(AdvancementRef<'_>) -> R,
    ) -> Option<R> {
        let reg = self.registry.load();
        reg.value_by_key(key).map(f)
    }

    /// Builds a new tree from the current registrations without publishing it.
    ///
    /// Registrations made after this snapshot are picked up by the next call.
    #[must_use]
    pub fn prepare_tree(&self) -> FrozenAdvancementRegistry {
        let advancements = self
            .registered_advancements
            .lock()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        Self::build_tree(advancements)
    }

    /// Publishes a tree previously built by [`Self::prepare_tree`].
    pub fn replace_tree(&self, tree: FrozenAdvancementRegistry) {
        self.registry.store(Arc::new(tree));
    }

    /// Rebuilds and publishes the tree from every registered advancement.
    pub(crate) fn update_tree(&self) {
        self.replace_tree(self.prepare_tree());
    }

    fn build_tree(mut advancements: Vec<AdvancementHolder>) -> FrozenAdvancementRegistry {
        advancements.sort_by(|left, right| left.key().cmp(right.key()));

        let mut registry = FrozenAdvancementRegistry::default();
        registry.register_all(advancements);
        for advancement_idx in registry.roots.clone() {
            let Some(node) = registry.adv_nodes.get(advancement_idx.get()) else {
                eprintln!("unable to get the root node with index {advancement_idx}");
                return registry;
            };
            if node.has_display()
                && let Err(error) = positioner::run(&mut registry, advancement_idx)
            {
                eprintln!("{}", error.get_message());
            }
        }
        registry
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use crate::advancement::{AdvancementRequirement, AdvancementRewards};
    use crate::vanilla_advancements::STORY_ROOT;

    use super::*;

    fn advancement(key: &'static str, parent: Option<&'static str>) -> AdvancementHolder {
        AdvancementHolder::Dynamic(Arc::new(Advancement {
            key: Identifier::new("steel_test", key),
            parent: parent.map(|parent| Identifier::new("steel_test", parent)),
            criteria: BTreeMap::new(),
            display: None,
            send_telemetry_event: false,
            requirements: AdvancementRequirement::default(),
            rewards: AdvancementRewards::default(),
        }))
    }

    fn keys(registry: &FrozenAdvancementRegistry) -> Vec<String> {
        registry
            .get_all_key()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn rebuild_keeps_advancements_registered_before_it() {
        let registry = AdvancementRegistry::default();
        registry.register_all(&[
            advancement("root", None),
            advancement("child", Some("root")),
        ]);

        // A first build must not drain the registration map.
        assert_eq!(keys(&registry.prepare_tree()).len(), 2);
        assert_eq!(keys(&registry.prepare_tree()).len(), 2);
    }

    #[test]
    fn registering_an_existing_key_replaces_it() {
        let registry = AdvancementRegistry::default();
        assert!(registry.register(advancement("root", None)));
        assert!(!registry.register(advancement("root", None)));

        assert_eq!(registry.prepare_tree().get_all_key().len(), 1);
    }

    #[test]
    fn reset_restores_vanilla_and_drops_runtime_registrations() {
        let registry = AdvancementRegistry::default();
        registry.reset_to_vanilla();

        let vanilla_root = STORY_ROOT.key().clone();
        assert!(registry.prepare_tree().get_by_key(&vanilla_root).is_some());

        registry.remove(&vanilla_root);
        registry.register(advancement("plugin", None));
        let reloaded = registry.prepare_tree();
        assert!(reloaded.get_by_key(&vanilla_root).is_none());

        registry.reset_to_vanilla();
        let reset = registry.prepare_tree();
        assert!(reset.get_by_key(&vanilla_root).is_some());
        assert!(
            reset
                .get_by_key(&Identifier::new("steel_test", "plugin"))
                .is_none()
        );
    }
}
