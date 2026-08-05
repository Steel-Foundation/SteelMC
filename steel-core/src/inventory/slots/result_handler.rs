use std::sync::Arc;

use steel_registry::item_stack::ItemStack;

use crate::{
    inventory::lock::{ContainerLockGuard, ContainerRef},
    player::Player,
};

/// A trait for recipe handlers that update slots in containers according to recipes
pub trait ResultHandler: Send + Sync {
    /// The container the result is written to and read from.
    ///
    /// [`ResultSlot::new`](crate::inventory::slots::ResultSlot::new) derives the
    /// slot's container from this. Menu builders reuse that exact reference for
    /// validation and locking, so handler writes and slot reads cannot target
    /// different containers.
    fn result_container(&self) -> ContainerRef;

    /// Auxiliary containers accessed while validating or taking the result.
    ///
    /// The result container itself is already supplied by
    /// [`result_container`](Self::result_container) and must not be repeated.
    fn dependencies(&self) -> Vec<ContainerRef>;

    /// Recalculate the result based on current inputs.
    fn update_result(&self, guard: &mut ContainerLockGuard);

    /// Consume inputs when the result is taken. Return overflow remainders.
    fn on_result_taken(&self, guard: &mut ContainerLockGuard, player: &Player)
    -> Option<ItemStack>;

    /// Whether the stored result still matches the current inputs.
    fn is_result_valid(&self, guard: &ContainerLockGuard, player: &Player) -> bool;
}

impl<T: ResultHandler + ?Sized> ResultHandler for Arc<T> {
    fn result_container(&self) -> ContainerRef {
        (**self).result_container()
    }

    fn dependencies(&self) -> Vec<ContainerRef> {
        (**self).dependencies()
    }

    fn update_result(&self, guard: &mut ContainerLockGuard) {
        (**self).update_result(guard);
    }

    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) -> Option<ItemStack> {
        (**self).on_result_taken(guard, player)
    }

    fn is_result_valid(&self, guard: &ContainerLockGuard, player: &Player) -> bool {
        (**self).is_result_valid(guard, player)
    }
}
