use crate::inventory::lock::ContainerId;

use super::Entity;

/// A living entity that can open containers.
/// Steel identifies the opener counter's owning inventory by `ContainerId`.
pub trait ContainerUser: Entity {
    /// Whether this entity currently has the given chest inventory open.
    fn has_container_open_for(&self, container: ContainerId) -> bool;

    /// Maximum distance this entity can interact with a container.
    fn container_interaction_range(&self) -> f64;
}
