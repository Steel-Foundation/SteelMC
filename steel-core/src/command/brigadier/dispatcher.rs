//! Dispatcher-owned command node arena.

use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    CommandNodeBuilder, NodeId, NodeKind, RegistrationError, RegistrationErrorKind,
    node::{CommandNode, UnregisteredCommandNode},
};
#[cfg(test)]
use super::{CommandSyntaxError, node::CommandContext};

static NEXT_DISPATCHER_ID: AtomicU64 = AtomicU64::new(1);

/// Owns a stable arena of command nodes.
pub(crate) struct CommandDispatcher<S> {
    id: u64,
    nodes: Vec<CommandNode<S>>,
}

impl<S> CommandDispatcher<S> {
    /// Creates an empty dispatcher containing only its root node.
    pub(crate) fn new() -> Self {
        Self {
            id: NEXT_DISPATCHER_ID.fetch_add(1, Ordering::Relaxed),
            nodes: vec![CommandNode::root()],
        }
    }

    /// Returns the stable root node ID.
    pub(crate) const fn root(&self) -> NodeId {
        NodeId::new(self.id, 0)
    }

    /// Registers and merges a literal command tree.
    pub(crate) fn register(
        &mut self,
        builder: CommandNodeBuilder<S>,
    ) -> Result<NodeId, RegistrationError> {
        let node = builder.normalize()?;
        if node.kind() != NodeKind::Literal {
            return Err(RegistrationError::new(RegistrationErrorKind::ArgumentRoot));
        }

        self.validate_redirects(&node)?;
        self.validate_merge(self.root(), &node)?;
        Ok(self.apply_merge(self.root(), node))
    }

    /// Returns a node if the ID belongs to this dispatcher.
    pub(crate) fn node(&self, id: NodeId) -> Option<&CommandNode<S>> {
        if id.dispatcher != self.id {
            return None;
        }
        self.nodes.get(id.index)
    }

    /// Returns a node's children in registration order.
    pub(crate) fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.node(id).map(|node| node.children.as_slice())
    }

    /// Returns the number of allocated nodes, including the root.
    pub(crate) const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn validate_merge(
        &self,
        parent: NodeId,
        incoming: &UnregisteredCommandNode<S>,
    ) -> Result<(), RegistrationError> {
        let Some(existing_id) = self.find_child(parent, incoming.name()) else {
            return Ok(());
        };
        let existing = &self.nodes[existing_id.index];
        existing.validate_compatible(incoming)?;
        for child in &incoming.children {
            self.validate_merge(existing_id, child)?;
        }
        Ok(())
    }

    fn validate_redirects(
        &self,
        node: &UnregisteredCommandNode<S>,
    ) -> Result<(), RegistrationError> {
        if let Some(redirect) = &node.redirect
            && self.node(redirect.target).is_none()
        {
            return Err(RegistrationError::new(
                RegistrationErrorKind::InvalidRedirectTarget {
                    target: redirect.target,
                },
            ));
        }
        for child in &node.children {
            self.validate_redirects(child)?;
        }
        Ok(())
    }

    fn apply_merge(&mut self, parent: NodeId, mut incoming: UnregisteredCommandNode<S>) -> NodeId {
        if let Some(existing_id) = self.find_child(parent, incoming.name()) {
            if incoming.command.is_some() {
                self.nodes[existing_id.index].command = incoming.command.take();
            }
            for child in incoming.children {
                self.apply_merge(existing_id, child);
            }
            return existing_id;
        }

        let node_id = NodeId::new(self.id, self.nodes.len());
        let children = incoming.children;
        self.nodes.push(CommandNode {
            data: incoming.data,
            children: Vec::new(),
            command: incoming.command,
            requirement: incoming.requirement,
            redirect: incoming.redirect,
        });
        self.nodes[parent.index].children.push(node_id);
        for child in children {
            self.apply_merge(node_id, child);
        }
        node_id
    }

    fn find_child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        let parent = self.node(parent)?;
        parent.children.iter().copied().find(|child| {
            self.nodes
                .get(child.index)
                .is_some_and(|node| node.name() == name)
        })
    }

    #[cfg(test)]
    pub(super) fn execute_node_for_test(
        &self,
        node: NodeId,
        source: S,
    ) -> Option<Result<i32, CommandSyntaxError>> {
        let command = self.node(node)?.command.as_ref()?;
        Some(command(&CommandContext::new(source)))
    }
}

impl<S> Default for CommandDispatcher<S> {
    fn default() -> Self {
        Self::new()
    }
}
